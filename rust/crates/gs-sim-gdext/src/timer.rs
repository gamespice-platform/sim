//! `GSSimTimer`: a precise sleep for a loop that paces itself, where `OS.delay_usec` is rounded up to the OS timer tick
//! (Windows' 15.6 ms). Windows' high-resolution waitable timer, a futex wait elsewhere; and the Windows timer resolution
//! held while the object lives. No script original: the caller's fallback is `OS.delay_usec` and a spin.

use godot::prelude::*;
use gs_sim_timer::{Resolution, Sleeper};

/// One pacing thread's sleeper; drop it (or call [method release_resolution]) to give the timer resolution back.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimTimer {
    sleeper: Sleeper,
    resolution: Option<Resolution>,
}

#[godot_api]
impl GSSimTimer {
    /// Blocks for at least [param usec] microseconds, on time whatever the OS timer tick.
    #[func]
    fn sleep_usec(&self, usec: i64) {
        self.sleeper.sleep_usec(usec.max(0) as u64);
    }

    /// How [method sleep_usec] waits: "high-res waitable timer", "Sleep (global timer tick)" or "futex wait".
    #[func]
    fn sleep_kind(&self) -> GString {
        self.sleeper.kind().name().into()
    }

    /// True when [method sleep_usec] wakes on time without a raised resolution (every system but a Windows older than 10 1803).
    #[func]
    fn is_precise(&self) -> bool {
        self.sleeper.kind() != gs_sim_timer::Kind::Sleep
    }

    /// Raises the Windows timer resolution to [param ms] (timeBeginPeriod, kept even while hidden or throttled) until
    /// [method release_resolution] or this object is freed. False elsewhere, where there is nothing to raise.
    #[func]
    fn request_resolution_ms(&mut self, ms: i64) -> bool {
        self.resolution = None;
        self.resolution = gs_sim_timer::request_resolution(ms.clamp(1, 1000) as u32);
        self.resolution.is_some()
    }

    #[func]
    fn release_resolution(&mut self) {
        self.resolution = None;
    }

    /// The resolution held in ms, 0 when none.
    #[func]
    fn resolution_ms(&self) -> i64 {
        self.resolution.as_ref().map_or(0, |r| i64::from(r.ms()))
    }
}
