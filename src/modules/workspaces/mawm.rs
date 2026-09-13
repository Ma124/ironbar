use std::collections::HashMap;

use color_eyre::Result;
use gtk::prelude::{BoxExt, WidgetExt};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::{
    channels::{AsyncSenderExt, BroadcastReceiverExt},
    clients::compositor::mawm::{Client, MawmUpdate},
    config::{CommonConfig, LayoutConfig, default},
    gtk_helpers::IronbarGlibExt,
    module_impl,
    modules::{
        Module, ModuleInfo, ModuleParts, WidgetContext,
        workspaces::{
            Format, WorkspaceItemContext,
            button::Button,
            button_map::{ButtonMap, Identifier},
            open_state::OpenState,
        },
    },
    spawn,
};

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(feature = "extras", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct MawmWorkspacesModule {
    #[serde(default)]
    len: Option<u8>,

    /// Map of actual workspace names to custom names.
    ///
    /// Custom names can be [images](images).
    ///
    /// If a workspace is not present in the map,
    /// it will fall back to using its actual name.
    #[serde(default)]
    name_map: HashMap<String, String>,

    /// The size to render icons at (image icons only).
    ///
    /// **Default**: `32`
    icon_size: i32,

    /// The format string for named workspaces.
    ///
    /// The following placeholders are supported:
    /// - `{label}`: The display label (from `name_map` or the workspace name).
    /// - `{name}`: The actual workspace name.
    /// - `{index}`: The workspace index.
    ///
    /// **Default**: `"{label}"`
    #[serde(default)]
    format: Format,

    // -- Common --
    /// See [layout options](module-level-options#layout)
    #[serde(default, flatten)]
    layout: LayoutConfig,

    /// See [common options](module-level-options#common-options).
    #[serde(flatten)]
    pub common: Option<CommonConfig>,
}

impl Default for MawmWorkspacesModule {
    fn default() -> Self {
        Self {
            len: None,
            name_map: HashMap::default(),
            icon_size: default::IconSize::Normal as i32,
            format: Format::default(),
            layout: LayoutConfig::default(),
            common: Some(CommonConfig::default()),
        }
    }
}

impl Module<gtk::Box> for MawmWorkspacesModule {
    type SendMessage = MawmUpdate;
    type ReceiveMessage = i64;

    module_impl!("mawm_workspaces");

    fn spawn_controller(
        &self,
        _info: &ModuleInfo,
        context: &WidgetContext<Self::SendMessage, Self::ReceiveMessage>,
        mut rx: mpsc::Receiver<Self::ReceiveMessage>,
    ) -> Result<()> {
        let client = context.try_client::<Client>()?;

        let mut updates = client.subscribe();
        let tx = context.tx.clone();

        spawn(async move {
            while let Ok(()) = updates.changed().await {
                let update = updates.borrow().clone();

                tx.send_update(update).await;
            }
        });

        spawn(async move {
            while let Some(ev) = rx.recv().await {
                client.set_tag_filter(1 << (ev as u64));
            }
        });

        Ok(())
    }

    fn into_widget(
        self,
        context: WidgetContext<Self::SendMessage, Self::ReceiveMessage>,
        info: &ModuleInfo,
    ) -> Result<ModuleParts<gtk::Box>> {
        let container = gtk::Box::new(self.layout.orientation(info), 0);
        container.add_css_class("workspaces");

        let mut button_map = ButtonMap::new();

        let (format_named, format_unnamed) = self.format.resolve();

        let item_context = WorkspaceItemContext {
            name_map: self.name_map.clone(),
            icon_size: self.icon_size,
            image_provider: context.ironbar.image_provider(),
            tx: context.controller_tx.clone(),
            format_named,
            format_unnamed,
        };

        for index in 0..self.len.unwrap_or(9) {
            let btn = Button::new(
                index as _,
                index as _,
                &format!("{}", index + 1),
                info.output_name,
                OpenState::Closed,
                &item_context,
            );

            btn.button().set_tag("workspace_index", index);
            container.append(btn.button());
            button_map.insert(Identifier::Name(format!("{index}")), btn);
        }

        let client = context.try_client::<Client>()?;
        let mut update = move || {
            let state = client.state();

            for index in 0..self.len.unwrap_or(9) {
                let mask = 1 << u64::from(index);

                let btn = button_map
                    .find_button_by_id_mut(index.into())
                    .expect("ids are dense integers in range");

                btn.set_open_state(if state.focused_tags & mask != 0 {
                    OpenState::Focused
                } else if state.occupied_tags & mask != 0 {
                    OpenState::Visible
                } else {
                    OpenState::Closed
                });
            }
        };

        update();
        context.subscribe().recv_glib((), move |(), _event| {
            update();
        });

        Ok(ModuleParts {
            widget: container,
            popup: None,
        })
    }
}
