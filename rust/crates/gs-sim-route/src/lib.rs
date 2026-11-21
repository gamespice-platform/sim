//! Airdrift's route bot to the bit: `AirdriftRouteBot.command`, the bot that follows a route of
//! points written down per map and air-strafes to keep up with it.
//!
//! The game drives every route bot once a tick, and in script each one costs three to five
//! `intersect_ray` calls, each with a fresh `PhysicsRayQueryParameters3D` and a result dictionary,
//! plus the arithmetic around them. Here the arithmetic is the script's, step for step: script
//! floats in `f64`, engine vectors in `f32` (a script float into a vector rounds to `f32`, a
//! vector's component read in script widens to `f64`), `Vector3 * float` with the scalar rounded
//! first, `normalized()` zero for a non-finite or zero vector, `atan2`/`sin`/`cos`/`sqrt` in `f64`,
//! `angle_to` in `f32`. The rays are the caller's ([`Space`]), cast in the script's order and
//! only where the script casts them (its `or` stops at the first true), so the same world answers
//! the same rays.

use gs_sim_math::{Vec3, clampf, minf, rad_to_deg};

#[cfg(test)]
mod tests;

/// Metres per genre unit (`AirdriftUnits.METRES_PER_UNIT`).
pub const METRES_PER_UNIT: f64 = 0.01905;
/// Genre units per metre, the script's `1.0 / METRES_PER_UNIT` in `f64`.
pub const UNITS_PER_METRE: f64 = 1.0 / METRES_PER_UNIT;

const JUMP_FROM: f64 = 230.0;
const STRAIGHT_DEGREES: f64 = 2.0;
const TRACE_UNITS: f64 = 160.0;
const GAIN_MARGIN: f64 = 20.0;
const MAX_AIM_SPEED: f64 = 3500.0;
const LEDGE_LEAD: f64 = 6.0;
const LEDGE_TICKS: f64 = 3.0;
const LEDGE_DROP: f64 = 24.0;
const LEDGE_STEP: f64 = 4.0;
const STEP_PLUS: f64 = 22.0;
const WALL_LOOK: f64 = 40.0;
const STUCK_SECONDS: f64 = 2.0;
const STUCK_RADIUS: f64 = 1.0;
const UNSTICK_SECONDS: f64 = 1.0;
const UNSTICK_TURNS: [f64; 7] = [90.0, -90.0, 180.0, 45.0, -45.0, 135.0, -135.0];

/// `GSFpsCommand.BUTTON_JUMP`.
pub const BUTTON_JUMP: i64 = 1;

const INF3: Vec3 = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const DOWN: Vec3 = Vec3::new(0.0, -1.0, 0.0);
const FORWARD: Vec3 = Vec3::new(0.0, 0.0, -1.0);

/// `AirdriftUnits.to_metres`.
#[inline]
fn metres(units: f64) -> f64 {
    units * METRES_PER_UNIT
}

/// The script's `deg_to_rad`.
#[inline]
fn deg_to_rad(d: f64) -> f64 {
    d * (std::f64::consts::PI / 180.0)
}

/// The script's `maxf`: the engine's `MAX`, `a > b ? a : b`.
#[inline]
fn maxf(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// The script's `signf`: the engine's `SIGN`.
#[inline]
fn signf(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// The script's `wrapf` (`Math::wrapf` in `f64`).
#[inline]
fn wrapf(v: f64, lo: f64, hi: f64) -> f64 {
    let range = hi - lo;
    if range.abs() < 0.00001 {
        return lo;
    }
    let result = v - (range * ((v - lo) / range).floor());
    if result == hi {
        return lo;
    }
    let mut tolerance = 0.00001 * result.abs();
    if tolerance < 0.00001 {
        tolerance = 0.00001;
    }
    if (result - hi).abs() < tolerance {
        return lo;
    }
    result
}

/// `Vector3.normalized()` as the engine: zero for a non-finite or zero vector.
#[inline]
fn normalized(v: Vec3) -> Vec3 {
    if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
        return ZERO;
    }
    v.normalized()
}

/// `Vector2(a, b).length()` for two script floats: each rounded to `f32`, the length in `f32`.
#[inline]
fn len2(a: f64, b: f64) -> f32 {
    let (x, y) = (a as f32, b as f32);
    (x * x + y * y).sqrt()
}

/// The script's `atan2(-v.x, -v.z)` in degrees: a yaw looking along `v`.
#[inline]
fn yaw_along(x: f64, z: f64) -> f64 {
    rad_to_deg((-x).atan2(-z))
}

/// One point of a route, as the script's dictionary holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Metres.
    pub at: Vec3,
    /// Metres.
    pub radius: f64,
    /// Units per second; 0 for no cap.
    pub max: f64,
    pub walk: bool,
    pub hop: bool,
    pub land: bool,
    pub door: bool,
    pub launch: bool,
    pub slide: bool,
}

/// A route: its points and the finish's centre (metres), aimed at once every point is passed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Route {
    pub points: Vec<Point>,
    pub finish: Vec3,
}

/// The tunables the bot reads, as the script reads them (`f64`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tunables {
    /// Degrees.
    pub max_slope_angle: f64,
    pub gravity: f64,
    pub max_speed: f64,
    pub air_accelerate: f64,
    pub max_air_wish_speed: f64,
}

/// A ray's hit: `intersect_ray`'s `position` and `normal`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub position: Vec3,
    pub normal: Vec3,
}

/// The world the rays are cast in: `intersect_ray(PhysicsRayQueryParameters3D.create(from, to))`.
pub trait Space {
    fn ray(&mut self, from: Vec3, to: Vec3) -> Option<Hit>;
}

/// A tick's command: the parts of a `GSFpsCommand` the bot sets (pitch stays 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Command {
    pub yaw: f64,
    pub mv: [f32; 2],
    pub buttons: i64,
}

/// One bot's state on its route, as the script's fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bot {
    pub index: i64,
    pub surfed: i64,
    pub side: f64,
    pub leg_from: Vec3,
    pub unstuck: i64,
    pub anchor: Vec3,
    pub held: f64,
    pub unstick_left: f64,
    pub unstick_turn: f64,
}

/// Values in [`Bot::to_state`]'s vector.
pub const STATE_LEN: usize = 13;

impl Default for Bot {
    fn default() -> Self {
        Self {
            index: 0,
            surfed: 0,
            side: 1.0,
            leg_from: INF3,
            unstuck: 0,
            anchor: INF3,
            held: 0.0,
            unstick_left: 0.0,
            unstick_turn: 0.0,
        }
    }
}

impl Bot {
    /// The state as [`STATE_LEN`] numbers: index, surfed, side, leg_from xyz, unstuck, anchor xyz,
    /// held, unstick_left, unstick_turn.
    pub fn to_state(&self) -> [f64; STATE_LEN] {
        [
            self.index as f64,
            self.surfed as f64,
            self.side,
            self.leg_from.x as f64,
            self.leg_from.y as f64,
            self.leg_from.z as f64,
            self.unstuck as f64,
            self.anchor.x as f64,
            self.anchor.y as f64,
            self.anchor.z as f64,
            self.held,
            self.unstick_left,
            self.unstick_turn,
        ]
    }

    /// The state back from [`Bot::to_state`]'s layout.
    pub fn from_state(s: &[f64; STATE_LEN]) -> Self {
        Self {
            index: s[0] as i64,
            surfed: s[1] as i64,
            side: s[2],
            leg_from: Vec3::new(s[3] as f32, s[4] as f32, s[5] as f32),
            unstuck: s[6] as i64,
            anchor: Vec3::new(s[7] as f32, s[8] as f32, s[9] as f32),
            held: s[10],
            unstick_left: s[11],
            unstick_turn: s[12],
        }
    }

    /// `restart()`: from the first point again.
    pub fn restart(&mut self) {
        self.index = 0;
        self.leg_from = INF3;
        self.anchor = INF3;
        self.unstick_left = 0.0;
    }

    /// `teleported()`: a door moved the player, so the door point it walked into is passed.
    pub fn teleported(&mut self, route: &Route) {
        if let Some(p) = self.point(route)
            && p.door
        {
            self.index += 1;
        }
        self.leg_from = INF3;
    }

    fn point<'r>(&self, route: &'r Route) -> Option<&'r Point> {
        if self.index < 0 {
            return None;
        }
        route.points.get(self.index as usize)
    }

    /// `target()`: the point aimed at now, or the finish once every point is passed.
    pub fn target(&self, route: &Route) -> Vec3 {
        self.point(route).map_or(route.finish, |p| p.at)
    }

    /// `command(state, space, tunables, delta)` for a player at `at` moving at `vel`.
    #[allow(clippy::too_many_arguments)]
    pub fn command(
        &mut self,
        route: &Route,
        at: Vec3,
        vel: Vec3,
        grounded: bool,
        space: &mut impl Space,
        tun: &Tunables,
        delta: f64,
    ) -> Command {
        let max_slope = tun.max_slope_angle;
        if self.leg_from == INF3 {
            self.leg_from = at;
        }
        self.advance(route, at, grounded);
        let goal = self.target(route);
        if self.watch_progress(at, delta) {
            return self.unstick(at, grounded, goal);
        }
        let point = self.point(route).copied();
        let to = flat_to(goal, at);
        let goal_yaw = yaw_along(to.x as f64, to.z as f64);
        let flat = Vec3::new(vel.x, 0.0, vel.z);
        let speed = flat.length() as f64 * UNITS_PER_METRE;
        let cap = point.map_or(0.0, |p| p.max);
        let hop = point.is_some_and(|p| p.hop);
        let slide = point.is_some_and(|p| p.slide);
        let walk = point.is_some_and(|p| p.walk);

        let mut c = Command::default();
        let ramp = ramp_under(space, at, flat, max_slope);
        if ramp != ZERO && slide {
            c = self.air(at, vel, goal, flat, cap, tun, delta);
        } else if ramp != ZERO {
            let inward = normalized(Vec3::new(-ramp.x, 0.0, -ramp.z));
            let mut along = Vec3::new(inward.z, 0.0, -inward.x);
            let going = if to.length() as f64 > 0.01 { to } else { flat };
            if (along.dot(going) as f64) < 0.0 {
                along = -along;
            }
            c.yaw = yaw_along(along.x as f64, along.z as f64);
            let r = deg_to_rad(c.yaw);
            let right = Vec3::new(r.cos() as f32, 0.0, (-r.sin()) as f32);
            c.mv = [signf(right.dot(inward) as f64) as f32, 0.0];
            self.surfed += 1;
        } else if grounded {
            c.yaw = goal_yaw;
            c.mv = [0.0, 1.0];
            let mut jump = false;
            if !walk && !slide {
                let dir = if speed > 30.0 {
                    normalized(flat)
                } else {
                    normalized(to)
                };
                jump = (hop && speed > JUMP_FROM)
                    || ledge_ahead(space, at, dir, speed)
                    || wall_ahead(space, at, dir)
                    || self.on_launch(route, at);
            }
            if jump {
                c.buttons |= BUTTON_JUMP;
            }
        } else {
            c = self.air(at, vel, goal, flat, cap, tun, delta);
        }
        c
    }

    fn on_launch(&self, route: &Route, at: Vec3) -> bool {
        let n = route.points.len() as i64;
        if self.index == 0 || self.index > n || self.index < 0 {
            return false;
        }
        let p = route.points[(self.index - 1) as usize];
        if !p.launch {
            return false;
        }
        (len2(at.x as f64 - p.at.x as f64, at.z as f64 - p.at.z as f64) as f64) < 2.0 * p.radius
    }

    fn watch_progress(&mut self, at: Vec3, delta: f64) -> bool {
        if self.unstick_left > 0.0 {
            self.unstick_left -= delta;
            self.anchor = at;
            self.held = 0.0;
            return self.unstick_left > 0.0;
        }
        if self.anchor == INF3
            || len2(
                at.x as f64 - self.anchor.x as f64,
                at.z as f64 - self.anchor.z as f64,
            ) as f64
                > STUCK_RADIUS
            || (at.y as f64 - self.anchor.y as f64).abs() > 2.0 * STUCK_RADIUS
        {
            self.anchor = at;
            self.held = 0.0;
            return false;
        }
        self.held += delta;
        if self.held < STUCK_SECONDS {
            return false;
        }
        self.held = 0.0;
        self.unstick_left = UNSTICK_SECONDS;
        self.unstick_turn =
            UNSTICK_TURNS[self.unstuck.rem_euclid(UNSTICK_TURNS.len() as i64) as usize];
        self.unstuck += 1;
        true
    }

    fn unstick(&mut self, at: Vec3, grounded: bool, goal: Vec3) -> Command {
        let mut c = Command {
            yaw: yaw_along(goal.x as f64 - at.x as f64, goal.z as f64 - at.z as f64)
                + self.unstick_turn,
            mv: [0.0, 1.0],
            buttons: 0,
        };
        if !grounded && (self.unstuck / 2) % 2 == 0 {
            self.side = -self.side;
            c.mv = [self.side as f32, 1.0];
        }
        if grounded {
            c.buttons |= BUTTON_JUMP;
        }
        c
    }

    #[allow(clippy::too_many_arguments)]
    fn air(
        &mut self,
        at: Vec3,
        vel: Vec3,
        goal: Vec3,
        flat: Vec3,
        cap: f64,
        tun: &Tunables,
        delta: f64,
    ) -> Command {
        let mut c = Command::default();
        let to = flat_to(goal, at);
        let g = tun.gravity;
        let vy = vel.y as f64;
        let drop = goal.y as f64 - at.y as f64;
        let disc = vy * vy - 2.0 * g * drop;
        let t = if disc >= 0.0 {
            (vy + disc.sqrt()) / g
        } else {
            0.0
        };
        let mut want_speed = if t > delta {
            to.length() as f64 / t
        } else {
            f64::INFINITY
        };
        if cap > 0.0 {
            want_speed = minf(want_speed, metres(cap));
        }
        let speed = flat.length() as f64;
        let goal_yaw = yaw_along(to.x as f64, to.z as f64);
        let heading = if speed > 0.5 {
            yaw_along(flat.x as f64, flat.z as f64)
        } else {
            goal_yaw
        };
        let off = wrapf(goal_yaw - heading, -180.0, 180.0);

        if want_speed > speed + metres(GAIN_MARGIN) && off.abs() < 90.0 {
            c.yaw = heading;
            if off.abs() < STRAIGHT_DEGREES {
                self.side = -self.side;
            } else {
                self.side = if off > 0.0 { -1.0 } else { 1.0 };
            }
            c.mv = [self.side as f32, 0.0];
            return c;
        }

        let want = if to.length() as f64 > 0.01 {
            normalized(to).scale(minf(want_speed, metres(MAX_AIM_SPEED)))
        } else {
            ZERO
        };
        let change = want - flat;
        let need = change.length() as f64;
        if need < 0.01 {
            return c;
        }
        let w = change.div_scalar(need);
        let run = tun.max_speed;
        let along = flat.dot(w) as f64;
        let per_unit = tun.air_accelerate * run * delta;
        let mut m = 1.0;
        if need + along <= tun.max_air_wish_speed {
            m = clampf(maxf(need / per_unit, (need + along) / run), 0.0, 1.0);
        }
        c.yaw = yaw_along(w.x as f64, w.z as f64);
        c.mv = [0.0, m as f32];
        c
    }

    fn advance(&mut self, route: &Route, at: Vec3, grounded: bool) {
        while self.index >= 0 && (self.index as usize) < route.points.len() {
            let pt = route.points[self.index as usize];
            let p = pt.at;
            let (fx, fz) = (
                (at.x as f64 - p.x as f64) as f32,
                (at.z as f64 - p.z as f64) as f32,
            );
            let (lx, lz) = (
                (p.x as f64 - self.leg_from.x as f64) as f32,
                (p.z as f64 - self.leg_from.z as f64) as f32,
            );
            let near = ((fx * fx + fz * fz).sqrt() as f64) < pt.radius;
            let beyond =
                ((lx * lx + lz * lz).sqrt() as f64) > 0.01 && ((fx * lx + fz * lz) as f64) > 0.0;
            if !(near || beyond) {
                return;
            }
            if pt.land && !grounded {
                return;
            }
            if pt.door {
                return;
            }
            self.leg_from = p;
            self.index += 1;
        }
    }
}

/// `Vector3(goal.x - at.x, 0.0, goal.z - at.z)`: script differences rounded into the vector.
#[inline]
fn flat_to(goal: Vec3, at: Vec3) -> Vec3 {
    Vec3::new(
        (goal.x as f64 - at.x as f64) as f32,
        0.0,
        (goal.z as f64 - at.z as f64) as f32,
    )
}

/// `ledge_ahead`: no ground under the point the player is about to move over.
pub fn ledge_ahead(space: &mut impl Space, position: Vec3, dir: Vec3, speed: f64) -> bool {
    let ahead = metres(LEDGE_LEAD + speed * LEDGE_TICKS / 128.0);
    let from = position + dir.scale(ahead) + UP.scale(metres(8.0));
    let to = from + DOWN.scale(metres(8.0 + LEDGE_DROP));
    match space.ray(from, to) {
        None => true,
        Some(hit) => (hit.position.y as f64) < position.y as f64 - metres(LEDGE_STEP),
    }
}

/// `wall_ahead`: something too tall to walk up just ahead of the knees.
pub fn wall_ahead(space: &mut impl Space, position: Vec3, dir: Vec3) -> bool {
    let from = position + UP.scale(metres(STEP_PLUS));
    let to = from + dir.scale(metres(WALL_LOOK));
    match space.ray(from, to) {
        None => false,
        Some(hit) => (hit.normal.y as f64).abs() < 0.7,
    }
}

/// `ramp_under`: the normal of a surf face under or beside the player, or zero.
pub fn ramp_under(space: &mut impl Space, position: Vec3, flat: Vec3, limit: f64) -> Vec3 {
    let feet = position + UP.scale(metres(8.0));
    let ahead = if flat.length() as f64 > 0.01 {
        normalized(flat)
    } else {
        FORWARD
    };
    let side = Vec3::new(-ahead.z, 0.0, ahead.x);
    let reach = metres(TRACE_UNITS);
    for dir in [DOWN, normalized(DOWN + side), normalized(DOWN - side)] {
        let Some(hit) = space.ray(feet, feet + dir.scale(reach)) else {
            continue;
        };
        let angle = rad_to_deg(hit.normal.angle_to(UP) as f64);
        if angle > limit && angle < 85.0 {
            return hit.normal;
        }
    }
    ZERO
}
