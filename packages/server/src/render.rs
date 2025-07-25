//! A shared pool of renderers for efficient server side rendering.
use crate::{document::ServerDocument, ProvideServerContext, ServeConfig};
use crate::{
    streaming::{Mount, StreamingRenderer},
    DioxusServerContext,
};
use dioxus_cli_config::base_path;
use dioxus_core::{
    has_context, provide_error_boundary, DynamicNode, ErrorContext, ScopeId, SuspenseContext,
    VNode, VirtualDom,
};
use dioxus_fullstack_hooks::history::FullstackHistory;
use dioxus_fullstack_hooks::{StreamingContext, StreamingStatus};
use dioxus_fullstack_protocol::{HydrationContext, SerializedHydrationData};
use dioxus_isrg::{CachedRender, IncrementalRendererError, RenderFreshness};
use dioxus_router::ParseRouteError;
use dioxus_ssr::Renderer;
use futures_channel::mpsc::Sender;
use futures_util::{Stream, StreamExt};
use std::{collections::HashMap, fmt::Write, future::Future, rc::Rc, sync::Arc, sync::RwLock};
use tokio::task::JoinHandle;

use crate::StreamingMode;

/// A suspense boundary that is pending with a placeholder in the client
struct PendingSuspenseBoundary {
    mount: Mount,
    children: Vec<ScopeId>,
}

/// Spawn a task in the background. If wasm is enabled, this will use the single threaded tokio runtime
fn spawn_platform<Fut>(f: impl FnOnce() -> Fut + Send + 'static) -> JoinHandle<Fut::Output>
where
    Fut: Future + 'static,
    Fut::Output: Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        use tokio_util::task::LocalPoolHandle;
        static TASK_POOL: std::sync::OnceLock<LocalPoolHandle> = std::sync::OnceLock::new();

        let pool = TASK_POOL.get_or_init(|| {
            let threads = std::thread::available_parallelism()
                .unwrap_or(std::num::NonZeroUsize::new(1).unwrap());
            LocalPoolHandle::new(threads.into())
        });

        pool.spawn_pinned(f)
    }
    #[cfg(target_arch = "wasm32")]
    {
        tokio::task::spawn_local(f())
    }
}

fn in_root_scope<T>(virtual_dom: &VirtualDom, f: impl FnOnce() -> T) -> T {
    virtual_dom.in_runtime(|| ScopeId::ROOT.in_runtime(f))
}

/// Errors that can occur during server side rendering before the initial chunk is sent down
pub enum SSRError {
    /// An error from the incremental renderer. This should result in a 500 code
    Incremental(IncrementalRendererError),
    /// An error from the dioxus router. This should result in a 404 code
    Routing(ParseRouteError),
}

struct SsrRendererPool {
    renderers: RwLock<Vec<Renderer>>,
    incremental_cache: Option<RwLock<dioxus_isrg::IncrementalRenderer>>,
}

impl SsrRendererPool {
    fn new(
        initial_size: usize,
        incremental: Option<dioxus_isrg::IncrementalRendererConfig>,
    ) -> Self {
        let renderers = RwLock::new((0..initial_size).map(|_| pre_renderer()).collect());
        Self {
            renderers,
            incremental_cache: incremental.map(|cache| RwLock::new(cache.build())),
        }
    }

    /// Look for a cached route in the incremental cache and send it into the render channel if it exists
    fn check_cached_route(
        &self,
        route: &str,
        render_into: &mut Sender<Result<String, dioxus_isrg::IncrementalRendererError>>,
    ) -> Option<RenderFreshness> {
        if let Some(incremental) = &self.incremental_cache {
            if let Ok(mut incremental) = incremental.write() {
                match incremental.get(route) {
                    Ok(Some(cached_render)) => {
                        let CachedRender {
                            freshness,
                            response,
                            ..
                        } = cached_render;
                        _ = render_into.start_send(String::from_utf8(response.to_vec()).map_err(
                            |err| dioxus_isrg::IncrementalRendererError::Other(Box::new(err)),
                        ));
                        return Some(freshness);
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to get route \"{route}\" from incremental cache: {e}"
                        );
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// Render a virtual dom into a stream. This method will return immediately and continue streaming the result in the background
    /// The streaming is canceled when the stream the function returns is dropped
    async fn render_to(
        self: Arc<Self>,
        cfg: &ServeConfig,
        route: String,
        virtual_dom_factory: impl FnOnce() -> VirtualDom + Send + Sync + 'static,
        server_context: &DioxusServerContext,
    ) -> Result<
        (
            RenderFreshness,
            impl Stream<Item = Result<String, dioxus_isrg::IncrementalRendererError>>,
        ),
        SSRError,
    > {
        struct ReceiverWithDrop {
            receiver: futures_channel::mpsc::Receiver<
                Result<String, dioxus_isrg::IncrementalRendererError>,
            >,
            cancel_task: Option<tokio::task::JoinHandle<()>>,
        }

        impl Stream for ReceiverWithDrop {
            type Item = Result<String, dioxus_isrg::IncrementalRendererError>;

            fn poll_next(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                self.receiver.poll_next_unpin(cx)
            }
        }

        // When we drop the stream, we need to cancel the task that is feeding values to the stream
        impl Drop for ReceiverWithDrop {
            fn drop(&mut self) {
                if let Some(cancel_task) = self.cancel_task.take() {
                    cancel_task.abort();
                }
            }
        }

        let (mut into, rx) = futures_channel::mpsc::channel::<
            Result<String, dioxus_isrg::IncrementalRendererError>,
        >(1000);

        let (initial_result_tx, initial_result_rx) = futures_channel::oneshot::channel();

        // before we even spawn anything, we can check synchronously if we have the route cached
        if let Some(freshness) = self.check_cached_route(&route, &mut into) {
            return Ok((
                freshness,
                ReceiverWithDrop {
                    receiver: rx,
                    cancel_task: None,
                },
            ));
        }

        let wrapper = FullstackHTMLTemplate { cfg: cfg.clone() };

        let server_context = server_context.clone();
        let mut renderer = self
            .renderers
            .write()
            .unwrap()
            .pop()
            .unwrap_or_else(pre_renderer);

        let myself = self.clone();
        let streaming_mode = cfg.streaming_mode;

        let create_render_future = move || async move {
            let mut virtual_dom = virtual_dom_factory();
            let document = Rc::new(ServerDocument::default());
            virtual_dom.provide_root_context(document.clone());
            // If there is a base path, trim the base path from the route and add the base path formatting to the
            // history provider
            let history;
            if let Some(base_path) = base_path() {
                let base_path = base_path.trim_matches('/');
                let base_path = format!("/{base_path}");
                let route = route.strip_prefix(&base_path).unwrap_or(&route);
                history =
                    dioxus_history::MemoryHistory::with_initial_path(route).with_prefix(base_path);
            } else {
                history = dioxus_history::MemoryHistory::with_initial_path(&route);
            }
            // Wrap the memory history in a fullstack history provider to provide the initial route for hydration
            let history = FullstackHistory::new_server(history);

            let streaming_context = in_root_scope(&virtual_dom, StreamingContext::new);
            virtual_dom.provide_root_context(Rc::new(history) as Rc<dyn dioxus_history::History>);
            virtual_dom.provide_root_context(document.clone() as Rc<dyn dioxus_document::Document>);
            virtual_dom.provide_root_context(streaming_context);

            // rebuild the virtual dom
            virtual_dom.rebuild_in_place();

            // If streaming is disabled, wait for the virtual dom to finish all suspense work
            // before rendering anything
            if streaming_mode == StreamingMode::Disabled {
                virtual_dom.wait_for_suspense().await;
            }
            // Otherwise, just wait for the streaming context to signal the initial chunk is ready
            else {
                loop {
                    // Check if the router has finished and set the streaming context to finished
                    let streaming_context_finished =
                        in_root_scope(&virtual_dom, || streaming_context.current_status())
                            == StreamingStatus::InitialChunkCommitted;
                    // Or if this app isn't using the router and has finished suspense
                    let suspense_finished = !virtual_dom.suspended_tasks_remaining();
                    if streaming_context_finished || suspense_finished {
                        break;
                    }

                    // Wait for new async work that runs during suspense (mainly use_server_futures)
                    virtual_dom.wait_for_suspense_work().await;

                    // Do that async work
                    virtual_dom.render_suspense_immediate().await;
                }
            }

            // check if there are any errors
            let errors = virtual_dom.in_runtime(|| {
                let error_context: ErrorContext = ScopeId::APP
                    .consume_context()
                    .expect("The root should be under an error boundary");
                let errors = error_context.errors();
                errors.to_vec()
            });
            if errors.is_empty() {
                // If routing was successful, we can return a 200 status and render into the stream
                _ = initial_result_tx.send(Ok(()));
            } else {
                // If there was an error while routing, return the error with a 400 status
                // Return a routing error if any of the errors were a routing error
                let routing_error = errors.iter().find_map(|err| err.downcast().cloned());
                if let Some(routing_error) = routing_error {
                    _ = initial_result_tx.send(Err(SSRError::Routing(routing_error)));
                    return;
                }
                #[derive(thiserror::Error, Debug)]
                #[error("{0}")]
                pub struct ErrorWhileRendering(String);
                let mut all_errors = String::new();
                for error in errors {
                    all_errors += &error.to_string();
                    all_errors += "\n"
                }
                let error = ErrorWhileRendering(all_errors);
                _ = initial_result_tx.send(Err(SSRError::Incremental(
                    IncrementalRendererError::Other(Box::new(error)),
                )));
                return;
            }

            let mut pre_body = String::new();

            if let Err(err) = wrapper.render_head(&mut pre_body, &virtual_dom) {
                _ = into.start_send(Err(err));
                return;
            }

            let stream = Arc::new(StreamingRenderer::new(pre_body, into));
            let scope_to_mount_mapping = Arc::new(RwLock::new(HashMap::new()));

            renderer.pre_render = true;
            {
                let scope_to_mount_mapping = scope_to_mount_mapping.clone();
                let stream = stream.clone();
                renderer.set_render_components(streaming_render_component_callback(
                    stream,
                    scope_to_mount_mapping,
                ));
            }

            macro_rules! throw_error {
                ($e:expr) => {
                    stream.close_with_error($e);
                    return;
                };
            }

            // Render the initial frame with loading placeholders
            let mut initial_frame = renderer.render(&virtual_dom);

            // Along with the initial frame, we render the html after the main element, but before the body tag closes. This should include the script that starts loading the wasm bundle.
            if let Err(err) = wrapper.render_after_main(&mut initial_frame, &virtual_dom) {
                throw_error!(err);
            }
            stream.render(initial_frame);

            // After the initial render, we need to resolve suspense
            while virtual_dom.suspended_tasks_remaining() {
                virtual_dom.wait_for_suspense_work().await;
                let resolved_suspense_nodes = virtual_dom.render_suspense_immediate().await;

                // Just rerender the resolved nodes
                for scope in resolved_suspense_nodes {
                    let pending_suspense_boundary = {
                        let mut lock = scope_to_mount_mapping.write().unwrap();
                        lock.remove(&scope)
                    };
                    // If the suspense boundary was immediately removed, it may not have a mount. We can just skip resolving it
                    if let Some(pending_suspense_boundary) = pending_suspense_boundary {
                        let mut resolved_chunk = String::new();
                        // After we replace the placeholder in the dom with javascript, we need to send down the resolved data so that the client can hydrate the node
                        let render_suspense = |into: &mut String| {
                            renderer.reset_hydration();
                            renderer.render_scope(into, &virtual_dom, scope)
                        };
                        let resolved_data = serialize_server_data(&virtual_dom, scope);
                        if let Err(err) = stream.replace_placeholder(
                            pending_suspense_boundary.mount,
                            render_suspense,
                            resolved_data,
                            &mut resolved_chunk,
                        ) {
                            throw_error!(dioxus_isrg::IncrementalRendererError::RenderError(err));
                        }

                        stream.render(resolved_chunk);
                        // Freeze the suspense boundary to prevent future reruns of any child nodes of the suspense boundary
                        if let Some(suspense) =
                            SuspenseContext::downcast_suspense_boundary_from_scope(
                                &virtual_dom.runtime(),
                                scope,
                            )
                        {
                            suspense.freeze();
                            // Go to every child suspense boundary and add an error boundary. Since we cannot rerun any nodes above the child suspense boundary,
                            // we need to capture the errors and send them to the client as it resolves
                            virtual_dom.in_runtime(|| {
                                for &suspense_scope in pending_suspense_boundary.children.iter() {
                                    start_capturing_errors(suspense_scope);
                                }
                            });
                        }
                    }
                }
            }

            // After suspense is done, we render the html after the body
            let mut post_streaming = String::new();

            if let Err(err) = wrapper.render_after_body(&mut post_streaming) {
                throw_error!(err);
            }

            // If incremental rendering is enabled, add the new render to the cache without the streaming bits
            if let Some(incremental) = &self.incremental_cache {
                let mut cached_render = String::new();
                if let Err(err) = wrapper.render_head(&mut cached_render, &virtual_dom) {
                    throw_error!(err);
                }
                renderer.reset_hydration();
                if let Err(err) = renderer.render_to(&mut cached_render, &virtual_dom) {
                    throw_error!(dioxus_isrg::IncrementalRendererError::RenderError(err));
                }
                if let Err(err) = wrapper.render_after_main(&mut cached_render, &virtual_dom) {
                    throw_error!(err);
                }
                cached_render.push_str(&post_streaming);

                if let Ok(mut incremental) = incremental.write() {
                    let _ = incremental.cache(route, cached_render);
                }
            }

            stream.render(post_streaming);

            renderer.reset_render_components();
            myself.renderers.write().unwrap().push(renderer);
        };

        let join_handle = spawn_platform(move || {
            ProvideServerContext::new(create_render_future(), server_context)
        });

        // Wait for the initial result which determines the status code
        initial_result_rx.await.map_err(|err| {
            SSRError::Incremental(IncrementalRendererError::Other(Box::new(err)))
        })??;

        Ok((
            RenderFreshness::now(None),
            ReceiverWithDrop {
                receiver: rx,
                cancel_task: Some(join_handle),
            },
        ))
    }
}

/// Create the streaming render component callback. It will keep track of what scopes are mounted to what pending
/// suspense boundaries in the DOM.
///
/// This mapping is used to replace the DOM mount with the resolved contents once the suspense boundary is finished.
fn streaming_render_component_callback(
    stream: Arc<StreamingRenderer<IncrementalRendererError>>,
    scope_to_mount_mapping: Arc<RwLock<HashMap<ScopeId, PendingSuspenseBoundary>>>,
) -> impl Fn(&mut Renderer, &mut dyn Write, &VirtualDom, ScopeId) -> std::fmt::Result
       + Send
       + Sync
       + 'static {
    // We use a stack to keep track of what suspense boundaries we are nested in to add children to the correct boundary
    // The stack starts with the root scope because the root is a suspense boundary
    let pending_suspense_boundaries_stack = RwLock::new(vec![]);
    move |renderer, to, vdom, scope| {
        let is_suspense_boundary =
            SuspenseContext::downcast_suspense_boundary_from_scope(&vdom.runtime(), scope)
                .filter(|s| s.has_suspended_tasks())
                .is_some();
        if is_suspense_boundary {
            let mount = stream.render_placeholder(
                |to| {
                    {
                        pending_suspense_boundaries_stack
                            .write()
                            .unwrap()
                            .push(scope);
                    }
                    let out = renderer.render_scope(to, vdom, scope);
                    {
                        pending_suspense_boundaries_stack.write().unwrap().pop();
                    }
                    out
                },
                &mut *to,
            )?;
            // Add the suspense boundary to the list of pending suspense boundaries
            // We will replace the mount with the resolved contents later once the suspense boundary is resolved
            let mut scope_to_mount_mapping_write = scope_to_mount_mapping.write().unwrap();
            scope_to_mount_mapping_write.insert(
                scope,
                PendingSuspenseBoundary {
                    mount,
                    children: vec![],
                },
            );
            // Add the scope to the list of children of the parent suspense boundary
            let pending_suspense_boundaries_stack =
                pending_suspense_boundaries_stack.read().unwrap();
            // If there is a parent suspense boundary, add the scope to the list of children
            // This suspense boundary will start capturing errors when the parent is resolved
            if let Some(parent) = pending_suspense_boundaries_stack.last() {
                let parent = scope_to_mount_mapping_write.get_mut(parent).unwrap();
                parent.children.push(scope);
            }
            // Otherwise this is a root suspense boundary, so we need to start capturing errors immediately
            else {
                vdom.in_runtime(|| {
                    start_capturing_errors(scope);
                });
            }
        } else {
            renderer.render_scope(to, vdom, scope)?
        }
        Ok(())
    }
}

/// Start capturing errors at a suspense boundary. If the parent suspense boundary is frozen, we need to capture the errors in the suspense boundary
/// and send them to the client to continue bubbling up
fn start_capturing_errors(suspense_scope: ScopeId) {
    // Add an error boundary to the scope
    suspense_scope.in_runtime(provide_error_boundary);
}

fn serialize_server_data(virtual_dom: &VirtualDom, scope: ScopeId) -> SerializedHydrationData {
    // After we replace the placeholder in the dom with javascript, we need to send down the resolved data so that the client can hydrate the node
    // Extract any data we serialized for hydration (from server futures)
    let html_data = extract_from_suspense_boundary(virtual_dom, scope);

    // serialize the server state into a base64 string
    html_data.serialized()
}

/// Walks through the suspense boundary in a depth first order and extracts the data from the context API.
/// We use depth first order instead of relying on the order the hooks are called in because during suspense on the server, the order that futures are run in may be non deterministic.
pub(crate) fn extract_from_suspense_boundary(
    vdom: &VirtualDom,
    scope: ScopeId,
) -> HydrationContext {
    let data = HydrationContext::default();
    serialize_errors(&data, vdom, scope);
    take_from_scope(&data, vdom, scope);
    data
}

/// Get the errors from the suspense boundary
fn serialize_errors(context: &HydrationContext, vdom: &VirtualDom, scope: ScopeId) {
    // If there is an error boundary on the suspense boundary, grab the error from the context API
    // and throw it on the client so that it bubbles up to the nearest error boundary
    let error = vdom.in_runtime(|| {
        scope
            .consume_context::<ErrorContext>()
            .and_then(|error_context| error_context.errors().first().cloned())
    });
    context
        .error_entry()
        .insert(&error, std::panic::Location::caller());
}

fn take_from_scope(context: &HydrationContext, vdom: &VirtualDom, scope: ScopeId) {
    vdom.in_runtime(|| {
        scope.in_runtime(|| {
            // Grab any serializable server context from this scope
            let other: Option<HydrationContext> = has_context();
            if let Some(other) = other {
                context.extend(&other);
            }
        });
    });

    // then continue to any children
    if let Some(scope) = vdom.get_scope(scope) {
        // If this is a suspense boundary, move into the children first (even if they are suspended) because that will be run first on the client
        if let Some(suspense_boundary) =
            SuspenseContext::downcast_suspense_boundary_from_scope(&vdom.runtime(), scope.id())
        {
            if let Some(node) = suspense_boundary.suspended_nodes() {
                take_from_vnode(context, vdom, &node);
            }
        }
        if let Some(node) = scope.try_root_node() {
            take_from_vnode(context, vdom, node);
        }
    }
}

fn take_from_vnode(context: &HydrationContext, vdom: &VirtualDom, vnode: &VNode) {
    for (dynamic_node_index, dyn_node) in vnode.dynamic_nodes.iter().enumerate() {
        match dyn_node {
            DynamicNode::Component(comp) => {
                if let Some(scope) = comp.mounted_scope(dynamic_node_index, vnode, vdom) {
                    take_from_scope(context, vdom, scope.id());
                }
            }
            DynamicNode::Fragment(nodes) => {
                for node in nodes {
                    take_from_vnode(context, vdom, node);
                }
            }
            _ => {}
        }
    }
}

/// State used in server side rendering. This utilizes a pool of [`dioxus_ssr::Renderer`]s to cache static templates between renders.
#[derive(Clone)]
pub struct SSRState {
    // We keep a pool of renderers to avoid re-creating them on every request. They are boxed to make them very cheap to move
    renderers: Arc<SsrRendererPool>,
}

impl SSRState {
    /// Create a new [`SSRState`].
    pub fn new(cfg: &ServeConfig) -> Self {
        Self {
            renderers: Arc::new(SsrRendererPool::new(4, cfg.incremental.clone())),
        }
    }

    /// Render the application to HTML.
    pub async fn render<'a>(
        &'a self,
        route: String,
        cfg: &'a ServeConfig,
        virtual_dom_factory: impl FnOnce() -> VirtualDom + Send + Sync + 'static,
        server_context: &'a DioxusServerContext,
    ) -> Result<
        (
            RenderFreshness,
            impl Stream<Item = Result<String, dioxus_isrg::IncrementalRendererError>>,
        ),
        SSRError,
    > {
        self.renderers
            .clone()
            .render_to(cfg, route, virtual_dom_factory, server_context)
            .await
    }
}

/// The template that wraps the body of the HTML for a fullstack page. This template contains the data needed to hydrate server functions that were run on the server.
pub struct FullstackHTMLTemplate {
    cfg: ServeConfig,
}

impl FullstackHTMLTemplate {
    /// Create a new [`FullstackHTMLTemplate`].
    pub fn new(cfg: &ServeConfig) -> Self {
        Self { cfg: cfg.clone() }
    }
}

impl FullstackHTMLTemplate {
    /// Render any content before the head of the page.
    pub fn render_head<R: std::fmt::Write>(
        &self,
        to: &mut R,
        virtual_dom: &VirtualDom,
    ) -> Result<(), dioxus_isrg::IncrementalRendererError> {
        let ServeConfig { index, .. } = &self.cfg;

        let title = {
            let document: Option<Rc<ServerDocument>> =
                virtual_dom.in_runtime(|| ScopeId::ROOT.consume_context());
            // Collect any head content from the document provider and inject that into the head
            document.and_then(|document| document.title())
        };

        to.write_str(&index.head_before_title)?;
        if let Some(title) = title {
            to.write_str(&title)?;
        } else {
            to.write_str(&index.title)?;
        }
        to.write_str(&index.head_after_title)?;

        let document: Option<Rc<ServerDocument>> =
            virtual_dom.in_runtime(|| ScopeId::ROOT.consume_context());
        if let Some(document) = document {
            // Collect any head content from the document provider and inject that into the head
            document.render(to)?;

            // Enable a warning when inserting contents into the head during streaming
            document.start_streaming();
        }

        self.render_before_body(to)?;

        Ok(())
    }

    /// Render any content before the body of the page.
    fn render_before_body<R: std::fmt::Write>(
        &self,
        to: &mut R,
    ) -> Result<(), dioxus_isrg::IncrementalRendererError> {
        let ServeConfig { index, .. } = &self.cfg;

        to.write_str(&index.close_head)?;

        // // #[cfg(feature = "document")]
        // {
        use dioxus_interpreter_js::INITIALIZE_STREAMING_JS;
        write!(to, "<script>{INITIALIZE_STREAMING_JS}</script>")?;
        // }

        Ok(())
    }

    /// Render all content after the main element of the page.
    pub fn render_after_main<R: std::fmt::Write>(
        &self,
        to: &mut R,
        virtual_dom: &VirtualDom,
    ) -> Result<(), dioxus_isrg::IncrementalRendererError> {
        let ServeConfig { index, .. } = &self.cfg;

        // Collect the initial server data from the root node. For most apps, no use_server_futures will be resolved initially, so this will be full on `None`s.
        // Sending down those Nones are still important to tell the client not to run the use_server_futures that are already running on the backend
        let resolved_data = serialize_server_data(virtual_dom, ScopeId::ROOT);
        // We always send down the data required to hydrate components on the client
        let raw_data = resolved_data.data;
        write!(
            to,
            r#"<script>window.initial_dioxus_hydration_data="{raw_data}";"#,
        )?;
        #[cfg(debug_assertions)]
        {
            // In debug mode, we also send down the type names and locations of the serialized data
            let debug_types = &resolved_data.debug_types;
            let debug_locations = &resolved_data.debug_locations;
            write!(
                to,
                r#"window.initial_dioxus_hydration_debug_types={debug_types};"#,
            )?;
            write!(
                to,
                r#"window.initial_dioxus_hydration_debug_locations={debug_locations};"#,
            )?;
        }
        write!(to, r#"</script>"#,)?;
        to.write_str(&index.post_main)?;

        Ok(())
    }

    /// Render all content after the body of the page.
    pub fn render_after_body<R: std::fmt::Write>(
        &self,
        to: &mut R,
    ) -> Result<(), dioxus_isrg::IncrementalRendererError> {
        let ServeConfig { index, .. } = &self.cfg;

        to.write_str(&index.after_closing_body_tag)?;

        Ok(())
    }
}

fn pre_renderer() -> Renderer {
    let mut renderer = Renderer::default();
    renderer.pre_render = true;
    renderer
}
