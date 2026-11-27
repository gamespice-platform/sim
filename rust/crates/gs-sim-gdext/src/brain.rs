//! `GSSimBotMind`: one bot brain's aim state and its fight step, native. GSBotAim keeps its
//! public fields (the view, the engagement's times) and hands each call the ones it needs; the
//! private ones (aim error, overshoot, tracked velocity, bursts, the aim's random stream) live here.
//! The view comes back as `f64`s, never a `Vector2`, so the script's yaw keeps its precision.

use crate::bots::{GSSimBotSenses, Packed};
use crate::sight::{GSSimSightRay, SightQuery};
use godot::prelude::*;
use gs_sim_bots::{Found, Query, candidates};
use gs_sim_brain::aim::direction;
use gs_sim_brain::fight::FirstShot;
use gs_sim_brain::num::rad_to_deg;
use gs_sim_brain::{
    Aim, AimProfile, FightIn, FightProfile, Strafe, Stream, Vec2 as V2, Vec3 as V3, View, fight,
};
use std::collections::HashMap;
use std::sync::Arc;

/// How long a line-of-sight answer is reused (GSBotBrain.SIGHT_REUSE).
const SIGHT_REUSE: f64 = 0.045;

/// How close a body is felt from outside the view cone (GSBotBrain.CLOSE_NOTICE).
const CLOSE_NOTICE: f64 = 2.0;

/// The `io` layout of [`GSSimBotMind::fight`] (and GSBotBrain's packing of it).
mod io {
    pub const YAW: usize = 0;
    pub const PITCH: usize = 1;
    pub const REACT_AT: usize = 2;
    pub const ENGAGED_AT: usize = 3;
    pub const FIRST_SHOT_AT: usize = 4;
    pub const FIRST_SHOT_ERROR: usize = 5;
    pub const COUNTER: usize = 6;
    pub const NOW: usize = 7;
    pub const DT: usize = 8;
    pub const SPEED: usize = 9;
    pub const HEALTH: usize = 10;
    pub const ROLL_STOP: usize = 11;
    pub const ROLL_CROUCH: usize = 12;
    pub const ROLL_COVER: usize = 13;
    pub const HESITATE_UNTIL: usize = 14;
    pub const STRAFE: usize = 15;
    pub const STRAFE_UNTIL: usize = 16;
    pub const STRAFE_EASED: usize = 17;
    /// The profile from here: prediction, track_lag, overshoot, aim_floor, aim_settle, fire_cone,
    /// turn_smooth, turn_rate, max_range, burst_range, burst, burst_pause, range, strafe,
    /// strafe_jitter, strafe_time, jump.
    pub const PROFILE: usize = 18;
    /// 1 when the brain's stale-standoff clock says walk in (GSBotBrain.STALE_FIGHT).
    pub const PUSH: usize = PROFILE + 17;
    pub const LEN: usize = PUSH + 1;
}

fn v3(v: Vector3) -> V3 {
    V3::new(v.x, v.y, v.z)
}

fn aim_profile(a: &[f64]) -> AimProfile {
    AimProfile {
        prediction: a[0],
        track_lag: a[1],
        overshoot: a[2],
        aim_floor: a[3],
        aim_settle: a[4],
        fire_cone: a[5],
        turn_smooth: a[6],
        turn_rate: a[7],
        max_range: a[8],
        burst_range: a[9],
        burst: a[10],
        burst_pause: a[11],
    }
}

fn view_out(v: View) -> PackedFloat64Array {
    PackedFloat64Array::from(&[v.yaw, v.pitch][..])
}

/// One brain's native aim and fight step.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimBotMind {
    aim: Aim,
    brain_key: i64,
    /// Per target id: when sight was last asked, the answer and the world's sight stamp then.
    sight: HashMap<i64, (f64, bool, i64)>,
    /// Sight LOD (`set_sight_lod`): beyond `far` m (0 = off) a "not seen" is held `far_reuse` s.
    far: f64,
    far_reuse: f64,
    found: Found,
    /// The tick's bodies handed over on the main thread (`hold_senses`) for `perceive_held`.
    held: Option<Arc<Packed>>,
    /// The query this mind asks a [`GSSimSightRay`] with, when that is its `los`.
    query: SightQuery,
    /// GSBotTricks' state, kept here between steps ([`Self::tricks_step`]).
    trick: gs_sim_brain::tricks::Resident,
    unstick: gs_sim_brain::unstick::Unstick,
    /// GSBotBrain's glance, kept here between steers ([`Self::steer_step`]).
    glance: gs_sim_brain::act::Glance,
}

impl GSSimBotMind {
    /// The cached answer for `id`, if it is still good at `now` under `stamp`.
    fn sight_held(&self, id: i64, now: f64, stamp: i64) -> Option<bool> {
        self.sight_held_far(id, now, stamp, 0.0)
    }

    /// [`Self::sight_held`] with the sight LOD: a "not seen" is also good for `far_reuse` s
    /// (never a "seen"; 0 or less holds nothing longer).
    fn sight_held_far(&self, id: i64, now: f64, stamp: i64, far_reuse: f64) -> Option<bool> {
        match self.sight.get(&id) {
            Some(&(at, seen, st)) if st == stamp && now >= at => {
                if now - at < SIGHT_REUSE || (!seen && now - at < far_reuse) {
                    Some(seen)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

fn gv(v: V3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

#[godot_api]
impl IRefCounted for GSSimBotMind {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            aim: Aim::new(0),
            brain_key: 0,
            sight: HashMap::new(),
            far: 0.0,
            far_reuse: 0.0,
            found: Found::default(),
            held: None,
            query: SightQuery::default(),
            trick: gs_sim_brain::tricks::Resident::default(),
            unstick: gs_sim_brain::unstick::Unstick::default(),
            glance: gs_sim_brain::act::Glance::default(),
        }
    }
}

#[godot_api]
impl GSSimBotMind {
    /// The aim's stream key (`stream(&"gs_bot_aim")`) and the brain's (`stream(&"gs_bot")`).
    #[func]
    fn set_keys(&mut self, aim_key: i64, brain_key: i64) {
        self.aim.rng.key = aim_key;
        self.brain_key = brain_key;
    }

    /// The private state: err_mag0, err_angle, err_spin, over (2), vel_est (3), prev_gap, flinch,
    /// want (2), want_prev (2), in_burst, burst_until, pause_until, the aim stream's counter.
    #[func]
    fn state(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(&self.aim.to_array()[..])
    }

    /// The inverse of [`Self::state`].
    #[func]
    fn load(&mut self, state: PackedFloat64Array) {
        self.aim.load(state.as_slice());
    }

    /// `engage` past its continue check; returns `react_at`.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn engage(
        &mut self,
        resumed: bool,
        now: f64,
        off: f64,
        reaction: f64,
        react_jitter: f64,
        notice: f64,
        aim_error: f64,
    ) -> f64 {
        self.aim
            .engage(resumed, now, off, reaction, react_jitter, notice, aim_error)
    }

    #[func]
    fn release(&mut self) {
        self.aim.release();
    }

    #[func]
    fn forget(&mut self) {
        self.aim.forget();
    }

    #[func]
    fn hurt(&mut self, flinch: f64) {
        self.aim.hurt(flinch);
    }

    #[func]
    fn error_deg(&self, now: f64, engaged_at: f64, aim_floor: f64, aim_settle: f64) -> f64 {
        self.aim.error_deg(now, engaged_at, aim_floor, aim_settle)
    }

    #[func]
    fn on_aim(&self, yaw: f64, pitch: f64, fire_cone: f64) -> bool {
        self.aim.on_aim(&View { yaw, pitch }, fire_cone)
    }

    /// `turn_to`; returns the new [yaw, pitch].
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn turn(
        &mut self,
        yaw: f64,
        pitch: f64,
        want: Vector2,
        dt: f64,
        prediction: f64,
        turn_smooth: f64,
        turn_rate: f64,
    ) -> PackedFloat64Array {
        let mut v = View { yaw, pitch };
        self.aim.turn_to(
            &mut v,
            V2::new(want.x, want.y),
            dt,
            prediction,
            turn_smooth,
            turn_rate,
        );
        view_out(v)
    }

    /// `look_at_point`; returns the new [yaw, pitch].
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn look(
        &mut self,
        yaw: f64,
        pitch: f64,
        eye: Vector3,
        point: Vector3,
        dt: f64,
        prediction: f64,
        turn_smooth: f64,
        turn_rate: f64,
    ) -> PackedFloat64Array {
        let mut v = View { yaw, pitch };
        self.aim.look(
            &mut v,
            v3(eye),
            v3(point),
            dt,
            prediction,
            turn_smooth,
            turn_rate,
        );
        view_out(v)
    }

    /// `track` with no aim solver. `io`: yaw, pitch, engaged_at, now, dt, projectile speed, then
    /// the aim profile (prediction .. burst_pause, as [`Self::fight`]). Returns [yaw, pitch].
    #[func]
    fn track(
        &mut self,
        eye: Vector3,
        point: Vector3,
        velocity: Vector3,
        io: PackedFloat64Array,
    ) -> PackedFloat64Array {
        let a = io.as_slice();
        if a.len() < 18 {
            godot_error!("GSSimBotMind.track: io holds {} of 18 values", a.len());
            return PackedFloat64Array::new();
        }
        let p = aim_profile(&a[6..18]);
        let mut v = View {
            yaw: a[0],
            pitch: a[1],
        };
        self.aim.track(
            &mut v,
            v3(eye),
            v3(point),
            v3(velocity),
            a[3],
            a[4],
            a[5],
            a[2],
            &p,
        );
        view_out(v)
    }

    /// `trigger`. `io`: yaw, pitch, react_at, then the aim profile.
    #[func]
    fn trigger(&mut self, now: f64, distance: f64, io: PackedFloat64Array) -> bool {
        let a = io.as_slice();
        if a.len() < 15 {
            godot_error!("GSSimBotMind.trigger: io holds {} of 15 values", a.len());
            return false;
        }
        let p = aim_profile(&a[3..15]);
        self.aim.trigger(
            &View {
                yaw: a[0],
                pitch: a[1],
            },
            now,
            distance,
            a[2],
            &p,
        )
    }

    /// `note_shot`'s error: degrees between the view and `point` from `eye`.
    #[func]
    fn shot_error(&self, yaw: f64, pitch: f64, eye: Vector3, point: Vector3) -> f64 {
        rad_to_deg(direction(yaw, pitch).angle_to(v3(point) - v3(eye)) as f64)
    }

    /// GSBotBrain's whole fight step (see the `io` layout in the source). Returns yaw, pitch,
    /// first_shot_at, first_shot_error, the brain stream's counter, fire, crouch, jump, move (2),
    /// scene (0 fight, 1 close, 2 flee), retreat, strafe, strafe_until, strafe_eased.
    #[func]
    fn fight(
        &mut self,
        eye: Vector3,
        point: Vector3,
        velocity: Vector3,
        me_pos: Vector3,
        c_pos: Vector3,
        io: PackedFloat64Array,
    ) -> PackedFloat64Array {
        let a = io.as_slice();
        if a.len() < io::LEN {
            godot_error!(
                "GSSimBotMind.fight: io holds {} of {} values",
                a.len(),
                io::LEN
            );
            return PackedFloat64Array::new();
        }
        let pr = &a[io::PROFILE..io::PUSH];
        let p = FightProfile {
            aim: aim_profile(&pr[..12]),
            range: pr[12],
            strafe: pr[13],
            strafe_jitter: pr[14],
            strafe_time: pr[15],
            jump: pr[16],
        };
        let i = FightIn {
            eye: v3(eye),
            point: v3(point),
            velocity: v3(velocity),
            me_pos: v3(me_pos),
            c_pos: v3(c_pos),
            now: a[io::NOW],
            dt: a[io::DT],
            projectile_speed: a[io::SPEED],
            health: a[io::HEALTH],
            react_at: a[io::REACT_AT],
            engaged_at: a[io::ENGAGED_AT],
            roll_stop: a[io::ROLL_STOP] != 0.0,
            roll_crouch: a[io::ROLL_CROUCH] != 0.0,
            roll_cover: a[io::ROLL_COVER] != 0.0,
            hesitate_until: a[io::HESITATE_UNTIL],
            push: a[io::PUSH] != 0.0,
        };
        let mut view = View {
            yaw: a[io::YAW],
            pitch: a[io::PITCH],
        };
        let mut first = FirstShot {
            at: a[io::FIRST_SHOT_AT],
            error: a[io::FIRST_SHOT_ERROR],
        };
        let mut strafe = Strafe {
            side: a[io::STRAFE],
            until: a[io::STRAFE_UNTIL],
            eased: a[io::STRAFE_EASED],
        };
        let mut rng = Stream::new(self.brain_key, a[io::COUNTER] as i64);
        let o = fight(
            &mut self.aim,
            &mut view,
            &mut first,
            &mut strafe,
            &mut rng,
            &i,
            &p,
        );
        let b = |x: bool| if x { 1.0 } else { 0.0 };
        PackedFloat64Array::from(
            &[
                view.yaw,
                view.pitch,
                first.at,
                first.error,
                rng.counter as f64,
                b(o.fire),
                b(o.crouch),
                b(o.jump),
                o.move_.x as f64,
                o.move_.y as f64,
                o.scene as f64,
                b(o.retreat),
                strafe.side,
                strafe.until,
                strafe.eased,
            ][..],
        )
    }

    /// One `GSBotTricks.apply` over the row `io` (layout `gs_sim_brain::tricks::io`, length
    /// 52) on the brain's stream; returns the row stepped, or an empty array for a short row.
    /// Stage 1 out asks the caller for the wall probe and a call with stage 2 to finish.
    #[func]
    fn tricks(&self, io: PackedFloat64Array) -> PackedFloat64Array {
        let mut row = io.to_vec();
        if !gs_sim_brain::tricks::tricks(&mut row, self.brain_key) {
            return PackedFloat64Array::new();
        }
        PackedFloat64Array::from(&row[..])
    }

    /// One `GSBotTricks.apply` on the state this mind keeps (`tricks_load` hands it over,
    /// `tricks_state` gives it back): returns the buttons packed (`gs_sim_brain::tricks::packed`:
    /// jump 1, crouch 2, air_jump 4, probe 8, move changed 16, counts 4 bits each from bit 8,
    /// the draws taken from bit 32), or -1 when `profile` is short. `bits`: the scene code (0
    /// other, 1 close, 2 chase, 3 flee, 4 roam, 5 cover), grounded 8, wall_running 16,
    /// air_jump_ready 32, a world to probe 64, the intent's jump 128, crouch 256, air_jump 512.
    /// `profile`: jump, trick_chain, cover_crouch, trick_delay, air_jump, wall_run, slide and
    /// the tricks' slide_speed. On probe 8, run the wall probe along `tricks_heading` and finish
    /// with `tricks_wall`.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn tricks_step(
        &mut self,
        now: f64,
        dt: f64,
        up: f64,
        yaw: f64,
        out_yaw: f64,
        health: f64,
        velocity: Vector3,
        move_: Vector2,
        counter: i64,
        bits: i64,
        profile: PackedFloat64Array,
    ) -> i64 {
        let pr = profile.as_slice();
        if pr.len() < 8 {
            godot_error!(
                "GSSimBotMind.tricks_step: profile holds {} of 8 values",
                pr.len()
            );
            return -1;
        }
        let mut p = [0.0; 8];
        p.copy_from_slice(&pr[..8]);
        let input = gs_sim_brain::tricks::Input {
            now,
            dt,
            up,
            yaw,
            out_yaw,
            health,
            vel_x: velocity.x as f64,
            vel_z: velocity.z as f64,
            move_x: move_.x as f64,
            move_y: move_.y as f64,
            counter,
            bits,
            profile: p,
        };
        self.trick.step(&input, self.brain_key)
    }

    /// The heading the wall probe a `tricks_step` asked for looks along.
    #[func]
    fn tricks_heading(&self) -> Vector2 {
        let (x, y) = self.trick.heading();
        Vector2::new(x as f32, y as f32)
    }

    /// The rest of a `tricks_step` that asked for the probe, with the wall's normal (zero: none);
    /// returns the buttons packed as `tricks_step` does.
    #[func]
    fn tricks_wall(&mut self, wall: Vector3) -> i64 {
        self.trick.resume(
            [wall.x as f64, wall.y as f64, wall.z as f64],
            self.brain_key,
        )
    }

    /// The intent's move after the last tricks step (read it when the step's bit 16 is set).
    #[func]
    fn tricks_move(&self) -> Vector2 {
        let (x, y) = self.trick.move_xy();
        Vector2::new(x as f32, y as f32)
    }

    /// GSBotTricks.reset on the kept state.
    #[func]
    fn tricks_reset(&mut self) {
        self.trick.reset();
    }

    /// The kept state: phase, at, until, look_at, quiet_until, wall x y z, chain, duck_until,
    /// duck_next, air_next, was_crouch (the first 13 slots of `tricks`' row).
    #[func]
    fn tricks_state(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(self.trick.state())
    }

    /// Takes the state over in `tricks_state`'s layout; false (and keeps its own) when short.
    #[func]
    fn tricks_load(&mut self, state: PackedFloat64Array) -> bool {
        self.trick.load(state.as_slice())
    }

    /// One `GSBotUnstick.step` on the state this mind keeps (`unstick_load` hands it over,
    /// `unstick_state` gives it back), up to the nudge's world query: returns
    /// `gs_sim_brain::unstick`'s bits (jump 1, crouch 2, replan 4, stranded 8, strafe 16, strafe
    /// left 32, a window ended stuck 64, wants the nudge 128) with the level from bit 8.
    /// `can_nudge`: the brain has a world to ask; `after`: the stranded interval (0: never).
    #[func]
    fn unstick_step(
        &mut self,
        position: Vector3,
        pushing: bool,
        now: f64,
        can_nudge: bool,
        after: f64,
    ) -> i64 {
        let p = gs_sim_brain::num::Vec3::new(position.x, position.y, position.z);
        self.unstick.step(p, pushing, now, can_nudge, after)
    }

    /// GSBotUnstick.reset on the kept state (an infinite `position` waits for the next step).
    #[func]
    fn unstick_reset(&mut self, position: Vector3, now: f64) {
        let p = gs_sim_brain::num::Vec3::new(position.x, position.y, position.z);
        self.unstick.reset(p, now);
    }

    /// The kept state: level, stuck_since, anchor x y z, anchor_at, pushed, seen, side.
    #[func]
    fn unstick_state(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(&self.unstick.state()[..])
    }

    /// Takes the state over in `unstick_state`'s layout; false (and keeps its own) when short.
    #[func]
    fn unstick_load(&mut self, state: PackedFloat64Array) -> bool {
        self.unstick.load(state.as_slice())
    }

    /// One roaming steer (`GSBotBrain._walk`, `_hold` or `_drift` with `_set_move`) over the row
    /// `io` (layout `gs_sim_brain::act::io`) on this mind's aim and the brain's stream; returns
    /// the row stepped (view, glance, counter, legs), or an empty array for a short row or an
    /// unknown mode. No engine calls, so it runs on a worker thread.
    #[func]
    fn steer(&mut self, io: PackedFloat64Array) -> PackedFloat64Array {
        let mut row = io.to_vec();
        if !gs_sim_brain::act::steer(&mut self.aim, &mut row, self.brain_key) {
            return PackedFloat64Array::new();
        }
        PackedFloat64Array::from(&row[..])
    }

    /// One roaming steer like [`Self::steer`], with the inputs as arguments and the glance kept
    /// in this mind (`glance_load` hands it over, `glance_state` gives it back). `mode` 0 walk
    /// along the flat direction `at`, 1 hold facing the point x z `at` (x NaN: none); a drift
    /// is [`Self::steer_drift`]. `prediction`, `turn_smooth`, `turn_rate`: the aim's profile.
    /// Returns yaw, pitch, the stream's counter and the legs x y, or an empty array for an
    /// unknown mode (nothing changed; the script then steers). No engine calls: worker-safe.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn steer_step(
        &mut self,
        mode: i64,
        yaw: f64,
        pitch: f64,
        eye: Vector3,
        at: Vector2,
        now: f64,
        dt: f64,
        id: i64,
        glance: f64,
        counter: i64,
        prediction: f64,
        turn_smooth: f64,
        turn_rate: f64,
    ) -> PackedFloat64Array {
        if !(0..=1).contains(&mode) {
            return PackedFloat64Array::new();
        }
        let inp = gs_sim_brain::act::Input {
            mode: mode as f64,
            eye: V3::new(eye.x, eye.y, eye.z),
            at: (at.x as f64, at.y as f64),
            now,
            dt,
            id,
            glance,
            turn: [prediction, turn_smooth, turn_rate],
            rate: 0.0,
        };
        let mut v = View { yaw, pitch };
        let mut counter = counter;
        let Some(legs) = gs_sim_brain::act::step(
            &mut self.aim,
            &mut v,
            &mut self.glance,
            &mut counter,
            &inp,
            self.brain_key,
        ) else {
            return PackedFloat64Array::new();
        };
        PackedFloat64Array::from(
            &[v.yaw, v.pitch, counter as f64, legs.x as f64, legs.y as f64][..],
        )
    }

    /// `GSBotBrain._drift` at `rate` deg/s for `dt` s (the aim's profile as in
    /// [`Self::steer_step`]); returns the view, yaw and pitch.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn steer_drift(
        &mut self,
        yaw: f64,
        pitch: f64,
        rate: f64,
        dt: f64,
        prediction: f64,
        turn_smooth: f64,
        turn_rate: f64,
    ) -> PackedFloat64Array {
        let inp = gs_sim_brain::act::Input {
            mode: gs_sim_brain::act::mode::DRIFT,
            eye: V3::new(0.0, 0.0, 0.0),
            at: (0.0, 0.0),
            now: 0.0,
            dt,
            id: 0,
            glance: 0.0,
            turn: [prediction, turn_smooth, turn_rate],
            rate,
        };
        let mut v = View { yaw, pitch };
        let mut counter = 0;
        let mut g = self.glance;
        if gs_sim_brain::act::step(
            &mut self.aim,
            &mut v,
            &mut g,
            &mut counter,
            &inp,
            self.brain_key,
        )
        .is_none()
        {
            return PackedFloat64Array::new();
        }
        PackedFloat64Array::from(&[v.yaw, v.pitch][..])
    }

    /// The kept glance: next, until, off.
    #[func]
    fn glance_state(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(&self.glance.state()[..])
    }

    /// Takes the glance over in `glance_state`'s layout; false (and keeps its own) when short.
    #[func]
    fn glance_load(&mut self, state: PackedFloat64Array) -> bool {
        self.glance.load(state.as_slice())
    }

    /// One `GSBotBrain._hear` over GSBotWorld's sound rows (`gs_sim_brain::hear::STRIDE` each,
    /// oldest first) on the brain's stream. A source's team comes from `senses`' bodies or, when
    /// `senses` is null, from the ones [`Self::hold_senses`] kept (so it runs on a worker thread).
    /// `io`: after_seq, now, here x, y, z, reach, my_id, my_team, eager, counter. Returns heard
    /// (0 or 1), x, y, z, chase (0 or 1), counter; empty when no bodies are at hand or `io` is
    /// short (the script then hears).
    #[func]
    fn hear(
        &self,
        senses: Option<Gd<GSSimBotSenses>>,
        rows: PackedFloat64Array,
        io: PackedFloat64Array,
    ) -> PackedFloat64Array {
        let packed = match senses {
            Some(s) => s.bind().share(),
            None => match &self.held {
                Some(h) => Arc::clone(h),
                None => return PackedFloat64Array::new(),
            },
        };
        let a = io.as_slice();
        if a.len() < 10 {
            return PackedFloat64Array::new();
        }
        let inp = gs_sim_brain::hear::HearIn {
            after_seq: a[0] as i64,
            now: a[1],
            here: V3::new(a[2] as f32, a[3] as f32, a[4] as f32),
            reach: a[5],
            my_id: a[6] as i64,
            my_team: a[7] as i64,
            eager: a[8],
        };
        let table = &packed.table;
        let team_of = |id: i64| {
            table
                .ids
                .iter()
                .position(|&x| x == id)
                .map(|i| table.teams[i] as i64)
        };
        let mut rng = Stream::new(self.brain_key, a[9] as i64);
        let heard = gs_sim_brain::hear::hear(rows.as_slice(), &inp, team_of, &mut rng);
        let (h, p, c) = match heard {
            Some(x) => (1.0, x.pos, x.chase),
            None => (0.0, V3::new(0.0, 0.0, 0.0), false),
        };
        PackedFloat64Array::from(
            &[
                h,
                p.x as f64,
                p.y as f64,
                p.z as f64,
                if c { 1.0 } else { 0.0 },
                rng.counter as f64,
            ][..],
        )
    }

    /// The brain's sight cache: 1 or 0 for a good cached answer about `id`, -1 when sight must be asked.
    #[func]
    fn sight_cached(&self, id: i64, now: f64, stamp: i64) -> i64 {
        match self.sight_held(id, now, stamp) {
            Some(seen) => seen as i64,
            None => -1,
        }
    }

    /// [`Self::sight_cached`] under the sight LOD: `far_reuse` is how long a "not seen" about `id`
    /// is held (the caller measured the distance; 0 for a near body or the target).
    #[func]
    fn sight_cached_far(&self, id: i64, now: f64, stamp: i64, far_reuse: f64) -> i64 {
        match self.sight_held_far(id, now, stamp, far_reuse) {
            Some(seen) => seen as i64,
            None => -1,
        }
    }

    /// The sight LOD `perceive` applies (GSBotWorld.sight_far, sight_far_reuse); `far` 0 is off.
    #[func]
    fn set_sight_lod(&mut self, far: f64, far_reuse: f64) {
        self.far = far;
        self.far_reuse = far_reuse;
    }

    /// Files a sight answer about `id`.
    #[func]
    fn sight_put(&mut self, id: i64, now: f64, seen: bool, stamp: i64) {
        self.sight.insert(id, (now, seen, stamp));
    }

    /// GSBotBrain._perceive's choice over `senses`' packed bodies: the native scan, the current
    /// target kept while in sight, else the best score seen (`los(from, to) -> bool`, the world's
    /// line of sight, asked of the chest then the head, through the sight cache; a
    /// [`GSSimSightRay`]'s `clear` is asked natively, the same answer). `flags`: 1 the
    /// target is visible, 2 the bot sees through walls. Returns
    /// [chosen row or -1, 1 when the visible target was found dead], or nothing when `senses`
    /// has no eyes for its rows (the script then perceives).
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn perceive(
        &mut self,
        senses: Gd<GSSimBotSenses>,
        me_id: i64,
        me_team: i64,
        eye: Vector3,
        yaw: f64,
        pitch: f64,
        half_fov_deg: f64,
        sight: f64,
        target_id: i64,
        flags: i64,
        now: f64,
        stamp: i64,
        los: Callable,
    ) -> PackedInt32Array {
        let held = senses.bind().share();
        self.perceive_in(
            &held,
            me_id,
            me_team,
            eye,
            yaw,
            pitch,
            half_fov_deg,
            sight,
            target_id,
            flags,
            now,
            stamp,
            los,
        )
    }

    /// GSBotPickup's `_choose` over the world's drop table (`ids`, `at`, `rows` of slot, value,
    /// ammo): the row of the drop a quiet bot walks to, -1 for none, -2 when the columns do not
    /// match. `mine` is the loadout's dictionary slots as slot, value, ammo; `skip_ids` and
    /// `skip_until` the drops left alone. `los(eye, to)` is asked as the script asks it (same drops,
    /// same order), so the choice is the script's to the bit. Safe on a worker thread.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn pickup_choose(
        &self,
        ids: PackedInt64Array,
        at: PackedVector3Array,
        rows: PackedFloat64Array,
        mine: PackedFloat64Array,
        skip_ids: PackedInt64Array,
        skip_until: PackedFloat64Array,
        pos: Vector3,
        eye: Vector3,
        range: f64,
        through: bool,
        now: f64,
        los: Callable,
    ) -> i64 {
        use gs_sim_brain::pickup::{ChooseIn, STRIDE, Skip, choose};
        let n = ids.len();
        if at.len() != n || rows.len() < n * STRIDE || skip_until.len() < skip_ids.len() {
            return -2;
        }
        let points: Vec<V3> = at.as_slice().iter().map(|&v| v3(v)).collect();
        let inp = ChooseIn {
            pos: v3(pos),
            range,
            through,
            now,
        };
        let skip = Skip {
            ids: skip_ids.as_slice(),
            until: skip_until.as_slice(),
        };
        let eye = eye.to_variant();
        let got = choose(
            ids.as_slice(),
            &points,
            rows.as_slice(),
            mine.as_slice(),
            skip,
            &inp,
            |to| los.call(&[eye.clone(), gv(to).to_variant()]).booleanize(),
        );
        got.map_or(-1, |i| i as i64)
    }

    /// Whether this library's objects may be called on worker threads (one thread per object at a
    /// time): desktop and server builds bind with godot-rust's thread-safe storage; web builds are
    /// single-threaded and panic off the main thread, so their brains stay in script there.
    #[func]
    fn worker_safe() -> bool {
        cfg!(not(target_family = "wasm"))
    }

    /// Keeps a handle on `senses`' bodies as packed now, for [`Self::perceive_held`]. Call it on the
    /// main thread: the handle is what lets a brain perceive on a worker thread, where several
    /// brains must not bind the one senses object at once. A later repack leaves the handle whole.
    #[func]
    fn hold_senses(&mut self, senses: Gd<GSSimBotSenses>) {
        self.held = Some(senses.bind().share());
    }

    /// Drops the handle [`Self::hold_senses`] kept.
    #[func]
    fn drop_senses(&mut self) {
        self.held = None;
    }

    /// [`Self::perceive`] over the bodies [`Self::hold_senses`] kept (the same choice to the bit),
    /// safe on a worker thread; nothing when none is held (the script then perceives).
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn perceive_held(
        &mut self,
        me_id: i64,
        me_team: i64,
        eye: Vector3,
        yaw: f64,
        pitch: f64,
        half_fov_deg: f64,
        sight: f64,
        target_id: i64,
        flags: i64,
        now: f64,
        stamp: i64,
        los: Callable,
    ) -> PackedInt32Array {
        let Some(held) = self.held.clone() else {
            return PackedInt32Array::new();
        };
        self.perceive_in(
            &held,
            me_id,
            me_team,
            eye,
            yaw,
            pitch,
            half_fov_deg,
            sight,
            target_id,
            flags,
            now,
            stamp,
            los,
        )
    }
}

impl GSSimBotMind {
    #[allow(clippy::too_many_arguments)]
    fn perceive_in(
        &mut self,
        held: &Packed,
        me_id: i64,
        me_team: i64,
        eye: Vector3,
        yaw: f64,
        pitch: f64,
        half_fov_deg: f64,
        sight: f64,
        target_id: i64,
        flags: i64,
        now: f64,
        stamp: i64,
        los: Callable,
    ) -> PackedInt32Array {
        let table = &held.table;
        if held.eyes.len() != table.len() || (self.far > 0.0 && held.positions.len() != table.len())
        {
            return PackedInt32Array::new();
        }
        let (far, far_reuse) = (self.far, self.far_reuse);
        let visible = flags & 1 != 0;
        let through = flags & 2 != 0;
        let eye3 = v3(eye);
        let q = Query {
            me_id,
            me_team: me_team as i32,
            eye: eye3,
            forward: direction(yaw, pitch),
            half_fov_deg,
            sight,
            close_notice: CLOSE_NOTICE,
            target_id,
            visible,
        };
        let mut found = std::mem::take(&mut self.found);
        candidates(table, &q, &mut found);
        // A dead visible target is forgotten before the choice (the script's _forget_target).
        let (visible, target_id) = if found.target_dead {
            (false, -1)
        } else {
            (visible, target_id)
        };
        // The world's sight as a native ray: asked here, not through a call into script.
        let direct = match (los.object(), los.method_name()) {
            (Some(obj), Some(name)) if name == "clear" && los.get_bound_arguments_count() == 0 => {
                obj.try_cast::<GSSimSightRay>()
                    .ok()
                    .map(|ray| ray.bind().conf())
            }
            _ => None,
        };
        let sees = |mind: &mut Self, row: usize| -> bool {
            let id = table.ids[row];
            // Sight LOD: a far body (never the target) keeps a "not seen" longer.
            let mut reuse = 0.0;
            if far > 0.0 && id != target_id {
                let p = held.positions[row];
                let (dx, dy, dz) = (eye3.x - p.x, eye3.y - p.y, eye3.z - p.z);
                if ((dx * dx + dy * dy + dz * dz) as f64) > far * far {
                    reuse = far_reuse;
                }
            }
            if let Some(seen) = mind.sight_held_far(id, now, stamp, reuse) {
                return seen;
            }
            let query = &mut mind.query;
            let mut ask = |to: V3| match &direct {
                Some(conf) => conf.clear(query, eye, gv(to)),
                None => los
                    .call(&[eye.to_variant(), gv(to).to_variant()])
                    .booleanize(),
            };
            let seen = ask(table.points[row]) || ask(held.eyes[row]);
            mind.sight.insert(id, (now, seen, stamp));
            seen
        };
        let mut chosen: i32 = -1;
        let mut current = false;
        if visible {
            for i in 0..found.index.len() {
                let row = found.index[i] as usize;
                if table.ids[row] == target_id {
                    if through || sees(self, row) {
                        chosen = found.index[i];
                        current = true;
                    }
                    found.scores[i] = f32::INFINITY;
                    break;
                }
            }
        }
        if !current {
            loop {
                let mut bi: Option<usize> = None;
                let mut best = f64::INFINITY;
                for (i, s) in found.scores.iter().enumerate() {
                    if (*s as f64) < best {
                        best = *s as f64;
                        bi = Some(i);
                    }
                }
                let Some(bi) = bi else { break };
                let row = found.index[bi] as usize;
                if through || sees(self, row) {
                    chosen = found.index[bi];
                    break;
                }
                found.scores[bi] = f32::INFINITY;
            }
        }
        let dead = found.target_dead as i32;
        self.found = found;
        PackedInt32Array::from(&[chosen, dead][..])
    }
}
