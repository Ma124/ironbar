use std::sync::{Arc, Mutex, MutexGuard};

use color_eyre::Result;
use futures_lite::StreamExt;
use tokio::sync::watch;

use crate::{clients::compositor::mawm::dbus::Mawm1Proxy, register_fallible_client, spawn};

mod dbus;

#[derive(Debug)]
pub struct Client {
    proxy: Mawm1Proxy<'static>,
    latest_state: Arc<Mutex<MawmState>>,
    tx: watch::Sender<MawmUpdate>,
}

#[derive(Debug, Clone)]
pub struct MawmState {
    pub focused_tags: u64,
    pub occupied_tags: u64,
    pub focused_window_title: String,
}

#[derive(Debug, Clone)]
pub struct MawmUpdate {
    _private: (),
}

impl Client {
    pub async fn new() -> Result<Self> {
        let (tx, _rx) = watch::channel(MawmUpdate { _private: () });

        let dbus = Box::pin(zbus::Connection::session()).await?;

        let proxy = Mawm1Proxy::new(&dbus).await?;
        let latest_state = Arc::new(Mutex::new(query_state(&proxy).await?));

        let mut stream = proxy.receive_update().await?;

        {
            let proxy = proxy.clone();
            let latest_state = latest_state.clone();
            let tx = tx.clone();

            spawn(async move {
                while let Some(ev) = stream.next().await {
                    ev.message()
                        .body()
                        .deserialize::<()>()
                        .expect("to deserialize");

                    let new_state = match query_state(&proxy).await {
                        Ok(state) => state,
                        Err(e) => {
                            tracing::error!(?e, "query mawm state");
                            continue;
                        }
                    };

                    *latest_state
                        .lock()
                        .expect("mawm state must not be poisoned") = new_state;

                    _ = tx.send(MawmUpdate { _private: () });
                }
            });
        }

        Ok(Self {
            proxy,
            latest_state,
            tx,
        })
    }

    fn state_mut(&self) -> MutexGuard<'_, MawmState> {
        self.latest_state
            .lock()
            .expect("mawm state must not be poisoned")
    }

    pub fn state(&self) -> MawmState {
        self.state_mut().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<MawmUpdate> {
        self.tx.subscribe()
    }

    pub fn set_tag_filter(&self, mask_any: u64) {
        // TODO
    }
}

async fn query_state(proxy: &Mawm1Proxy<'_>) -> color_eyre::Result<MawmState> {
    Ok(MawmState {
        focused_tags: proxy.focused_tags().await?,
        occupied_tags: proxy.occupied_tags().await?,
        focused_window_title: proxy.focused_window_title().await?,
    })
}

register_fallible_client!(Client, mawm);
