use dioxus_core::use_hook;
use dioxus_signals::{Signal, SignalData, Storage, SyncStorage, UnsyncStorage};

/// Creates a new Signal. Signals are a Copy state management solution with automatic dependency tracking.
///
/// ```rust
/// use dioxus::prelude::*;
/// use dioxus_signals::*;
///
/// fn App() -> Element {
///     let mut count = use_signal(|| 0);
///
///     // Because signals have automatic dependency tracking, if you never read them in a component, that component will not be re-rended when the signal is updated.
///     // The app component will never be rerendered in this example.
///     rsx! { Child { state: count } }
/// }
///
/// #[component]
/// fn Child(state: Signal<u32>) -> Element {
///     use_future(move || async move {
///         // Because the signal is a Copy type, we can use it in an async block without cloning it.
///         *state.write() += 1;
///     });
///
///     rsx! {
///         button {
///             onclick: move |_| *state.write() += 1,
///             "{state}"
///         }
///     }
/// }
/// ```
///
#[doc = include_str!("../docs/rules_of_hooks.md")]
#[doc = include_str!("../docs/moving_state_around.md")]
#[doc(alias = "use_state")]
#[track_caller]
#[must_use]
pub fn use_signal<T: 'static>(f: impl FnOnce() -> T) -> Signal<T, UnsyncStorage> {
    use_maybe_signal_sync(f)
}

/// Creates a new `Send + Sync`` Signal. Signals are a Copy state management solution with automatic dependency tracking.
///
/// ```rust
/// use dioxus::prelude::*;
/// use dioxus_signals::*;
///
/// fn App() -> Element {
///     let mut count = use_signal_sync(|| 0);
///
///     // Because signals have automatic dependency tracking, if you never read them in a component, that component will not be re-rended when the signal is updated.
///     // The app component will never be rerendered in this example.
///     rsx! { Child { state: count } }
/// }
///
/// #[component]
/// fn Child(state: Signal<u32, SyncStorage>) -> Element {
///     use_future(move || async move {
///         // This signal is Send + Sync, so we can use it in an another thread
///         tokio::spawn(async move {
///             // Because the signal is a Copy type, we can use it in an async block without cloning it.
///             *state.write() += 1;
///         }).await;
///     });
///
///     rsx! {
///         button {
///             onclick: move |_| *state.write() += 1,
///             "{state}"
///         }
///     }
/// }
/// ```
#[doc(alias = "use_rw")]
#[must_use]
#[track_caller]
pub fn use_signal_sync<T: Send + Sync + 'static>(f: impl FnOnce() -> T) -> Signal<T, SyncStorage> {
    use_maybe_signal_sync(f)
}

#[must_use]
#[track_caller]
fn use_maybe_signal_sync<T: 'static, U: Storage<SignalData<T>>>(
    f: impl FnOnce() -> T,
) -> Signal<T, U> {
    let caller = std::panic::Location::caller();

    // todo: (jon)
    // By default, we want to unsubscribe the current component from the signal on every render
    // any calls to .read() in the body will re-subscribe the component to the signal
    // use_before_render(move || signal.unsubscribe(current_scope_id().unwrap()));

    use_hook(|| Signal::new_with_caller(f(), caller))
}
