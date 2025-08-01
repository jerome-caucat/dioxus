use dioxus_core::{use_after_render, use_before_render, use_hook};
use dioxus_signals::{CopyValue, Writable};

/// A hook that uses before/after lifecycle hooks to determine if the hook was run
#[doc = include_str!("../docs/rules_of_hooks.md")]
#[doc = include_str!("../docs/moving_state_around.md")]
pub fn use_hook_did_run(mut handler: impl FnMut(bool) + 'static) {
    let mut did_run_ = use_hook(|| CopyValue::new(false));

    // Before render always set the value to false
    use_before_render(move || did_run_.set(false));

    // Only when this hook is hit do we want to set the value to true
    did_run_.set(true);

    // After render, we can check if the hook was run
    use_after_render(move || handler(did_run_()));
}
