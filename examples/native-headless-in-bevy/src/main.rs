mod bevy_scene_plugin;
mod dioxus_in_bevy_plugin;

use bevy::prelude::*;
use dioxus::prelude::*;
use crate::bevy_scene_plugin::BevyScenePlugin;
use crate::dioxus_in_bevy_plugin::DioxusInBevyPlugin;

static CSS: dioxus::prelude::Asset = asset!("/assets/main.css");

fn ui() -> Element {
    rsx! {
        document::Stylesheet { href: CSS }
        div { id: "title",
            h1 { "Dioxus In Bevy Example" }
        }
        div { id: "buttons",
            button { id: "button", class: "button", "button" }
        }
    }
}

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(BevyScenePlugin {})
        .add_plugins(DioxusInBevyPlugin {ui})
        .run();
}

