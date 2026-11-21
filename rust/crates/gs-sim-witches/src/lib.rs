//! Look at Me's witches: mg-look-at-me's `LmGame._watch` (`_listen` for every witch, then
//! `exposure` for every witch) over one player, to the bit.
//!
//! A witch walks a loop on a clock (`LmLevel.witch_pose`), hears footsteps, a door, a voice and a
//! lit flashlight, sees with a cone, and on a sound stands to listen, turns, runs at it and walks
//! back (`_mode_walk`, `_mode_listen`, `_mode_turn`, `_mode_chase`, `_mode_back`). Scalars are the
//! script's `f64`, vectors the engine's `f32` (`gs_sim_math`), in the script's order; the line of
//! sight is a [`Rays`] the caller casts (the engine's space in the game, a fake in tests), asked in
//! the script's order and only where the script asks it.

use gs_sim_math::{Vec3, clampf, maxf, minf};
use std::f64::consts::PI;

/// The witch walks her loop.
pub const MODE_WALK: i32 = 0;
/// She stands and listens.
pub const MODE_LISTEN: i32 = 1;
/// She turns her head round to the sound.
pub const MODE_TURN: i32 = 2;
/// She runs at it.
pub const MODE_CHASE: i32 = 3;
/// She walks back to where her clock has her.
pub const MODE_BACK: i32 = 4;

/// Seconds a door she passed stays open, as the level's `DOOR_HELD`.
pub const DOOR_HELD: f64 = 2.0;

/// The engine's `CMP_EPSILON`.
const CMP_EPSILON: f64 = 0.00001;

/// The script's `deg_to_rad`, in `f64`.
#[inline]
pub fn deg_to_rad(d: f64) -> f64 {
    d * (PI / 180.0)
}

/// The furthest her head turns, as `MAX_HEAD` (`deg_to_rad(140.0)`).
#[inline]
pub fn max_head() -> f64 {
    deg_to_rad(140.0)
}

/// The engine's `is_equal_approx` for `f64`.
#[inline]
pub fn is_equal_approx(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let mut tolerance = CMP_EPSILON * a.abs();
    if tolerance < CMP_EPSILON {
        tolerance = CMP_EPSILON;
    }
    (a - b).abs() < tolerance
}

/// `wrapf(value, min, max)`.
#[inline]
pub fn wrapf(value: f64, min: f64, max: f64) -> f64 {
    let range = max - min;
    if range.abs() < CMP_EPSILON {
        return min;
    }
    let result = value - range * ((value - min) / range).floor();
    if is_equal_approx(result, max) {
        return min;
    }
    result
}

/// `smoothstep(from, to, s)`.
#[inline]
pub fn smoothstep(from: f64, to: f64, s: f64) -> f64 {
    if is_equal_approx(from, to) {
        if from <= to {
            return if s <= from { 0.0 } else { 1.0 };
        }
        return if s <= to { 1.0 } else { 0.0 };
    }
    let s = clampf((s - from) / (to - from), 0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// `lerpf(from, to, weight)`.
#[inline]
pub fn lerpf(from: f64, to: f64, w: f64) -> f64 {
    from + (to - from) * w
}

/// `Vector3.lerp`: the weight rounded to `f32`, per component.
#[inline]
pub fn lerp3(from: Vec3, to: Vec3, weight: f64) -> Vec3 {
    let w = weight as f32;
    Vec3::new(
        from.x + (to.x - from.x) * w,
        from.y + (to.y - from.y) * w,
        from.z + (to.z - from.z) * w,
    )
}

/// `atan2(-v.x, -v.z)`: the heading that looks along `v`.
#[inline]
fn heading(v: Vec3) -> f64 {
    (-f64::from(v.x)).atan2(-f64::from(v.z))
}

/// `Vector3(-sin(facing), 0.0, -cos(facing))`.
#[inline]
fn forward(facing: f64) -> Vec3 {
    Vec3::new(-facing.sin() as f32, 0.0, -facing.cos() as f32)
}

/// `Vector3(v.x, 0.0, v.z)`.
#[inline]
fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Her eyes over a point of her loop, `Vector3(0, 2.0, 0)` up.
const UP: Vec3 = Vec3::new(0.0, 2.0, 0.0);

/// One witch's loop, as `LmLevel._witch_loop` builds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loop {
    pub points: Vec<Vec3>,
    /// Each leg's length, `f32` as the script's `PackedFloat32Array`.
    pub lengths: Vec<f32>,
    /// The door at each point, `-1` for none (the script's `""`).
    pub doors: Vec<i32>,
    pub total: f64,
    pub speed: f64,
    pub sight: f64,
    pub cone: f64,
}

/// Where a witch is: the script's pose dictionary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub at: Vec3,
    pub facing: f64,
    pub head: f64,
    /// The door she holds open, `-1` for none.
    pub door: i32,
    pub open: f64,
}

/// `LmLevel.witch_pose(index, seconds, speed_scale, sweep, dwell)`.
pub fn witch_pose(
    l: &Loop,
    index: usize,
    seconds: f64,
    speed_scale: f64,
    sweep: bool,
    dwell: f64,
) -> Pose {
    let n = l.points.len();
    if n == 0 || l.lengths.is_empty() {
        // The script cannot index an empty loop; the game never builds one.
        return Pose {
            door: -1,
            ..Pose::default()
        };
    }
    let door_at = |i: usize| l.doors.get(i).copied().unwrap_or(-1);
    let v = maxf(l.speed * speed_scale, 0.001);
    let mut stops = 0i64;
    if dwell > 0.0 {
        stops = l.doors.iter().filter(|d| **d >= 0).count() as i64;
    }
    let cycle = l.total / v + stops as f64 * dwell;
    let mut u = (seconds + index as f64 * 7.3 / v) % cycle;
    let mut i = 0usize;
    let at;
    let mut door = -1;
    let mut open = 0.0;
    loop {
        let here = if dwell > 0.0 && door_at(i) >= 0 {
            dwell
        } else {
            0.0
        };
        let a = l.points[i % n];
        let b = l.points[(i + 1) % n];
        if u < here {
            at = a;
            door = door_at(i);
            open = smoothstep(0.0, here * 0.5, u);
            break;
        }
        u -= here;
        let len = f64::from(l.lengths[i]);
        let leg = len / v;
        if u <= leg || i + 1 >= l.lengths.len() {
            at = lerp3(a, b, clampf(u * v / maxf(len, 0.001), 0.0, 1.0));
            if here > 0.0 {
                door = door_at(i);
                open = if u > DOOR_HELD {
                    clampf(DOOR_HELD + 1.0 - u, 0.0, 1.0)
                } else {
                    1.0
                };
            }
            break;
        }
        u -= leg;
        i += 1;
    }
    let dir = l.points[(i + 1) % n] - l.points[i % n];
    let facing = if f64::from(dir.length_squared()) > 0.0001 {
        heading(dir)
    } else {
        0.0
    };
    let head = if sweep {
        (seconds * 0.8 + index as f64).sin() * deg_to_rad(35.0)
    } else {
        0.0
    };
    Pose {
        at,
        facing,
        head,
        door: if open > 0.0 { door } else { -1 },
        open,
    }
}

/// The game's tuning the witches read, as `LmConfig`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Config {
    pub speed_scale: f64,
    pub head_sweep: bool,
    pub door_seconds: f64,
    pub sight_scale: f64,
    pub flashlight_sight: f64,
    pub cone_scale: f64,
    pub exposure_rate: f64,
    pub opening_ease: f64,
    pub opening_levels: i64,
    pub flashlight_exposure: f64,
    pub crouch_exposure: f64,
    pub still_exposure: f64,
    pub listen_hearing: f64,
    pub light_range: f64,
    pub hear_walls: f64,
    pub reaction_seconds: f64,
    pub alert_seconds: f64,
    pub listens: bool,
    pub listen_seconds: f64,
    pub listen_confirm: f64,
    pub listen_cooldown: f64,
    pub turn_delay: f64,
    pub chase_seconds: f64,
    pub turn_rate: f64,
    pub chase_speed: f64,
}

/// One witch's memory of one player, the script's `player.gaze[i]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gaze {
    pub head: f64,
    pub notice: f64,
    pub alert: bool,
    pub hold: f64,
    pub at: Vec3,
    pub mode: i32,
    pub t: f64,
    pub heard: f64,
    pub stall: f64,
    pub cool: f64,
    pub pos: Vec3,
    pub face: f64,
}

/// What the witches read of the player this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Player {
    /// `noise_radius(player)`, after `_listen` has run the noise and voice down.
    pub noise: f64,
    /// `horizontal_speed()`.
    pub hspeed: f64,
    pub caught_left: f64,
    pub flashlight: bool,
    pub crouching: bool,
    /// The player's level number (`player.level`).
    pub level: i64,
    /// `eye_position()`.
    pub target: Vec3,
}

/// A line of sight: nothing solid between `from` and `to` with the player's own exclusions and,
/// when `open_door` is not `-1`, that door's body left out too (`_clear_between`).
pub trait Rays {
    fn clear(&mut self, from: Vec3, to: Vec3, open_door: i32) -> bool;
}

impl<F: FnMut(Vec3, Vec3, i32) -> bool> Rays for F {
    fn clear(&mut self, from: Vec3, to: Vec3, open_door: i32) -> bool {
        self(from, to, open_door)
    }
}

/// A level's witches, where the level stands.
pub struct Level<'a> {
    pub origin: Vec3,
    pub loops: &'a [Loop],
}

/// `_pose_of(level, index, seconds, state)`.
pub fn pose_of(cfg: &Config, l: &Loop, index: usize, seconds: f64, state: Option<&Gaze>) -> Pose {
    let stall = state.map_or(0.0, |s| s.stall);
    let mut pose = witch_pose(
        l,
        index,
        maxf(seconds - stall, 0.0),
        cfg.speed_scale,
        cfg.head_sweep,
        cfg.door_seconds,
    );
    let Some(s) = state else {
        return pose;
    };
    pose.head = s.head;
    if s.mode == MODE_CHASE || s.mode == MODE_BACK {
        pose.at = s.pos;
        pose.facing = s.face;
        pose.door = -1;
        pose.open = 0.0;
    }
    pose
}

/// The state `_listen` starts a witch on for a player she has no memory of.
pub fn fresh(cfg: &Config, l: &Loop, index: usize, seconds: f64, target: Vec3) -> Gaze {
    let first = witch_pose(
        l,
        index,
        seconds,
        cfg.speed_scale,
        cfg.head_sweep,
        cfg.door_seconds,
    );
    Gaze {
        head: first.head,
        at: target,
        mode: MODE_WALK,
        ..Gaze::default()
    }
}

/// `_sense`: heard, light and glimpse.
#[allow(clippy::too_many_arguments)]
fn sense(
    cfg: &Config,
    l: &Loop,
    p: &Player,
    state: &Gaze,
    pose: &Pose,
    mode: i32,
    eye: Vec3,
    open_door: i32,
    live: bool,
    rays: &mut impl Rays,
) -> (bool, bool, bool) {
    let target = p.target;
    let to = target - eye;
    let distance = f64::from(to.length());
    let mut heard = false;
    let mut light = false;
    let mut glimpse = false;
    let mut clear = false;
    let ears = p.noise
        * (if mode == MODE_LISTEN {
            cfg.listen_hearing
        } else {
            1.0
        });
    if live {
        let fwd = forward(pose.facing + state.head);
        let half_cone = deg_to_rad(l.cone * cfg.cone_scale * 0.5);
        let fl = flat(to).normalized();
        let in_cone = f64::from(fwd.dot(fl)) >= half_cone.cos();
        let reach = maxf(ears, if p.flashlight { cfg.light_range } else { 0.0 });
        if distance <= maxf(reach, l.sight * cfg.sight_scale) {
            clear = rays.clear(eye, target, open_door);
        }
        heard = ears > 0.0 && distance <= ears * (if clear { 1.0 } else { cfg.hear_walls });
        light = clear && p.flashlight && distance <= cfg.light_range && in_cone;
        if clear && !in_cone && (p.hspeed > 0.3 || p.flashlight) {
            let edge = deg_to_rad(l.cone * cfg.cone_scale * 0.5 + 35.0);
            glimpse = f64::from(fwd.dot(fl)) >= edge.cos() && distance <= l.sight * cfg.sight_scale;
        }
    }
    (heard, light, glimpse)
}

/// `_mode_walk`; true when she began to listen (the script's `listened` signal).
fn mode_walk(
    cfg: &Config,
    p: &Player,
    s: &mut Gaze,
    heard: bool,
    light: bool,
    glimpse: bool,
    delta: f64,
) -> bool {
    let mut listened = false;
    if light {
        s.notice += delta;
        s.at = p.target;
        if s.notice >= cfg.reaction_seconds {
            s.alert = true;
            s.hold = cfg.alert_seconds;
        }
    } else if (heard || glimpse) && cfg.listens && s.cool <= 0.0 {
        s.notice += delta;
        s.at = p.target;
        if s.notice >= cfg.reaction_seconds {
            s.notice = 0.0;
            s.mode = MODE_LISTEN;
            s.t = cfg.listen_seconds;
            s.heard = 0.0;
            s.alert = false;
            listened = true;
        }
    } else if heard && !cfg.listens {
        s.notice += delta;
        s.at = p.target;
        if s.notice >= cfg.reaction_seconds {
            s.alert = true;
            s.hold = cfg.alert_seconds;
        }
    } else {
        s.notice = maxf(s.notice - delta, 0.0);
    }
    if s.alert && !light && !heard {
        s.hold -= delta;
        if s.hold <= 0.0 {
            s.alert = false;
        }
    }
    listened
}

/// `_mode_listen`.
fn mode_listen(cfg: &Config, p: &Player, s: &mut Gaze, heard: bool, delta: f64) -> f64 {
    s.stall += delta;
    s.t -= delta;
    let wanted = s.head;
    if heard {
        s.heard += delta;
        s.at = p.target;
    } else {
        s.heard = maxf(s.heard - delta * 0.5, 0.0);
    }
    if s.heard >= cfg.listen_confirm {
        s.mode = MODE_TURN;
        s.t = cfg.turn_delay;
        s.alert = true;
        s.hold = cfg.alert_seconds;
    } else if s.t <= 0.0 {
        s.mode = MODE_WALK;
        s.cool = cfg.listen_cooldown;
        s.notice = 0.0;
    }
    wanted
}

/// `_mode_turn`.
#[allow(clippy::too_many_arguments)]
fn mode_turn(
    cfg: &Config,
    p: &Player,
    s: &mut Gaze,
    pose: &Pose,
    heard: bool,
    live: bool,
    eye: Vec3,
    open_door: i32,
    delta: f64,
    mut wanted: f64,
    rays: &mut impl Rays,
) -> f64 {
    s.stall += delta;
    s.t -= delta;
    if heard {
        s.at = p.target;
    }
    let wanted_turn = wrapf(heading(s.at - eye) - pose.facing, -PI, PI);
    let mh = max_head();
    let looked = wrapf(clampf(wanted_turn, -mh, mh) - s.head, -PI, PI).abs() < 0.35;
    if s.t <= 0.0 && (looked || s.t < -3.0) {
        if live && rays.clear(eye, s.at, open_door) {
            s.mode = MODE_CHASE;
            s.t = cfg.chase_seconds;
            s.pos = pose.at;
            s.face = pose.facing + s.head;
            s.head = 0.0;
            wanted = 0.0;
        } else {
            s.mode = MODE_WALK;
            s.cool = cfg.listen_cooldown;
        }
    }
    wanted
}

/// `_mode_chase`.
#[allow(clippy::too_many_arguments)]
fn mode_chase(
    cfg: &Config,
    lv: &Level,
    l: &Loop,
    p: &Player,
    s: &mut Gaze,
    heard: bool,
    light: bool,
    delta: f64,
    rays: &mut impl Rays,
) -> f64 {
    s.stall += delta;
    s.t -= delta;
    if heard || light {
        s.at = p.target;
    }
    let here = s.pos;
    let here_eye = lv.origin + here + UP;
    let aim = s.at - here_eye;
    let flat_aim = flat(aim);
    let face = heading(aim);
    let turn = deg_to_rad(cfg.turn_rate) * 3.0 * delta;
    s.face += clampf(wrapf(face - s.face, -PI, PI), -turn, turn);
    let step = l.speed * cfg.speed_scale * cfg.chase_speed * delta;
    let next = here
        + flat_aim
            .normalized()
            .scale(minf(step, f64::from(flat_aim.length())));
    if f64::from(flat_aim.length()) < 1.0
        || s.t <= 0.0
        || p.caught_left > 0.0
        || !rays.clear(here_eye, lv.origin + next + UP, -1)
    {
        s.mode = MODE_BACK;
        s.alert = false;
        s.cool = cfg.listen_cooldown;
    } else {
        s.pos = next;
    }
    0.0
}

/// `_mode_back`.
fn mode_back(cfg: &Config, l: &Loop, index: usize, s: &mut Gaze, seconds: f64, delta: f64) -> f64 {
    s.stall += delta;
    // The script does not clamp this clock at zero, unlike `_pose_of`.
    let home = witch_pose(
        l,
        index,
        seconds - s.stall,
        cfg.speed_scale,
        cfg.head_sweep,
        cfg.door_seconds,
    )
    .at;
    let here = s.pos;
    let way = Vec3::new(home.x - here.x, 0.0, home.z - here.z);
    let step = l.speed * cfg.speed_scale * delta;
    if f64::from(way.length()) <= maxf(step, 0.2) {
        s.mode = MODE_WALK;
    } else {
        s.pos = here + way.normalized().scale(step);
        s.face = heading(way);
    }
    0.0
}

/// `_listen_witch` for witch `index` over `state` (already started by [`fresh`] when new); true
/// when she began to listen this tick.
#[allow(clippy::too_many_arguments)]
pub fn listen_witch(
    cfg: &Config,
    lv: &Level,
    index: usize,
    p: &Player,
    state: &mut Gaze,
    seconds: f64,
    delta: f64,
    rays: &mut impl Rays,
) -> bool {
    let l = &lv.loops[index];
    let pose = pose_of(cfg, l, index, seconds, Some(state));
    let mode = state.mode;
    let eye = lv.origin + pose.at + UP;
    let to = p.target - eye;
    let distance = f64::from(to.length());
    let open_door = if pose.open > 0.5 { pose.door } else { -1 };
    let live = distance > 0.01 && p.caught_left <= 0.0;
    let (heard, light, glimpse) = sense(cfg, l, p, state, &pose, mode, eye, open_door, live, rays);
    state.cool = maxf(state.cool - delta, 0.0);
    let mut wanted = pose.head;
    let mut listened = false;
    match mode {
        MODE_WALK => listened = mode_walk(cfg, p, state, heard, light, glimpse, delta),
        MODE_LISTEN => wanted = mode_listen(cfg, p, state, heard, delta),
        MODE_TURN => {
            wanted = mode_turn(
                cfg, p, state, &pose, heard, live, eye, open_door, delta, wanted, rays,
            )
        }
        MODE_CHASE => wanted = mode_chase(cfg, lv, l, p, state, heard, light, delta, rays),
        MODE_BACK => wanted = mode_back(cfg, l, index, state, seconds, delta),
        _ => {}
    }
    if state.alert && state.mode != MODE_CHASE && state.mode != MODE_BACK {
        let look = state.at - eye;
        let mh = max_head();
        wanted = clampf(wrapf(heading(look) - pose.facing, -PI, PI), -mh, mh);
    }
    let turn_step = deg_to_rad(cfg.turn_rate) * delta;
    let head = state.head;
    state.head = head + clampf(wrapf(wanted - head, -PI, PI), -turn_step, turn_step);
    listened
}

/// `exposure(player, level, index, seconds)` with the witch's state after `_listen`.
pub fn exposure(
    cfg: &Config,
    lv: &Level,
    index: usize,
    p: &Player,
    state: Option<&Gaze>,
    seconds: f64,
    rays: &mut impl Rays,
) -> f64 {
    let l = &lv.loops[index];
    let pose = pose_of(cfg, l, index, seconds, state);
    let eye = lv.origin + pose.at + UP;
    let to = p.target - eye;
    let sight = l.sight
        * cfg.sight_scale
        * (if p.flashlight {
            cfg.flashlight_sight
        } else {
            1.0
        });
    let distance = f64::from(to.length());
    if distance > sight || distance < 0.01 {
        return 0.0;
    }
    let fwd = forward(pose.facing + pose.head);
    let fl = flat(to).normalized();
    let half_cone = deg_to_rad(l.cone * cfg.cone_scale * 0.5);
    if f64::from(fwd.dot(fl)) < half_cone.cos() {
        return 0.0;
    }
    let open_door = if pose.open > 0.5 { pose.door } else { -1 };
    if !rays.clear(eye, p.target, open_door) {
        return 0.0;
    }
    let mut rate = cfg.exposure_rate * clampf(1.0 - distance / sight, 0.15, 1.0);
    rate *= lerpf(
        cfg.opening_ease,
        1.0,
        clampf(
            (p.level - 1) as f64 / cfg.opening_levels.max(1) as f64,
            0.0,
            1.0,
        ),
    );
    if p.flashlight {
        rate *= cfg.flashlight_exposure;
    }
    if p.crouching {
        rate *= cfg.crouch_exposure;
    }
    if p.hspeed < 0.3 {
        rate *= cfg.still_exposure;
    }
    rate
}

/// One witch's answer for a tick: her state after `_listen`, her exposure rate and whether she
/// began to listen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Watched {
    pub state: Gaze,
    pub rate: f64,
    pub listened: bool,
}

/// `_watch`'s witch half: `_listen` over every witch (starting the ones in `states` that are
/// `None`), then `exposure` over every witch, into `out` (one per loop, in order).
#[allow(clippy::too_many_arguments)]
pub fn watch(
    cfg: &Config,
    lv: &Level,
    p: &Player,
    states: &[Option<Gaze>],
    seconds: f64,
    delta: f64,
    rays: &mut impl Rays,
    out: &mut Vec<Watched>,
) {
    out.clear();
    for (i, l) in lv.loops.iter().enumerate() {
        let mut state = states
            .get(i)
            .copied()
            .flatten()
            .unwrap_or_else(|| fresh(cfg, l, i, seconds, p.target));
        let listened = listen_witch(cfg, lv, i, p, &mut state, seconds, delta, rays);
        out.push(Watched {
            state,
            rate: 0.0,
            listened,
        });
    }
    for (i, w) in out.iter_mut().enumerate() {
        let state = w.state;
        w.rate = exposure(cfg, lv, i, p, Some(&state), seconds, rays);
    }
}

#[cfg(test)]
mod tests;
