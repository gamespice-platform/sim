//! Zombie Zone's zombie brains, native: the facts a common's chase reads every think, decided from
//! one per-tick survivor table instead of a handful of script calls into the horde.
//!
//! [`plan_chase`] is `ZzCommonBrain._chase_plan_script` (its `quarry`, `survivor_at`, `_chase_speed`,
//! `flat_distance` and `strikes_down`) in the script's order and precision: engine vectors `f32`,
//! script floats `f64`. The horde rebuilds its survivor facts once a tick before any brain thinks and
//! nothing writes them between, so a table pushed then answers as the dictionaries would.

use dot_sim_math::{Vec3, minf};

pub mod decide;
pub mod pace;
pub mod steer;
pub mod walk;

#[cfg(test)]
mod tests;

/// One survivor of the tick, as the horde's `_survivors[id]` facts hold it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Survivor {
    /// `facts["position"]`.
    pub at: Vec3,
    /// `facts["alive"]`: in the round and harmable, not behind a loading cover.
    pub alive: bool,
    /// `facts["down"]`.
    pub down: bool,
}

/// The chase rules of the tick: the speed cvars under the Hard and Angry Boxes, and `down_attack`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rules {
    /// `ZzNpcs.common_chase_speed(config, hard_stacks(), false)`.
    pub walk_speed: f64,
    /// `ZzNpcs.common_chase_speed(config, hard_stacks(), true)`.
    pub run_speed: f64,
    /// An Angry Box is on (`ZzHorde.is_runner` can be true).
    pub angry: bool,
    /// `ZzConfig.angry_runner_fraction`.
    pub runner_fraction: f64,
    /// `ZzConfig.down_attack`: zombies strike a downed survivor on the ground.
    pub down_attack: bool,
}

/// A survivor name a brain holds, looked up in the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    /// The empty name.
    Nobody,
    /// A name the table does not have (left, or not a survivor).
    Unknown,
    /// Row `i` of the table.
    Is(usize),
}

/// What one common brings to its chase.
#[derive(Clone, Copy, Debug)]
pub struct Chaser {
    /// `forced`: the survivor it was given.
    pub forced: Who,
    /// `npc.target_id` when `npc.has_target()`, else nobody.
    pub target: Who,
    /// Its own speed share, off its seed.
    pub pace: f64,
    /// `runner_roll`.
    pub roll: f64,
    /// `tune("reach", 1.55)`.
    pub reach: f64,
    /// Its body is burning.
    pub burning: bool,
    /// Its body is a crawler, which keeps its own swing on a downed survivor.
    pub crawler: bool,
    /// `npc.position()`.
    pub here: Vec3,
}

/// [`Plan::code`]: chasing the survivor it was given.
pub const FORCED: i32 = 0;
/// [`Plan::code`]: chasing the survivor it perceived.
pub const TARGET: i32 = 1;
/// [`Plan::code`]: nobody to chase.
pub const NOBODY: i32 = -1;
/// [`Plan::code`]: somebody, but not to be found (dead, gone, not a survivor).
pub const LOST: i32 = -2;

/// The chase's facts for one think.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// [`FORCED`], [`TARGET`], [`NOBODY`] or [`LOST`].
    pub code: i32,
    /// `forced` is dropped (it was set and that survivor is not alive).
    pub dropped: bool,
    /// Where the quarry stands; zero unless the code is [`FORCED`] or [`TARGET`].
    pub at: Vec3,
    /// `_chase_speed() * pace`, a fifth more when burning.
    pub speed: f64,
    /// `flat_distance(here, at)`.
    pub gap: f64,
    /// The reach, at most 1.1 for a ground strike.
    pub reach: f64,
}

impl Plan {
    fn none(code: i32, dropped: bool) -> Self {
        Self {
            code,
            dropped,
            at: Vec3::default(),
            speed: 0.0,
            gap: 0.0,
            reach: 0.0,
        }
    }
}

/// `ZzHorde.common_chase_speed(roll)`: a runner under an Angry Box runs, the rest walk the Hard Box ramp.
#[inline]
pub fn chase_speed(rules: &Rules, roll: f64) -> f64 {
    if rules.angry && roll < rules.runner_fraction {
        rules.run_speed
    } else {
        rules.walk_speed
    }
}

/// `ZzZombieBrain.flat_distance`: `Vector2(a.x - b.x, a.z - b.z).length()`, the differences in `f64`
/// rounded into the `f32` vector and its length taken in `f32`.
#[inline]
pub fn flat_distance(a: Vec3, b: Vec3) -> f64 {
    let x = (f64::from(a.x) - f64::from(b.x)) as f32;
    let z = (f64::from(a.z) - f64::from(b.z)) as f32;
    f64::from((x * x + z * z).sqrt())
}

/// The chase's facts for `z` against the tick's `survivors` under `rules`; see [`Plan`].
pub fn plan_chase(survivors: &[Survivor], rules: &Rules, z: &Chaser) -> Plan {
    let row = |w: Who| match w {
        Who::Is(i) => survivors.get(i).copied(),
        _ => None,
    };
    let alive = |w: Who| row(w).is_some_and(|s| s.alive);

    // `quarry()`: the forced survivor while alive; else forced is dropped and the perceived one taken.
    let (code, quarry, dropped) = if z.forced != Who::Nobody && alive(z.forced) {
        (FORCED, z.forced, false)
    } else if z.target == Who::Nobody {
        return Plan::none(NOBODY, z.forced != Who::Nobody);
    } else {
        (TARGET, z.target, z.forced != Who::Nobody)
    };

    // `survivor_at()`: INF for a dead or unknown one, which ends the chase.
    let Some(s) = row(quarry).filter(|s| s.alive) else {
        return Plan::none(LOST, dropped);
    };

    let mut speed = chase_speed(rules, z.roll) * z.pace;
    if z.burning {
        speed *= 1.2;
    }

    let mut reach = z.reach;
    if !z.crawler && s.down && rules.down_attack {
        reach = minf(reach, 1.1);
    }

    Plan {
        code,
        dropped,
        at: s.at,
        speed,
        gap: flat_distance(z.here, s.at),
        reach,
    }
}
