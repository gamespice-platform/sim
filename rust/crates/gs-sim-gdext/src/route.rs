//! `GSSimRouteBots`: Airdrift's route bots (`AirdriftRouteBot.command`), every bot of a tick in
//! one call.
//!
//! The game writes each map's route once (`add_route`), gives each bot a slot on it (`add`), and
//! once a tick hands over the bots it drives: their slots, positions, velocities and grounded
//! flags, the world's space and the shared tunables. The answer is one command per bot, the
//! script's to the bit (`gs_sim_route`), and the rays are the script's
//! `intersect_ray(PhysicsRayQueryParameters3D.create(from, to))` on one reused query (the
//! defaults: every layer, bodies, no areas), cast bot after bot in the script's order. The game's
//! `headless_native_route` and its `route_verify` hold the two together.

use godot::classes::{PhysicsDirectSpaceState3D, PhysicsRayQueryParameters3D};
use godot::prelude::*;
use gs_sim_math::Vec3;
use gs_sim_route::{Bot, Hit, Point, Route, STATE_LEN, Space, Tunables};

/// Flag bits of `add_route`'s `flags`, one int per point.
const WALK: i32 = 1;
const HOP: i32 = 2;
const LAND: i32 = 4;
const DOOR: i32 = 8;
const LAUNCH: i32 = 16;
const SLIDE: i32 = 32;

/// Values per bot in `drive`'s answer: yaw, move x, move y, buttons.
const OUT: usize = 4;

#[inline]
fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[inline]
fn gv(v: Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// The engine's space behind [`Space`], on one reused query.
struct Engine<'a> {
    space: Gd<PhysicsDirectSpaceState3D>,
    ray: &'a mut Gd<PhysicsRayQueryParameters3D>,
}

impl Space for Engine<'_> {
    fn ray(&mut self, from: Vec3, to: Vec3) -> Option<Hit> {
        self.ray.set_from(gv(from));
        self.ray.set_to(gv(to));
        let hit = self.space.intersect_ray(&*self.ray);
        if hit.is_empty() {
            return None;
        }
        let get = |key: &str| {
            hit.get(key)
                .and_then(|v| v.try_to::<Vector3>().ok())
                .map_or(Vec3::default(), v3)
        };
        Some(Hit {
            position: get("position"),
            normal: get("normal"),
        })
    }
}

/// Every route bot of a map: its routes, a slot per bot, and the reused ray query.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimRouteBots {
    routes: Vec<Route>,
    /// Per slot: the route it drives and its state; `None` once removed.
    bots: Vec<Option<(usize, Bot)>>,
    free: Vec<usize>,
    ray: Option<Gd<PhysicsRayQueryParameters3D>>,
}

impl GSSimRouteBots {
    fn slot(&self, slot: i32, what: &str) -> Option<usize> {
        let s = usize::try_from(slot).ok()?;
        match self.bots.get(s) {
            Some(Some(_)) => Some(s),
            _ => {
                godot_error!("GSSimRouteBots.{what}: no bot in slot {slot}");
                None
            }
        }
    }
}

#[godot_api]
impl GSSimRouteBots {
    /// Holds a route: per point its position (metres), radius (metres), speed cap (u/s, 0 for
    /// none) and flags (1 walk, 2 hop, 4 land, 8 door, 16 launch, 32 slide); then the finish's
    /// centre. Returns the route's id, or -1 when the counts differ (an error).
    #[func]
    fn add_route(
        &mut self,
        at: PackedVector3Array,
        radius: PackedFloat64Array,
        max: PackedFloat64Array,
        flags: PackedInt32Array,
        finish: Vector3,
    ) -> i64 {
        let n = at.len();
        if radius.len() != n || max.len() != n || flags.len() != n {
            godot_error!(
                "GSSimRouteBots.add_route: {} points, {} radii, {} caps, {} flags",
                n,
                radius.len(),
                max.len(),
                flags.len()
            );
            return -1;
        }
        let (at, radius, max, flags) = (
            at.as_slice(),
            radius.as_slice(),
            max.as_slice(),
            flags.as_slice(),
        );
        let points = (0..n)
            .map(|i| {
                let f = flags[i];
                Point {
                    at: v3(at[i]),
                    radius: radius[i],
                    max: max[i],
                    walk: f & WALK != 0,
                    hop: f & HOP != 0,
                    land: f & LAND != 0,
                    door: f & DOOR != 0,
                    launch: f & LAUNCH != 0,
                    slide: f & SLIDE != 0,
                }
            })
            .collect();
        self.routes.push(Route {
            points,
            finish: v3(finish),
        });
        (self.routes.len() - 1) as i64
    }

    /// A new bot at the start of [param route]; its slot, or -1 for no such route (an error).
    #[func]
    fn add(&mut self, route: i64) -> i64 {
        if usize::try_from(route)
            .ok()
            .is_none_or(|r| r >= self.routes.len())
        {
            godot_error!("GSSimRouteBots.add: no route {route}");
            return -1;
        }
        let bot = Some((route as usize, Bot::default()));
        if let Some(s) = self.free.pop() {
            self.bots[s] = bot;
            return s as i64;
        }
        self.bots.push(bot);
        (self.bots.len() - 1) as i64
    }

    /// Lets [param slot] go; it is reused by a later `add`.
    #[func]
    fn remove(&mut self, slot: i32) {
        if let Some(s) = self.slot(slot, "remove") {
            self.bots[s] = None;
            self.free.push(s);
        }
    }

    /// Bots held.
    #[func]
    fn count(&self) -> i64 {
        (self.bots.len() - self.free.len()) as i64
    }

    /// `restart()` for [param slot].
    #[func]
    fn restart(&mut self, slot: i32) {
        if let Some(s) = self.slot(slot, "restart")
            && let Some((_, bot)) = &mut self.bots[s]
        {
            bot.restart();
        }
    }

    /// `teleported()` for [param slot].
    #[func]
    fn teleported(&mut self, slot: i32) {
        if let Some(s) = self.slot(slot, "teleported")
            && let Some((r, bot)) = &mut self.bots[s]
        {
            bot.teleported(&self.routes[*r]);
        }
    }

    /// `target()` for [param slot]: the point aimed at now, in metres.
    #[func]
    fn target(&self, slot: i32) -> Vector3 {
        match self
            .slot(slot, "target")
            .and_then(|s| self.bots[s].as_ref())
        {
            Some((r, bot)) => gv(bot.target(&self.routes[*r])),
            None => Vector3::ZERO,
        }
    }

    /// [param slot]'s state: index, surfed, side, leg_from xyz, unstuck, anchor xyz, held,
    /// unstick_left, unstick_turn (empty for no such slot).
    #[func]
    fn get_state(&self, slot: i32) -> PackedFloat64Array {
        match self
            .slot(slot, "get_state")
            .and_then(|s| self.bots[s].as_ref())
        {
            Some((_, bot)) => PackedFloat64Array::from(&bot.to_state()[..]),
            None => PackedFloat64Array::new(),
        }
    }

    /// Writes [param slot]'s state in `get_state`'s layout.
    #[func]
    fn set_state(&mut self, slot: i32, state: PackedFloat64Array) {
        let Ok(s) = <&[f64; STATE_LEN]>::try_from(state.as_slice()) else {
            godot_error!(
                "GSSimRouteBots.set_state: {} values, want {}",
                state.len(),
                STATE_LEN
            );
            return;
        };
        if let Some(i) = self.slot(slot, "set_state")
            && let Some((_, bot)) = &mut self.bots[i]
        {
            *bot = Bot::from_state(s);
        }
    }

    /// One tick of every bot in [param slots], in order: `command(state, space, tunables, delta)`
    /// with each bot's position, velocity and grounded flag; [param tunables] is max_slope_angle,
    /// gravity, max_speed, air_accelerate, max_air_wish_speed. Four values per bot: yaw, move x,
    /// move y, buttons. Counts that differ, or a slot with no bot, are an error and answer empty.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn drive(
        &mut self,
        slots: PackedInt32Array,
        positions: PackedVector3Array,
        velocities: PackedVector3Array,
        grounded: PackedByteArray,
        space: Gd<PhysicsDirectSpaceState3D>,
        tunables: PackedFloat64Array,
        delta: f64,
    ) -> PackedFloat64Array {
        let n = slots.len();
        if positions.len() != n
            || velocities.len() != n
            || grounded.len() != n
            || tunables.len() != 5
        {
            godot_error!(
                "GSSimRouteBots.drive: {} slots, {} positions, {} velocities, {} grounded, {} tunables (want 5)",
                n,
                positions.len(),
                velocities.len(),
                grounded.len(),
                tunables.len()
            );
            return PackedFloat64Array::new();
        }
        let slots = slots.as_slice();
        if slots.iter().any(|&s| self.slot(s, "drive").is_none()) {
            return PackedFloat64Array::new();
        }
        let t = tunables.as_slice();
        let tun = Tunables {
            max_slope_angle: t[0],
            gravity: t[1],
            max_speed: t[2],
            air_accelerate: t[3],
            max_air_wish_speed: t[4],
        };
        let ray = self
            .ray
            .get_or_insert_with(PhysicsRayQueryParameters3D::new_gd);
        let mut engine = Engine { space, ray };
        let (positions, velocities, grounded) = (
            positions.as_slice(),
            velocities.as_slice(),
            grounded.as_slice(),
        );
        let mut out = vec![0.0f64; n * OUT];
        for (i, &s) in slots.iter().enumerate() {
            let Some((r, bot)) = &mut self.bots[s as usize] else {
                continue;
            };
            let c = bot.command(
                &self.routes[*r],
                v3(positions[i]),
                v3(velocities[i]),
                grounded[i] != 0,
                &mut engine,
                &tun,
                delta,
            );
            out[i * OUT] = c.yaw;
            out[i * OUT + 1] = c.mv[0] as f64;
            out[i * OUT + 2] = c.mv[1] as f64;
            out[i * OUT + 3] = c.buttons as f64;
        }
        PackedFloat64Array::from(&out[..])
    }
}
