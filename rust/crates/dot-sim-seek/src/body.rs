//! Hide and Seek's bodies in sight: game-hide-n-seek's `HsBots._sees`, `_nearest_seen_seeker`
//! and `_visible_prey` over one table of the tick's bodies, to the bit and in the script's order.
//!
//! The script asks, for every bot, every body on the other side, with a ray built in script for
//! each line of sight; here the bodies are written once a tick and each ask is one call. The
//! answer is the script's; only the rays a result does not depend on are skipped (the second of
//! the head and chest rays when the first has settled it), and a line of sight has no side effect.
//!
//! Precision follows the script: engine vectors are `f32` ([`Vec3`]), script floats `f64`; a
//! distance or angle is taken in `f32` and compared in `f64`.

use dot_sim_math::{Vec3, rad_to_deg};

/// The game's rules for a body in sight.
#[derive(Clone, Copy, Debug)]
pub struct BodyRules {
    /// `FELT_WITHIN`: within this (metres) of the chest a body is felt whatever way one faces.
    pub felt_within: f64,
    /// `STILL_LOW_SIGHT`: the share of sight a still, crouched body is seen at.
    pub still_low_sight: f64,
    /// The chest's height over the feet, as the script's `Vector3(0, 0.6, 0)`.
    pub chest: f64,
}

impl Default for BodyRules {
    fn default() -> Self {
        BodyRules {
            felt_within: 2.0,
            still_low_sight: 0.10,
            chest: 0.6,
        }
    }
}

/// One player as the tick has it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    /// `controller.state.position`.
    pub at: Vec3,
    /// `eye_position()` (also the head another sees).
    pub eye: Vec3,
    /// `DotBotAim.direction(yaw, pitch)`.
    pub forward: Vec3,
    /// `controller.state.velocity`.
    pub velocity: Vec3,
    /// `controller.state.crouch_fraction` (a script float).
    pub crouch: f64,
    pub alive: bool,
    pub watching: bool,
    pub caught: bool,
    pub marked: bool,
    /// The side (`team_of`).
    pub side: i32,
}

/// `DotBotHiding.sees(eye, forward, fov, sight, target, los)`.
#[inline]
pub fn sees(
    eye: Vec3,
    forward: Vec3,
    fov: f64,
    sight: f64,
    target: Vec3,
    los: &mut dyn FnMut(Vec3, Vec3) -> bool,
) -> bool {
    let to = target - eye;
    let d = to.length();
    let df = f64::from(d);
    if df > sight {
        return false;
    }
    if df > 0.01 && rad_to_deg(f64::from(forward.angle_to(to.div_scalar(df)))) > fov * 0.5 {
        return false;
    }
    los(eye, target)
}

/// `HsBots._sees(watcher, other)` with the profile's `fov` and `sight`.
pub fn body_sees(
    rules: &BodyRules,
    fov: f64,
    sight: f64,
    watcher: &Body,
    other: &Body,
    los: &mut dyn FnMut(Vec3, Vec3) -> bool,
) -> bool {
    let mut sight = sight;
    let v = other.velocity;
    let crouched = other.crouch > 0.5;
    let flat = f64::from((v.x * v.x + v.z * v.z).sqrt());
    let still_low = flat < 0.5 && crouched;
    if flat < 0.5 {
        sight *= if crouched { rules.still_low_sight } else { 0.5 };
    } else if crouched {
        sight *= 0.5;
    }
    let eye = watcher.eye;
    let forward = watcher.forward;
    let head = other.eye;
    let chest = other.at + Vec3::new(0.0, rules.chest as f32, 0.0);
    if f64::from(eye.distance_to(chest)) < rules.felt_within && los(eye, chest) {
        return true;
    }
    // The script asks both; the second is skipped when the first has settled the answer.
    let head_seen = sees(eye, forward, fov, sight, head, los);
    if still_low {
        head_seen && sees(eye, forward, fov, sight, chest, los)
    } else {
        head_seen || sees(eye, forward, fov, sight, chest, los)
    }
}

/// What a nearest ask looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// `_nearest_seen_seeker`: living, not watching, nearer than `within`, and seen.
    Seekers,
    /// `_visible_prey`: living, not watching, not caught, and marked or seen.
    Prey,
}

/// The nearest body on `side` from `at` that `watcher` has in sight as `look` asks, the first of
/// equals, in table order; `-1` for none.
#[allow(clippy::too_many_arguments)]
pub fn nearest_seen(
    bodies: &[Body],
    rules: &BodyRules,
    fov: f64,
    sight: f64,
    watcher: usize,
    at: Vec3,
    side: i32,
    within: f64,
    look: Look,
    los: &mut dyn FnMut(Vec3, Vec3) -> bool,
) -> i32 {
    let Some(w) = bodies.get(watcher) else {
        return -1;
    };
    let mut best = -1;
    let mut best_d = f64::INFINITY;
    for (i, b) in bodies.iter().enumerate() {
        if b.side != side || !b.alive || b.watching {
            continue;
        }
        if look == Look::Prey && b.caught {
            continue;
        }
        let d = f64::from(at.distance_to(b.at));
        // The script's `d < nearest_d`: a NaN distance is passed over.
        if d.is_nan() || d >= best_d {
            continue;
        }
        let seen = match look {
            Look::Seekers => d < within && body_sees(rules, fov, sight, w, b, los),
            Look::Prey => b.marked || body_sees(rules, fov, sight, w, b, los),
        };
        if seen {
            best = i as i32;
            best_d = d;
        }
    }
    best
}

#[cfg(test)]
mod tests;
