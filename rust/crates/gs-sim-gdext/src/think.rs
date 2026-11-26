//! `GSSimZombieThink`: Zombie Zone's zombie think glue (swing timer, stuck watchdog, think rate) and a
//! common's kind machine, native. Stateless: see [`gs_sim_zombie::think`] and [`gs_sim_zombie::kind`]
//! for the codes; every method answers an integer so a think allocates nothing.

use godot::prelude::*;
use gs_sim_math::Vec3 as V3;
use gs_sim_zombie::{kind, think};

fn v3(v: Vector3) -> V3 {
    V3::new(v.x, v.y, v.z)
}

/// The native cores of `ZzZombieBrain`'s think and `ZzCommonBrain._decide`.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimZombieThink {}

#[godot_api]
impl GSSimZombieThink {
    /// `_advance_swing`'s clocks: 0 none, 1 the blow lands, 2 the recovery ends, 3 both.
    #[func]
    fn swing_step(&self, swing_at: f64, landed: bool, now: f64, scale: f64) -> i64 {
        think::swing_step(swing_at, landed, now, scale)
    }

    /// `_next_swing` for a swing that ends at `now`.
    #[func]
    fn swing_next(&self, now: f64, scale: f64, interval: f64) -> f64 {
        think::swing_next(now, scale, interval)
    }

    /// `_unstick_watch` up to the hooks: 0 hold, 1 reset, 2 anchor, 3 count, 4 nudge then relocate, 5 relocate.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn unstick_step(
        &self,
        flying: bool,
        limit: f64,
        no_try: bool,
        wish_len: f64,
        here: Vector3,
        watch_at: Vector3,
        watch_for: f64,
        stage: i64,
        delta: f64,
    ) -> i64 {
        think::unstick_step(
            flying,
            limit,
            no_try,
            wish_len,
            v3(here),
            v3(watch_at),
            watch_for,
            stage,
            delta,
        )
    }

    /// `_think_every` while the state alone decides (0: the distance tiers are next).
    #[func]
    fn every_early(&self, flags: i64, now: f64, swing_at: f64, scale: f64) -> i64 {
        think::think_every_early(flags, now, swing_at, scale)
    }

    /// `_think_every`'s distance tiers.
    #[func]
    fn every_tier(&self, nearest: f64, dormant: f64, dormant_every: i64) -> i64 {
        think::think_every_tier(nearest, dormant, dormant_every)
    }

    /// The body stride and cheap step: `stride | cheap << 8`.
    #[func]
    fn body_stride(&self, nearest: f64, near: f64, far: f64, dormant: f64, cheap_from: f64) -> i64 {
        think::body_stride(nearest, near, far, dormant, cheap_from)
    }

    /// The conditions a kind-machine state reads.
    #[func]
    fn kind_needs(&self, state: i64) -> i64 {
        kind::needs(state)
    }

    /// The state to go to before the match (-1 none).
    #[func]
    fn kind_pre(&self, state: i64, conditions: i64) -> i64 {
        kind::pre(state, conditions)
    }

    /// One step: `next | action << 8`.
    #[func]
    fn kind_step(&self, state: i64, conditions: i64) -> i64 {
        kind::step(state, conditions)
    }

    /// Investigate after its search: `next | idle << 8`.
    #[func]
    fn kind_after_search(&self, running: bool) -> i64 {
        kind::after_search(running)
    }

    /// Chase after its `_chase`: `next | idle << 8`.
    #[func]
    fn kind_after_chase(&self, ok: bool, remembers: bool) -> i64 {
        kind::after_chase(ok, remembers)
    }
}
