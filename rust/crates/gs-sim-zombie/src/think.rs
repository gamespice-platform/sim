//! The glue a Zombie Zone zombie runs every think, as pure cores: the swing timer (`_advance_swing`),
//! the stuck watchdog (`_unstick_watch`) and how often it thinks (`_think_every`, the body stride).
//!
//! Each function is the script's arithmetic in the script's order and precision (engine vectors `f32`,
//! script floats `f64`) and answers a small integer code. The script keeps what only the engine can do
//! (the hit resolution `horde.melee`, `ZzUnstick.nudge` and `relocate`, the nearest-survivor scan) as hooks
//! keyed by the code, and writes the few state fields the code names, so no state lives here and a row
//! can be thought by any brain in any order.

use crate::flat_distance;
use gs_sim_math::{Vec3, maxf};

/// `ZzZombieBrain.SWING_WINDUP`.
pub const SWING_WINDUP: f64 = 0.32;
/// `ZzZombieBrain.SWING_RECOVER`.
pub const SWING_RECOVER: f64 = 0.55;
/// `ZzZombieBrain.WATCH_RADIUS`.
pub const WATCH_RADIUS: f64 = 1.0;

/// [`swing_step`]: nothing to do.
pub const SWING_NONE: i64 = 0;
/// [`swing_step`]: the blow lands now: the script marks it landed and resolves the hit.
pub const SWING_LAND: i64 = 1;
/// [`swing_step`]: the recovery is over: `_swing_at = INF` and `_next_swing = swing_next(..)`.
pub const SWING_END: i64 = 2;

/// `ZzZombieBrain._advance_swing`'s two clocks for a swing started at `swing_at` (`INF`: none), as of
/// `now` with `scale` its `_attack_scale()`: [`SWING_LAND`] once the wind-up has run (and `landed` is
/// still false), plus [`SWING_END`] once the recovery has too. Both can come in one call.
pub fn swing_step(swing_at: f64, landed: bool, now: f64, scale: f64) -> i64 {
    if swing_at == f64::INFINITY {
        return SWING_NONE;
    }
    let t = now - swing_at;
    let mut code = SWING_NONE;
    if !landed && t >= SWING_WINDUP * scale {
        code |= SWING_LAND;
    }
    if t >= (SWING_WINDUP + SWING_RECOVER) * scale {
        code |= SWING_END;
    }
    code
}

/// `_next_swing` after a swing ends at `now`: the rest of `interval` (`tune("attack_interval", 0.9)`)
/// past the blow and its recovery, scaled.
pub fn swing_next(now: f64, scale: f64, interval: f64) -> f64 {
    now + maxf(interval - SWING_WINDUP - SWING_RECOVER, 0.0) * scale
}

/// [`unstick_step`]: the clock is paused (in the air); nothing changes.
pub const WATCH_HOLD: i64 = 0;
/// [`unstick_step`]: not trying to get anywhere: `_watch_at = INF`, `_watch_for = 0`.
pub const WATCH_RESET: i64 = 1;
/// [`unstick_step`]: moved on (or first look): `_watch_at = here`, `_watch_for = 0`, `_watch_stage = 0`.
pub const WATCH_ANCHOR: i64 = 2;
/// [`unstick_step`]: still within the limit: `_watch_for += delta`.
pub const WATCH_COUNT: i64 = 3;
/// [`unstick_step`]: stuck at stage 0: `_watch_for = 0`, try `ZzUnstick.nudge`, then the relocates.
pub const WATCH_NUDGE: i64 = 4;
/// [`unstick_step`]: stuck at a later stage: `_watch_for = 0`, the relocates.
pub const WATCH_RELOCATE: i64 = 5;

/// `ZzZombieBrain._unstick_watch` up to the hooks. `flying`: the body is in the air; `limit`:
/// `config.unstick_seconds` (0 without a config); `no_try`: no body, or bashing, or swinging; `wish_len`:
/// `b.wish.length()`; `here` and `watch_at` (all-`INF` for none); `watch_for`, `stage`, `delta`.
#[allow(clippy::too_many_arguments)]
pub fn unstick_step(
    flying: bool,
    limit: f64,
    no_try: bool,
    wish_len: f64,
    here: Vec3,
    watch_at: Vec3,
    watch_for: f64,
    stage: i64,
    delta: f64,
) -> i64 {
    if flying {
        return WATCH_HOLD;
    }
    if limit <= 0.0 || no_try || wish_len < 1.0 {
        return WATCH_RESET;
    }
    let unset =
        watch_at.x == f32::INFINITY && watch_at.y == f32::INFINITY && watch_at.z == f32::INFINITY;
    if unset
        || flat_distance(here, watch_at) > WATCH_RADIUS
        || f64::from((here.y - watch_at.y).abs()) > WATCH_RADIUS
    {
        return WATCH_ANCHOR;
    }
    if watch_for + delta < limit {
        return WATCH_COUNT;
    }
    if stage == 0 {
        WATCH_NUDGE
    } else {
        WATCH_RELOCATE
    }
}

/// [`think_every_early`] flag: the NPC is gone or dead, or staggered.
pub const EVERY_OFF: i64 = 1;
/// [`think_every_early`] flag: a swing is in progress.
pub const EVERY_SWINGING: i64 = 2;
/// [`think_every_early`] flag: the blow of that swing has landed.
pub const EVERY_LANDED: i64 = 4;
/// [`think_every_early`] flag: tearing at a barricade.
pub const EVERY_BASHING: i64 = 8;
/// [`think_every_early`] flag: no horde to ask.
pub const EVERY_NO_HORDE: i64 = 16;

/// `ZzZombieBrain._think_every` before it needs the nearest survivor: the ticks one thought spans when
/// the state alone settles it, or 0 when the distance tiers ([`think_every_tier`]) are next.
pub fn think_every_early(flags: i64, now: f64, swing_at: f64, scale: f64) -> i64 {
    if flags & EVERY_OFF != 0 {
        return 1;
    }
    if flags & EVERY_SWINGING != 0 {
        // Once the blow has landed only the recovery is left: half as often until its last ticks.
        if flags & EVERY_LANDED != 0
            && (SWING_WINDUP + SWING_RECOVER) * scale - (now - swing_at) > 0.05
        {
            return 2;
        }
        return 1;
    }
    if flags & EVERY_BASHING != 0 {
        return 4;
    }
    if flags & EVERY_NO_HORDE != 0 {
        return 1;
    }
    0
}

/// `_think_every`'s tiers by `nearest` (already divided by the think range): 1, 2, 3, 4, or the
/// dormant 8 (`dormant_every`, at least 8) from `dormant` on (0 turns dormancy off).
pub fn think_every_tier(nearest: f64, dormant: f64, dormant_every: i64) -> i64 {
    if nearest < 6.0 {
        return 1;
    }
    if nearest < 20.0 {
        return 2;
    }
    if nearest < 45.0 {
        return 3;
    }
    if nearest < 60.0 {
        return 4;
    }
    if dormant > 0.0 && nearest >= dormant {
        return dormant_every.max(8);
    }
    8
}

/// The body's stride and cheap step the nearest survivor sets, packed as `stride | cheap << 8`:
/// `near` and `far` are `config.body_stride_near` and `_far`, `dormant` `config.dormant_from`,
/// `cheap_from` `config.body_cheap_from`.
pub fn body_stride(nearest: f64, near: f64, far: f64, dormant: f64, cheap_from: f64) -> i64 {
    let far_stride = if dormant > 0.0 && nearest >= dormant {
        8
    } else {
        4
    };
    let stride = if nearest < near {
        1
    } else if nearest < far {
        2
    } else {
        far_stride
    };
    let cheap = cheap_from > 0.0 && nearest >= cheap_from;
    stride | (i64::from(cheap) << 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INF: f64 = f64::INFINITY;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn unset() -> Vec3 {
        v(f32::INFINITY, f32::INFINITY, f32::INFINITY)
    }

    #[test]
    fn swing_lands_then_ends() {
        assert_eq!(swing_step(INF, false, 5.0, 1.0), SWING_NONE);
        assert_eq!(swing_step(1.0, false, 1.1, 1.0), SWING_NONE);
        assert_eq!(swing_step(1.0, false, 1.32, 1.0), SWING_LAND);
        assert_eq!(swing_step(1.0, true, 1.5, 1.0), SWING_NONE);
        assert_eq!(swing_step(1.0, true, 1.87, 1.0), SWING_END);
        assert_eq!(swing_step(1.0, false, 3.0, 1.0), SWING_LAND | SWING_END);
        // Burning swings faster.
        assert_eq!(swing_step(1.0, false, 1.26, 0.8), SWING_LAND);
        assert_eq!(swing_step(1.0, false, 1.7, 0.8), SWING_LAND | SWING_END);
    }

    #[test]
    fn swing_next_waits_out_the_interval() {
        assert_eq!(swing_next(2.0, 1.0, 0.9), 2.0 + (0.9 - 0.32 - 0.55));
        assert_eq!(swing_next(2.0, 0.8, 0.5), 2.0);
    }

    #[test]
    fn watch_counts_then_tries_nudge_then_relocate() {
        let here = v(1.0, 0.0, 1.0);
        let step = |at, wf, stage| unstick_step(false, 2.0, false, 3.0, here, at, wf, stage, 0.5);
        assert_eq!(
            unstick_step(true, 2.0, false, 3.0, here, unset(), 0.0, 0, 0.5),
            WATCH_HOLD
        );
        assert_eq!(
            unstick_step(false, 0.0, false, 3.0, here, unset(), 0.0, 0, 0.5),
            WATCH_RESET
        );
        assert_eq!(
            unstick_step(false, 2.0, true, 3.0, here, unset(), 0.0, 0, 0.5),
            WATCH_RESET
        );
        assert_eq!(
            unstick_step(false, 2.0, false, 0.9, here, unset(), 0.0, 0, 0.5),
            WATCH_RESET
        );
        assert_eq!(step(unset(), 0.0, 0), WATCH_ANCHOR);
        assert_eq!(step(v(5.0, 0.0, 1.0), 0.0, 0), WATCH_ANCHOR);
        assert_eq!(step(v(1.0, 2.0, 1.0), 0.0, 0), WATCH_ANCHOR);
        assert_eq!(step(here, 1.0, 0), WATCH_COUNT);
        assert_eq!(step(here, 1.5, 0), WATCH_NUDGE);
        assert_eq!(step(here, 1.5, 1), WATCH_RELOCATE);
    }

    #[test]
    fn every_tiers() {
        let early = |f| think_every_early(f, 10.0, 9.0, 1.0);
        assert_eq!(early(EVERY_OFF | EVERY_SWINGING), 1);
        assert_eq!(early(EVERY_SWINGING), 1);
        assert_eq!(
            think_every_early(EVERY_SWINGING | EVERY_LANDED, 10.0, 9.7, 1.0),
            2
        );
        assert_eq!(
            think_every_early(EVERY_SWINGING | EVERY_LANDED, 10.6, 9.7, 1.0),
            1
        );
        assert_eq!(early(EVERY_BASHING), 4);
        assert_eq!(early(EVERY_NO_HORDE), 1);
        assert_eq!(early(0), 0);
        let tier = |n| think_every_tier(n, 90.0, 12);
        assert_eq!(
            [
                tier(5.9),
                tier(6.0),
                tier(20.0),
                tier(45.0),
                tier(60.0),
                tier(90.0)
            ],
            [1, 2, 3, 4, 8, 12]
        );
        assert_eq!(think_every_tier(500.0, 0.0, 12), 8);
        assert_eq!(think_every_tier(500.0, 90.0, 3), 8);
    }

    #[test]
    fn stride_and_cheap() {
        assert_eq!(body_stride(10.0, 20.0, 45.0, 0.0, 0.0), 1);
        assert_eq!(body_stride(30.0, 20.0, 45.0, 0.0, 0.0), 2);
        assert_eq!(body_stride(50.0, 20.0, 45.0, 0.0, 0.0), 4);
        assert_eq!(body_stride(95.0, 20.0, 45.0, 90.0, 80.0), 8 | 1 << 8);
        assert_eq!(body_stride(50.0, 20.0, 45.0, 90.0, 80.0), 4);
    }
}
