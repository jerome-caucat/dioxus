mod bevy_scene_plugin;
mod dioxus_in_bevy_plugin;

use bevy::prelude::*;
use dioxus::prelude::*;
use crate::bevy_scene_plugin::BevyScenePlugin;
use crate::dioxus_in_bevy_plugin::DioxusInBevyPlugin;

static CSS: dioxus::prelude::Asset = asset!("/assets/main.css");

fn ui() -> Element {
    rsx! {
        div { 
            id: "title",
            style: "background-color: #FFFFFF; color: #0000FF; text-align: center; padding: 10px;",
            h1 { "Dioxus In Bevy Example" }
        }
        div { 
            id: "buttons",
            style: "display: flex; justify-content: center; gap: 20px; padding: 20px;",
            button {
                id: "button",
                style: "padding: 10px 20px; font-size: 16px; background-color: #4CAF50; color: white; border: none; border-radius: 5px;",
                onclick: move |_| println!("Clicked"),
                "Click Me!"
            }
        }
    }
}

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(DioxusInBevyPlugin {ui})
        .add_plugins(BevyScenePlugin {})
        .run();
}

