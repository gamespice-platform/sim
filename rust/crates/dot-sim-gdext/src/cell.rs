//! `DotSimCell`: the switch of dot-sim's patched godot-cell (rust/vendor/godot-cell, README.dot-sim.md).
//!
//! With the binding's threads on, every `#[func]` call binds the instance through a blocking cell whose guards notified
//! their condvars on each drop (a futex syscall, about 2 us a call). The patch notifies only threads that wait.
//! `set_notify_always(true)` brings back upstream's notify-on-every-drop at run time: the fallback, and the A/B switch.

use std::sync::atomic::Ordering;

use godot::prelude::*;

/// Static switch for the instance-binding cell's notify mode (process-wide).
#[derive(GodotClass)]
#[class(base = Object, no_init)]
pub struct DotSimCell;

#[godot_api]
impl DotSimCell {
    /// True: every binding drop notifies as upstream godot-cell does. False (the default): only when a thread waits.
    #[func]
    fn set_notify_always(on: bool) {
        godot_cell::NOTIFY_ALWAYS.store(on, Ordering::Relaxed);
    }

    /// Whether binding drops notify always (upstream behaviour).
    #[func]
    fn notify_always() -> bool {
        godot_cell::NOTIFY_ALWAYS.load(Ordering::Relaxed)
    }
}
