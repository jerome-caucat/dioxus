mod bevy_scene_plugin;
mod dioxus_in_bevy_plugin;
mod ui;

use bevy::prelude::*;
use crate::bevy_scene_plugin::BevyScenePlugin;
use crate::dioxus_in_bevy_plugin::DioxusInBevyPlugin;
use crate::ui::ui;

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(DioxusInBevyPlugin {ui})
        .add_plugins(BevyScenePlugin {})
        .run();
}

