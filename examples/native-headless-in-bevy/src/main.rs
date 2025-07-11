mod bevy_scene_plugin;
mod dioxus_in_bevy_plugin;
mod ui;

use bevy::prelude::*;
use crate::bevy_scene_plugin::BevyScenePlugin;
use crate::dioxus_in_bevy_plugin::DioxusInBevyPlugin;
use crate::ui::{ui, UIMessage};

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    let (ui_sender, ui_receiver) = crossbeam_channel::unbounded();

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(DioxusInBevyPlugin::<UIMessage> {ui, ui_sender})
        .add_plugins(BevyScenePlugin {ui_receiver})
        .run();
}

