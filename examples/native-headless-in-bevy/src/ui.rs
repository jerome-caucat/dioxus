use dioxus::prelude::*;
use crossbeam_channel::Sender;
use paste::paste;

macro_rules! define_ui_state {
    (
        $($field:ident : $typ:ty = $default:expr),* $(,)?
    ) => { paste! {
        #[derive(Clone, Copy)]
        pub struct UiState {
            $($field: Signal<$typ>,)*
        }

        impl UiState {
            $(pub const [<DEFAULT_ $field:upper>]: $typ = $default;)*
            fn default() -> Self {
                Self {
                    $($field: Signal::new($default),)*
                }
            }
        }

        pub enum UIMessage {
            $([<$field:camel>]($typ))*
        }
    }};
}

define_ui_state! {
    cube_color: [f32; 4] = [0.0, 0.0, 1.0, 1.0],
}

pub fn ui(ui_sender: Sender<UIMessage>) -> Element {
    let mut state = use_context_provider(|| UiState::default());

    use_effect(move || {
        println!("Color changed to {:?}", state.cube_color);
        ui_sender.send(UIMessage::CubeColor((state.cube_color)())).unwrap();
    });

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
                    onclick: move |_| {
                        let mut color = state.cube_color.write();
                        println!("Button clicked {:?}", *color);
                        color[0] += 0.1;
                    },
                    onmousedown: move |_| println!("Button down"),
                    onmouseup: move |_| println!("Button up"),
                    "Click Me!"
                }
            }
        }
    }
}
