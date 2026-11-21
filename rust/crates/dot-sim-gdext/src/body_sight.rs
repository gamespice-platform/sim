//! `GSSimBodySight`: game-hide-n-seek's bodies in sight (`HsBots._sees`, `_nearest_seen_seeker`,
//! `_visible_prey`), the tick's bodies written once and each ask one call.
//!
//! The game binds a [`GSSimSightRay`] to its world (`set_sight`), sets its rules (`set_rules`),
//! writes the bodies as the tick has them before the bots are driven (`set_bodies`), then asks
//! `nearest_seen` and `sees` by body index (`dot_sim_seek::body`; the game's
//! `headless_native_sight` and its `HS_SIGHT_VERIFY` hold the two together). Every line of sight
//! is the script's `_clear_between` ray, asked natively.

use crate::sight::{GSSimSightRay, SightQuery};
use dot_sim_math::Vec3;
use dot_sim_seek::body::{Body, BodyRules, Look, body_sees, nearest_seen};
use godot::prelude::*;

/// Body flag bits in `set_bodies`.
const ALIVE: i32 = 1;
const WATCHING: i32 = 2;
const CAUGHT: i32 = 4;
const MARKED: i32 = 8;

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBodySight {
    bodies: Vec<Body>,
    rules: BodyRules,
    sight: Option<Gd<GSSimSightRay>>,
    query: SightQuery,
    /// Rays asked since the last `rays` read.
    rays: i64,
}

#[godot_api]
impl GSSimBodySight {
    /// The ray every line of sight is asked with (none: every line is clear).
    #[func]
    fn set_sight(&mut self, ray: Option<Gd<GSSimSightRay>>) {
        self.sight = ray;
        self.query = SightQuery::default();
    }

    /// `FELT_WITHIN`, `STILL_LOW_SIGHT` and the chest's height over the feet.
    #[func]
    fn set_rules(&mut self, felt_within: f64, still_low_sight: f64, chest: f64) {
        self.rules = BodyRules {
            felt_within,
            still_low_sight,
            chest,
        };
    }

    /// The tick's bodies, one entry per player in the game's order: feet, eyes, aim, velocity,
    /// crouch fraction, flags (alive 1, watching 2, caught 4, marked 8) and side. Returns
    /// whether the arrays agreed (none are kept when they do not).
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn set_bodies(
        &mut self,
        at: PackedVector3Array,
        eyes: PackedVector3Array,
        forwards: PackedVector3Array,
        velocities: PackedVector3Array,
        crouch: PackedFloat64Array,
        flags: PackedInt32Array,
        sides: PackedInt32Array,
    ) -> bool {
        let (a, e, f, v, c, fl, s) = (
            at.as_slice(),
            eyes.as_slice(),
            forwards.as_slice(),
            velocities.as_slice(),
            crouch.as_slice(),
            flags.as_slice(),
            sides.as_slice(),
        );
        let n = a.len();
        if [e.len(), f.len(), v.len(), c.len(), fl.len(), s.len()]
            .iter()
            .any(|&l| l != n)
        {
            godot_error!(
                "GSSimBodySight.set_bodies: {} feet, {} eyes, {} aims, {} velocities, {} crouches, {} flags, {} sides",
                n,
                e.len(),
                f.len(),
                v.len(),
                c.len(),
                fl.len(),
                s.len()
            );
            self.bodies.clear();
            return false;
        }
        self.bodies.clear();
        self.bodies.extend((0..n).map(|i| Body {
            at: v3(a[i]),
            eye: v3(e[i]),
            forward: v3(f[i]),
            velocity: v3(v[i]),
            crouch: c[i],
            alive: fl[i] & ALIVE != 0,
            watching: fl[i] & WATCHING != 0,
            caught: fl[i] & CAUGHT != 0,
            marked: fl[i] & MARKED != 0,
            side: s[i],
        }));
        true
    }

    /// The bodies held.
    #[func]
    fn body_count(&self) -> i64 {
        self.bodies.len() as i64
    }

    /// The nearest body on `side` from `at` that body `watcher` has in sight, or -1. `prey`
    /// false is `_nearest_seen_seeker` (nearer than `within` as well), true `_visible_prey`
    /// (a marked body is known; caught bodies are not).
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn nearest_seen(
        &mut self,
        watcher: i64,
        at: Vector3,
        side: i64,
        fov: f64,
        sight: f64,
        within: f64,
        prey: bool,
    ) -> i64 {
        let Ok(w) = usize::try_from(watcher) else {
            return -1;
        };
        let look = if prey { Look::Prey } else { Look::Seekers };
        let conf = self.sight.as_ref().map(|s| s.bind().conf());
        let query = &mut self.query;
        let rays = &mut self.rays;
        let mut los = |a: Vec3, b: Vec3| -> bool {
            *rays += 1;
            match &conf {
                Some(c) => c.clear_to(query, gv(a), gv(b)),
                None => true,
            }
        };
        i64::from(nearest_seen(
            &self.bodies,
            &self.rules,
            fov,
            sight,
            w,
            v3(at),
            side as i32,
            within,
            look,
            &mut los,
        ))
    }

    /// `_sees(watcher, other)` by body index; false for an index outside the table.
    #[func]
    fn sees(&mut self, watcher: i64, other: i64, fov: f64, sight: f64) -> bool {
        let get = |i: i64| usize::try_from(i).ok().and_then(|i| self.bodies.get(i));
        let (Some(w), Some(o)) = (get(watcher), get(other)) else {
            return false;
        };
        let conf = self.sight.as_ref().map(|s| s.bind().conf());
        let query = &mut self.query;
        let rays = &mut self.rays;
        let mut los = |a: Vec3, b: Vec3| -> bool {
            *rays += 1;
            match &conf {
                Some(c) => c.clear_to(query, gv(a), gv(b)),
                None => true,
            }
        };
        body_sees(&self.rules, fov, sight, w, o, &mut los)
    }

    /// Rays asked since the last read.
    #[func]
    fn rays(&mut self) -> i64 {
        std::mem::take(&mut self.rays)
    }
}

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn gv(v: Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}
