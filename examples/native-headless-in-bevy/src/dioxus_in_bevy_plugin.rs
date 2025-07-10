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
    window::{CursorMoved, WindowResized},
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

// Constant scale factor and color scheme for example purposes
const SCALE_FACTOR: f32 = 1.0;
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
        // Initial viewport will be set in setup_ui after we get the window size
        dioxus_doc.resolve();

        app.insert_non_send_resource(dioxus_doc);
        app.insert_non_send_resource(waker);
        app.add_systems(Startup, setup_ui);
        app.add_systems(Update, (update_ui, handle_mouse_events, handle_window_resize));
    }

    fn finish(&self, app: &mut App) {
        // Add the UI rendrer
        let render_app = app.sub_app(RenderApp);
        let render_device = render_app.world().resource::<RenderDevice>();
        let device = render_device.wgpu_device();
        let vello_renderer = VelloRenderer::new(&device, RendererOptions::default()).unwrap();
        app.insert_non_send_resource(vello_renderer);

        // Setup communication between main world and render world, to send
        // and receive the texture
        let (s, r) = crossbeam_channel::unbounded();
        app.insert_resource(MainWorldReceiver(r));
        let render_app = app.sub_app_mut(RenderApp);
        render_app.add_systems(bevy::render::ExtractSchedule, extract_texture_image);
        render_app.insert_resource(RenderWorldSender(s));

        // Add a render graph node to get the GPU texture
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

fn create_ui_texture(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0u8; 4],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING;
    image
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
        // main world
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

fn extract_texture_image(
    mut commands: Commands,
    texture_image: Extract<Option<Res<TextureImage>>>,
) {
    if let Some(texture_image) = texture_image.as_ref() {
        commands.insert_resource(ExtractedTextureImage(texture_image.0.clone()));
    }
}

#[derive(Component)]
pub struct DioxusUiQuad;

fn setup_ui(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut dioxus_doc: NonSendMut<DioxusDocument>,
    windows: Query<&Window>,
) {
    // Get the window size
    let window = windows.iter().next().expect("Should have at least one window");
    let width = window.physical_width();
    let height = window.physical_height();

    println!("Initial window size: {}x{}", width, height);

    // Set the initial viewport
    dioxus_doc.set_viewport(Viewport::new(width, height, SCALE_FACTOR, COLOR_SCHEME));
    dioxus_doc.resolve();

    // Create Bevy Image from the texture data
    let image = create_ui_texture(width, height);
    let handle = images.add(image);

    // Create a quad to display the texture
    commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial2d(materials.add(ColorMaterial {
                texture: Some(handle.clone()),
                ..default()
            })),
            Transform::from_scale(Vec3::new(width as f32, height as f32, 0.0)),
            DioxusUiQuad,
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
    windows: Query<&Window>,
) {
    if let (Ok(texture_view), Some(mut vello_renderer)) = (receiver.try_recv(), vello_renderer) {
        // Event handling - process events from Bevy
        //for event in pending_events.events.drain(..) {
        //    dioxus_doc.handle_event(event);
        //    dioxus_doc.resolve();
        //}

        // Get current window size
        let window = windows.iter().next().expect("Should have at least one window");
        let width = window.physical_width();
        let height = window.physical_height();

        // Poll the vdom
        dioxus_doc.poll(Context::from_waker(waker.as_ref()));

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
            width,
            height,
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
                    width,
                    height,
                    antialiasing_method: vello::AaConfig::Msaa16,
                },
            )
            .expect("failed to render to texture");
    }
}

#[derive(Resource, Default)]
pub struct MouseState {
    pub x: f32,
    pub y: f32,
    pub buttons: MouseEventButtons,
    pub mods: Modifiers,
}

fn handle_mouse_events(
    mut dioxus_doc: NonSendMut<DioxusDocument>,
    mut cursor_moved: EventReader<CursorMoved>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut last_mouse_state: Local<MouseState>,
) {
    let mut changed = false;
    let mouse_state = &mut last_mouse_state;

    if !cursor_moved.is_empty() {
        for cursor_event in cursor_moved.read() {
            mouse_state.x = cursor_event.position.x;
            mouse_state.y = cursor_event.position.y;
            dioxus_doc.handle_event(UiEvent::MouseMove(BlitzMouseButtonEvent {
                x: mouse_state.x,
                y: mouse_state.y,
                button: Default::default(),
                buttons: mouse_state.buttons,
                mods: mouse_state.mods,
            }));
        }
        changed = true;
    }

    for (button_bevy, button_blitz) in [
        (MouseButton::Left, MouseEventButton::Main),
        (MouseButton::Right, MouseEventButton::Secondary),
        (MouseButton::Middle, MouseEventButton::Auxiliary),
    ] {
        if mouse_buttons.just_pressed(button_bevy) {
            mouse_state.buttons |= MouseEventButtons::from(button_blitz);
            dioxus_doc.handle_event(UiEvent::MouseDown(BlitzMouseButtonEvent {
                x: mouse_state.x,
                y: mouse_state.y,
                button: button_blitz,
                buttons: mouse_state.buttons,
                mods: mouse_state.mods,
            }));
            changed = true;
        }
        if mouse_buttons.just_released(button_bevy) {
            mouse_state.buttons &= !MouseEventButtons::from(button_blitz);
            dioxus_doc.handle_event(UiEvent::MouseUp(BlitzMouseButtonEvent {
                x: mouse_state.x,
                y: mouse_state.y,
                button: button_blitz,
                buttons: mouse_state.buttons,
                mods: mouse_state.mods,
            }));
            changed = true;
        }
    }

    if changed {
        dioxus_doc.resolve();
    }
}

fn handle_window_resize(
    mut dioxus_doc: NonSendMut<DioxusDocument>,
    mut resize_events: EventReader<WindowResized>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    texture_image: Option<Res<TextureImage>>,
    mut query: Query<(&mut Transform, &mut MeshMaterial2d<ColorMaterial>), With<DioxusUiQuad>>,
) {
    for resize_event in resize_events.read() {
        let width = resize_event.width as u32;
        let height = resize_event.height as u32;

        println!("Window resized to: {}x{}", width, height);

        // Update the dioxus viewport
        dioxus_doc.set_viewport(Viewport::new(width, height, SCALE_FACTOR, COLOR_SCHEME));
        dioxus_doc.resolve();

        // Create a new texture with the new size
        let new_image = create_ui_texture(width, height);
        let new_handle = images.add(new_image);

        // Update the quad mesh to match the new size
        if let Ok((mut trans, mut mat)) = query.single_mut() {
            *trans = Transform::from_scale(Vec3::new(width as f32, height as f32, 0.0));
            materials.get_mut(&mut mat.0).unwrap().texture = Some(new_handle.clone());
        }

        // Update the material with the new texture
        if let Some(texture_image) = texture_image.as_ref() {
            // Remove the old texture
            images.remove(&texture_image.0);
        }

        // Insert the new texture resource
        commands.insert_resource(TextureImage(new_handle));
    }
}
