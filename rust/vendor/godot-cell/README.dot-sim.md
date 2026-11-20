# godot-cell 0.5.5, patched for GameSpice Sim

Upstream: https://github.com/godot-rust/gdext (crate `godot-cell` 0.5.5, MPL-2.0; the license headers stay on every file).

One change, in `src/blocking_cell.rs` and `src/blocking_guards.rs`: `ThreadTracker` counts the threads waiting on
`mut_condition` and `immut_condition` (changed only under the tracker's lock, around each `wait`), and the guards'
drops notify a condvar only when someone waits on it. Upstream notifies on every drop, and std's futex condvar makes
each notify a `futex` syscall: with `experimental-threads` every `#[func]` call on an instance paid it (2.4 us per
no-op call on a 40-core Linux build VM against 0.19 us without the feature). Waits, wakeups and borrow rules are unchanged:
a waiter registers before it sleeps under the same lock the drop checks the count under, so no wakeup is lost.

`tests/mock` is upstream's own test suite for the cell (blocking and panicking), plus `blocking_wakeups` for the
patch: `cargo test --manifest-path rust/vendor/godot-cell/Cargo.toml`.

Drop this directory and the `[patch.crates-io]` entry in `rust/Cargo.toml` once upstream ships the same fix, or
when bumping godot-rust (re-apply the patch to the new version's cell first).
