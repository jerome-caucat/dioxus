use std::sync::Arc;

use bevy::prelude::*;
use bevy::{
    input::{ButtonInput, mouse::MouseButton},
    render::{
        render_asset::{RenderAssetUsages, RenderAssets},
        render_graph::{self, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel},
        render_resource::{TextureDimension, TextureFormat, Extent3d},
        renderer::{RenderContext, RenderDevice, RenderQueue},
        texture::GpuImage,
        Extract, RenderApp,
    },
    window::CursorMoved,
};

use anyrender_vello::VelloScenePainter;
use blitz_paint::paint_scene;
use blitz_traits::events::{BlitzMouseButtonEvent, MouseEventButton, MouseEventButtons, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use blitz_dom::Document as _;
use std::task::Context;
use dioxus::prelude::*;
use dioxus_native::{CustomPaintSource, DioxusDocument};
use rustc_hash::FxHashMap;
use vello::{
    peniko::color::AlphaColor, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene,
};
use crossbeam_channel::{Receiver, Sender};

// Constant width, height, scale factor and color schemefor example purposes
const SCALE_FACTOR: f32 = 1.0;
const WIDTH: u32 = 500;
const HEIGHT: u32 = 400;
const COLOR_SCHEME: ColorScheme = ColorScheme::Light;

pub struct DioxusInBevyPlugin {
    pub ui: fn() -> Element,
}

impl Plugin for DioxusInBevyPlugin {
    fn build(&self, app: &mut App) {
        // Create the dioxus virtual dom and the dioxus-native document
        let waker = create_waker(Box::new(|| {
            println!("Waker");
            // This should wake up and "poll" your event loop
        }));
        let vdom = VirtualDom::new(self.ui);
        let mut dioxus_doc = DioxusDocument::new(vdom, None);
        dioxus_doc.initial_build();
        dioxus_doc.set_viewport(Viewport::new(WIDTH, HEIGHT, SCALE_FACTOR, COLOR_SCHEME));
        dioxus_doc.resolve();

        app.insert_non_send_resource(dioxus_doc);
        app.insert_non_send_resource(waker);
        app.init_resource::<MousePosition>();
        app.init_resource::<PendingEvents>();
        app.add_systems(Startup, setup_ui);
        app.add_systems(Update, (update_ui, handle_mouse_events));
    }

    fn finish(&self, app: &mut App) {
        // Add the UI rendrer.
        let render_app = app.sub_app(RenderApp);
        let render_device = render_app.world().resource::<RenderDevice>();
        let device = render_device.wgpu_device();
        let vello_renderer = VelloRenderer::new(&device, RendererOptions::default()).unwrap();
        app.insert_non_send_resource(vello_renderer);

        // Setup communication between main world and render world, to send
        // and receive the texture.
        let (s, r) = crossbeam_channel::unbounded();
        app.insert_resource(MainWorldReceiver(r));
        let render_app = app.sub_app_mut(RenderApp);
        render_app.add_systems(bevy::render::ExtractSchedule, extract_texture_image);
        render_app.insert_resource(RenderWorldSender(s));

        // Add a render graph node to get the GPU texture.
        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();
        graph.add_node(TextureGetterNode, TextureGetterNodeDriver);
        graph.add_node_edge(bevy::render::graph::CameraDriverLabel, TextureGetterNode);
    }
}

#[derive(Resource, Deref)]
struct MainWorldReceiver(Receiver<wgpu::TextureView>);

#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<wgpu::TextureView>);

pub fn create_waker(callback: Box<dyn Fn() + 'static + Send + Sync>) -> std::task::Waker {
    struct DomHandle {
        callback: Box<dyn Fn() + 'static + Send + Sync>,
    }

    impl futures_util::task::ArcWake for DomHandle {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            (arc_self.callback)()
        }
    }

    futures_util::task::waker(Arc::new(DomHandle { callback }))
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, RenderLabel)]
struct TextureGetterNode;

#[derive(Default)]
struct TextureGetterNodeDriver;

impl render_graph::Node for TextureGetterNodeDriver {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        _render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // Get the GPU texture from the texture image, and send it to the
        // main world.
        if let Some(image) = world.get_resource::<ExtractedTextureImage>() {
            let gpu_images = world
                .get_resource::<RenderAssets<GpuImage>>()
                .unwrap()
                .get(&image.0)
                .unwrap();
            let texture_view: &wgpu::TextureView = &*gpu_images.texture_view;

            if let Some(sender) = world.get_resource::<RenderWorldSender>() {
                let _ = sender.send(texture_view.clone());
            }
        }

        Ok(())
    }
}

#[derive(Resource)]
pub struct TextureImage(Handle<Image>);

#[derive(Resource)]
pub struct ExtractedTextureImage(Handle<Image>);

#[derive(Resource, Default)]
pub struct MousePosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Resource, Default)]
pub struct PendingEvents {
    pub events: Vec<UiEvent>,
}

fn extract_texture_image(
    mut commands: Commands,
    texture_image: Extract<Option<Res<TextureImage>>>,
) {
    if let Some(texture_image) = texture_image.as_ref() {
        commands.insert_resource(ExtractedTextureImage(texture_image.0.clone()));
    }
}

fn setup_ui(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Create Bevy Image from the texture data
    let mut image = Image::new_fill(
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0u8; 4],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING;

    let handle = images.add(image);

    // Create a quad to display the texture
    commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(WIDTH as f32, HEIGHT as f32))),
            MeshMaterial2d(materials.add(ColorMaterial {
                texture: Some(handle.clone()),
                ..default()
            })),
            Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: isize::MAX,
            ..default()
        },
    ));

    commands.insert_resource(TextureImage(handle));
}

fn update_ui(
    mut dioxus_doc: NonSendMut<DioxusDocument>,
    vello_renderer: Option<NonSendMut<VelloRenderer>>,
    waker: NonSendMut<std::task::Waker>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    receiver: Res<MainWorldReceiver>,
    mut pending_events: ResMut<PendingEvents>,
) {
    if let (Ok(texture_view), Some(mut vello_renderer)) = (receiver.try_recv(), vello_renderer) {
        //dioxus_doc.set_viewport(Viewport::new(WIDTH, HEIGHT, SCALE_FACTOR, COLOR_SCHEME));
        //dioxus_doc.resolve();

        // Poll the vdom
        let res = dioxus_doc.poll(Context::from_waker(waker.as_ref()));
        if res {
            println!("{res}");
        }

        // Create a `VelloScenePainter` to paint into
        let mut custom_paint_sources =
            FxHashMap::<u64, Box<dyn CustomPaintSource + 'static>>::default();
        let mut scene_painter = VelloScenePainter {
            inner: Scene::new(),
            renderer: &mut vello_renderer,
            custom_paint_sources: &mut custom_paint_sources,
        };

        // Paint the document using `blitz_paint::paint_scene`
        //
        // Note: the `paint_scene` will work with any implementation of `anyrender::PaintScene`
        // so you could also write your own implementation if you want more control over rendering
        // (i.e. to render a custom renderer instead of Vello)
        paint_scene(
            &mut scene_painter,
            &dioxus_doc,
            SCALE_FACTOR as f64,
            WIDTH,
            HEIGHT,
        );

        // Extract the `vello::Scene` from the `VelloScenePainter`
        let scene = scene_painter.finish();

        let device = render_device.wgpu_device();
        let queue: &wgpu::Queue = &render_queue.into_inner();

        // Render the `vello::Scene` to the Texture using the `VelloRenderer`
        vello_renderer.render_to_texture(
                &device,
                &queue,
                &scene,
                &texture_view,
                &RenderParams {
                    base_color: AlphaColor::TRANSPARENT,
                    width: WIDTH,
                    height: HEIGHT,
                    antialiasing_method: vello::AaConfig::Msaa16,
                },
            )
            .expect("failed to render to texture");

        // Event handling - process events from Bevy
        for event in pending_events.events.drain(..) {
            println!("Event");
            dioxus_doc.handle_event(event);
        }
    }
}

fn handle_mouse_events(
    mut mouse_position: ResMut<MousePosition>,
    mut pending_events: ResMut<PendingEvents>,
    mut cursor_moved: EventReader<CursorMoved>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut last_mouse_state: Local<ButtonInput<MouseButton>>,
) {
    // Update mouse position
    for cursor_event in cursor_moved.read() {
        mouse_position.x = cursor_event.position.x;
        mouse_position.y = cursor_event.position.y;
    }

    // Handle mouse button presses
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        let was_pressed = last_mouse_state.pressed(button);
        let is_pressed = mouse_buttons.pressed(button);

        if !was_pressed && is_pressed {
            let ui_event = UiEvent::MouseDown(BlitzMouseButtonEvent {
                x: mouse_position.x,
                y: mouse_position.y,
                button: match button {
                    MouseButton::Left => MouseEventButton::Main,
                    MouseButton::Right => MouseEventButton::Secondary,
                    MouseButton::Middle => MouseEventButton::Auxiliary,
                    _ => MouseEventButton::Main,
                },
                buttons: convert_mouse_buttons(&mouse_buttons),
                mods: Modifiers::empty(),
            });
            pending_events.events.push(ui_event);
        } else if was_pressed && !is_pressed {
            let ui_event = UiEvent::MouseUp(BlitzMouseButtonEvent {
                x: mouse_position.x,
                y: mouse_position.y,
                button: match button {
                    MouseButton::Left => MouseEventButton::Main,
                    MouseButton::Right => MouseEventButton::Secondary,
                    MouseButton::Middle => MouseEventButton::Auxiliary,
                    _ => MouseEventButton::Main,
                },
                buttons: convert_mouse_buttons(&mouse_buttons),
                mods: Modifiers::empty(),
            });
            pending_events.events.push(ui_event);
        }
    }

    *last_mouse_state = mouse_buttons.clone();
}

fn convert_mouse_buttons(buttons: &ButtonInput<MouseButton>) -> MouseEventButtons {
    let mut result = MouseEventButtons::empty();
    if buttons.pressed(MouseButton::Left) {
        result |= MouseEventButtons::Primary;
    }
    if buttons.pressed(MouseButton::Right) {
        result |= MouseEventButtons::Secondary;
    }
    if buttons.pressed(MouseButton::Middle) {
        result |= MouseEventButtons::Auxiliary;
    }
    result
}
