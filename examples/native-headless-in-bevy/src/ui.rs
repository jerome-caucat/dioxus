use dioxus::prelude::*;

pub fn ui() -> Element {
    rsx! {
        style { {include_str!("./ui.css")} }
        div {
            id: "panel",
            class: "catch-events",
            div {
                id: "title",
                h1 { "Dioxus In Bevy Example" }
            }
            div {
                id: "buttons",
                button {
                    id: "button",
                    class: "button",
                    onclick: move |_| println!("Clicked !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"),
                    onmousedown: move |_| println!("Down vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv"),
                    onmouseup: move |_| println!("UPp ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
                    "Click Me!"
                }
            }
        }
    }
}
