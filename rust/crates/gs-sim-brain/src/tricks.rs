//! GSBotTricks' `apply`: slides, hops, wall runs, cover crouches and air jumps, in the script's
//! order and precision. One step runs over a flat `f64` row ([`io`]) the script packs and unpacks.
//!
//! The one thing it cannot do is the wall probe (`GSBotWorld.wall_ahead`, a physics query). When
//! a look wants one, the step stops with [`io::STAGE`] 1 and the heading in [`io::HEADING`]; the
//! script runs `_find_wall` and calls again with [`io::STAGE`] 2 and the wall in [`io::WALL_IN`],
//! and the step carries on from where it stopped. The random draws happen in the same order either
//! way, so the bot makes the script's choices to the bit.

use crate::num::{Stream, Vec2, deg_to_rad};

/// The row's layout. State slots are carried between steps; the rest are filled each step.
pub mod io {
    pub const PHASE: usize = 0;
    pub const AT: usize = 1;
    pub const UNTIL: usize = 2;
    pub const LOOK_AT: usize = 3;
    pub const QUIET_UNTIL: usize = 4;
    /// The wall's normal, x y z (zero: none).
    pub const WALL: usize = 5;
    pub const CHAIN: usize = 8;
    pub const DUCK_UNTIL: usize = 9;
    pub const DUCK_NEXT: usize = 10;
    pub const AIR_NEXT: usize = 11;
    pub const WAS_CROUCH: usize = 12;
    /// The brain's random stream's counter.
    pub const COUNTER: usize = 13;
    pub const NOW: usize = 14;
    pub const DT: usize = 15;
    /// The brain's scenario: [`super::scene`].
    pub const SCENE: usize = 16;
    pub const UP: usize = 17;
    /// The `yaw` argument (the aim's yaw), which a heading without speed is taken from.
    pub const YAW: usize = 18;
    pub const VEL_X: usize = 19;
    pub const VEL_Z: usize = 20;
    pub const GROUNDED: usize = 21;
    pub const WALL_RUNNING: usize = 22;
    pub const AIR_JUMP_READY: usize = 23;
    pub const HEALTH: usize = 24;
    /// 1 when there is a world to probe for walls.
    pub const CAN_PROBE: usize = 25;
    /// Profile: jump, trick_chain, cover_crouch, trick_delay, air_jump, wall_run, slide; then
    /// the tricks' own slide_speed.
    pub const P_JUMP: usize = 26;
    pub const P_TRICK_CHAIN: usize = 27;
    pub const P_COVER_CROUCH: usize = 28;
    pub const P_TRICK_DELAY: usize = 29;
    pub const P_AIR_JUMP: usize = 30;
    pub const P_WALL_RUN: usize = 31;
    pub const P_SLIDE: usize = 32;
    pub const SLIDE_SPEED: usize = 33;
    /// The intent, in and out: move x y, jump, crouch, air_jump, and its yaw (read only).
    pub const MOVE_X: usize = 34;
    pub const MOVE_Y: usize = 35;
    pub const JUMP: usize = 36;
    pub const CROUCH: usize = 37;
    pub const AIR_JUMPED: usize = 38;
    pub const OUT_YAW: usize = 39;
    /// In: 0 a fresh step, 2 the rest of one after the wall probe. Out: 0 done, 1 probe wanted.
    pub const STAGE: usize = 40;
    /// The probe's answer (stage 2): the wall's normal, zero for none.
    pub const WALL_IN: usize = 41;
    /// The heading the probe looks along (stage 1 out), x and y.
    pub const HEADING: usize = 44;
    /// What was done, added to the tricks' counts: slide, hop, wall, wall_jump, crouch, air_jump.
    pub const COUNTS: usize = 46;
    pub const LEN: usize = 52;
}

/// Scenario codes for [`io::SCENE`] (any other scenario is 0).
pub mod scene {
    pub const OTHER: f64 = 0.0;
    pub const CLOSE: f64 = 1.0;
    pub const CHASE: f64 = 2.0;
    pub const FLEE: f64 = 3.0;
    pub const ROAM: f64 = 4.0;
    pub const COVER: f64 = 5.0;
}

/// GSBotTricks.Phase.
mod phase {
    pub const IDLE: f64 = 0.0;
    pub const SLIDE_WAIT: f64 = 1.0;
    pub const SLIDING: f64 = 2.0;
    pub const AIR: f64 = 3.0;
    pub const RIDE: f64 = 4.0;
}

const LOOK_EVERY: f64 = 0.35;
const COOLDOWN: f64 = 1.2;

const C_SLIDE: usize = 0;
const C_HOP: usize = 1;
const C_WALL: usize = 2;
const C_WALL_JUMP: usize = 3;
const C_CROUCH: usize = 4;
const C_AIR_JUMP: usize = 5;

/// `GSBotTricks.WEIGHTS` for a scenario, or `None` when it has no weight.
fn weight(scene: f64) -> Option<f64> {
    if scene == scene::CLOSE || scene == scene::CHASE {
        Some(1.0)
    } else if scene == scene::FLEE {
        Some(1.4)
    } else if scene == scene::ROAM {
        Some(0.2)
    } else {
        None
    }
}

fn flag(v: bool) -> f64 {
    if v { 1.0 } else { 0.0 }
}

struct Step<'a> {
    io: &'a mut [f64],
    rng: Stream,
}

impl Step<'_> {
    fn on(&self, i: usize) -> bool {
        self.io[i] != 0.0
    }

    fn set(&mut self, i: usize, v: bool) {
        self.io[i] = flag(v);
    }

    fn count(&mut self, c: usize) {
        self.io[io::COUNTS + c] += 1.0;
    }

    fn unit(&mut self) -> f64 {
        self.rng.next_unit()
    }

    fn flat_speed(&self) -> f64 {
        f64::from(Vec2::of(self.io[io::VEL_X], self.io[io::VEL_Z]).length())
    }

    fn wall(&self) -> Vec2 {
        // Only x and z are read (as `Vector2(_wall.x, _wall.z)`).
        Vec2::of(self.io[io::WALL], self.io[io::WALL + 2])
    }

    fn wall_is_zero(&self) -> bool {
        self.io[io::WALL] == 0.0 && self.io[io::WALL + 1] == 0.0 && self.io[io::WALL + 2] == 0.0
    }

    fn end(&mut self, now: f64) {
        self.io[io::PHASE] = phase::IDLE;
        self.io[io::WALL..io::WALL + 3].fill(0.0);
        self.io[io::QUIET_UNTIL] = now + COOLDOWN;
    }

    fn ground_tricks(&mut self, now: f64) {
        let sc = self.io[io::SCENE];
        let grounded = self.on(io::GROUNDED);
        if grounded
            && self.io[io::DT] > 0.0
            && (sc == scene::CLOSE || sc == scene::CHASE)
            && !self.on(io::JUMP)
        {
            let speed = self.flat_speed();
            if speed > 4.0 {
                let chance =
                    self.io[io::P_JUMP] * 2.5 * self.io[io::P_TRICK_CHAIN] * self.io[io::DT];
                if self.unit() < chance {
                    self.set(io::JUMP, true);
                    self.count(C_HOP);
                }
            }
        }
        if now < self.io[io::DUCK_UNTIL] {
            self.set(io::CROUCH, true);
            return;
        }
        if sc != scene::COVER && sc != scene::FLEE {
            return;
        }
        if now >= self.io[io::DUCK_NEXT] && grounded && self.flat_speed() < 3.5 {
            self.io[io::DUCK_NEXT] = now + 2.0 + 3.0 * self.unit();
            if self.unit() < self.io[io::P_COVER_CROUCH] {
                let a = now + self.io[io::P_TRICK_DELAY] * self.unit() + 0.6;
                self.io[io::DUCK_UNTIL] = a + 1.2 * self.unit();
                self.count(C_CROUCH);
                self.set(io::CROUCH, true);
            }
        }
    }

    fn air_jump(&mut self, now: f64) {
        if !self.on(io::AIR_JUMP_READY) || now < self.io[io::AIR_NEXT] {
            return;
        }
        let wants = (self.io[io::SCENE] == scene::FLEE && self.io[io::HEALTH] < 0.5)
            || self.io[io::UP] > 2.4;
        if !wants {
            return;
        }
        self.io[io::AIR_NEXT] = now + 1.0;
        if self.unit() < self.io[io::P_AIR_JUMP] {
            self.set(io::AIR_JUMPED, true);
            self.io[io::AIR_NEXT] = now + 5.0;
            self.count(C_AIR_JUMP);
        }
    }

    fn start_slide(&mut self, now: f64) {
        self.io[io::PHASE] = phase::SLIDE_WAIT;
        self.io[io::AT] = now + self.io[io::P_TRICK_DELAY] * (0.4 + 0.8 * self.unit());
    }

    /// The rest of `_look` once the wall is known (zero: none).
    fn look_with(&mut self, wall: [f64; 3], now: f64) {
        let w = weight(self.io[io::SCENE]).unwrap_or(0.0);
        let has_wall = wall != [0.0; 3];
        if has_wall && self.unit() < self.io[io::P_WALL_RUN] * w {
            self.io[io::WALL..io::WALL + 3].copy_from_slice(&wall);
            self.set(io::CHAIN, true);
            self.start_slide(now);
            return;
        }
        if self.unit() < self.io[io::P_SLIDE] * w {
            self.io[io::WALL..io::WALL + 3].fill(0.0);
            let chain = self.unit() < self.io[io::P_TRICK_CHAIN];
            self.set(io::CHAIN, chain);
            self.start_slide(now);
        }
    }

    fn heading_of(&self, yaw: f64) -> Vec2 {
        let y = deg_to_rad(yaw);
        let f = Vec2::of(-y.sin(), -y.cos());
        let r = Vec2::of(y.cos(), -y.sin());
        let w = f.scale(self.io[io::MOVE_Y]) + r.scale(self.io[io::MOVE_X]);
        if f64::from(w.length()) > 0.01 {
            w.normalized()
        } else {
            f
        }
    }

    fn steer_along(&mut self, heading: Vec2) {
        let n = self.wall().normalized();
        let mut along = Vec2::new(-n.y, n.x);
        if along.dot(heading) < 0.0 {
            along = Vec2::new(-along.x, -along.y);
        }
        let want = (along - n.scale(0.35)).normalized();
        let y = deg_to_rad(self.io[io::OUT_YAW]);
        let f = Vec2::of(-y.sin(), -y.cos());
        let r = Vec2::of(y.cos(), -y.sin());
        let m = Vec2::new(want.dot(r), want.dot(f)).limit_length(1.0);
        self.io[io::MOVE_X] = f64::from(m.x);
        self.io[io::MOVE_Y] = f64::from(m.y);
    }

    fn phase_step(&mut self, heading: Vec2, speed: f64, now: f64) {
        let grounded = self.on(io::GROUNDED);
        let ph = self.io[io::PHASE];
        if ph == phase::SLIDE_WAIT {
            if now < self.io[io::AT] {
                return;
            }
            if !grounded || speed < self.io[io::SLIDE_SPEED] || self.on(io::WAS_CROUCH) {
                self.end(now);
                return;
            }
            self.set(io::CROUCH, true);
            self.count(C_SLIDE);
            self.io[io::UNTIL] = now + (0.3 + 0.25 * self.unit());
            self.io[io::PHASE] = phase::SLIDING;
        } else if ph == phase::SLIDING {
            if !grounded {
                self.io[io::PHASE] = phase::AIR;
                self.io[io::UNTIL] = now + 1.0;
                return;
            }
            if now < self.io[io::UNTIL] {
                self.set(io::CROUCH, true);
                return;
            }
            if self.on(io::CHAIN) {
                self.set(io::JUMP, true);
                self.count(C_HOP);
                self.io[io::PHASE] = phase::AIR;
                self.io[io::UNTIL] = now + 1.0;
            } else {
                self.end(now);
            }
        } else if ph == phase::AIR {
            if self.wall_is_zero() {
                if grounded || now > self.io[io::UNTIL] {
                    self.end(now);
                }
                return;
            }
            if self.on(io::WALL_RUNNING) {
                self.count(C_WALL);
                self.io[io::UNTIL] = now + 0.5 + 0.7 * self.unit();
                self.io[io::PHASE] = phase::RIDE;
                return;
            }
            if grounded || now > self.io[io::UNTIL] {
                self.end(now);
                return;
            }
            self.steer_along(heading);
        } else if ph == phase::RIDE {
            if !self.on(io::WALL_RUNNING) || now > self.io[io::UNTIL] + 1.5 {
                self.end(now);
                return;
            }
            self.steer_along(heading);
            if now >= self.io[io::UNTIL] {
                let jump = self.unit() < self.io[io::P_TRICK_CHAIN];
                self.set(io::JUMP, jump);
                if jump {
                    self.count(C_WALL_JUMP);
                }
                self.end(now);
            }
        }
    }

    /// `apply` from its start; false when it stopped for the wall probe.
    fn fresh(&mut self, now: f64) -> bool {
        let flat = Vec2::of(self.io[io::VEL_X], self.io[io::VEL_Z]);
        let speed = f64::from(flat.length());
        let heading = if speed > 1.0 {
            flat.div_scalar(speed)
        } else {
            self.heading_of(self.io[io::YAW])
        };
        if self.io[io::PHASE] != phase::IDLE {
            self.phase_step(heading, speed, now);
            return true;
        }
        self.ground_tricks(now);
        if now >= self.io[io::LOOK_AT] && now >= self.io[io::QUIET_UNTIL] && !self.on(io::CROUCH) {
            self.io[io::LOOK_AT] = now + LOOK_EVERY * (0.6 + 0.8 * self.unit());
            // `_look`: nothing on the air, too slow or in a scenario without a weight.
            if self.on(io::GROUNDED)
                && speed >= self.io[io::SLIDE_SPEED]
                && weight(self.io[io::SCENE]).is_some()
            {
                if self.on(io::CAN_PROBE) && self.io[io::P_WALL_RUN] > 0.0 {
                    self.io[io::HEADING] = f64::from(heading.x);
                    self.io[io::HEADING + 1] = f64::from(heading.y);
                    return false;
                }
                self.look_with([0.0; 3], now);
            }
        }
        self.air_jump(now);
        true
    }
}

/// One `GSBotTricks.apply` over `io` with the brain's stream `key`. Returns false (and leaves
/// `io` alone) when the row is too short.
pub fn tricks(io: &mut [f64], key: i64) -> bool {
    if io.len() < io::LEN {
        return false;
    }
    let now = io[io::NOW];
    let resume = io[io::STAGE] == 2.0;
    let counter = io[io::COUNTER] as i64;
    if !resume {
        io[io::COUNTS..io::COUNTS + 6].fill(0.0);
    }
    let mut s = Step {
        io,
        rng: Stream::new(key, counter),
    };
    let done = if resume {
        let wall = [
            s.io[io::WALL_IN],
            s.io[io::WALL_IN + 1],
            s.io[io::WALL_IN + 2],
        ];
        s.look_with(wall, now);
        s.air_jump(now);
        true
    } else {
        s.fresh(now)
    };
    if done {
        // A key is pressed again only after it was let go: a held crouch is no second slide.
        s.io[io::WAS_CROUCH] = s.io[io::CROUCH];
    }
    s.io[io::STAGE] = if done { 0.0 } else { 1.0 };
    s.io[io::COUNTER] = s.rng.counter as f64;
    true
}

/// What [`Resident`]'s steps return, packed in one `i64` so only the intent's buttons cross.
pub mod packed {
    pub const JUMP: i64 = 1;
    pub const CROUCH: i64 = 2;
    pub const AIR_JUMP: i64 = 4;
    /// The step stopped for the wall probe: look along [`super::Resident::heading`] and resume.
    pub const PROBE: i64 = 8;
    /// The move changed: read [`super::Resident::move_xy`].
    pub const MOVED: i64 = 16;
    /// The counts (slide, hop, wall, wall_jump, crouch, air_jump), four bits each from here.
    pub const COUNTS_SHIFT: u32 = 8;
    /// How many draws the step took from the brain's stream, from here up.
    pub const COUNTER_SHIFT: u32 = 32;
    /// The input flags of [`super::Input::bits`]: the scene code in the low three bits, then these.
    pub const IN_GROUNDED: i64 = 8;
    pub const IN_WALL_RUNNING: i64 = 16;
    pub const IN_AIR_JUMP_READY: i64 = 32;
    pub const IN_CAN_PROBE: i64 = 64;
    pub const IN_JUMP: i64 = 128;
    pub const IN_CROUCH: i64 = 256;
    pub const IN_AIR_JUMP: i64 = 512;
}

/// How many state slots (`PHASE` to `WAS_CROUCH`) a bot's tricks carry between steps.
pub const STATE_LEN: usize = io::WAS_CROUCH + 1;

/// One step's inputs for [`Resident::step`]: what the row's per-step slots hold.
pub struct Input {
    pub now: f64,
    pub dt: f64,
    pub up: f64,
    pub yaw: f64,
    pub out_yaw: f64,
    pub health: f64,
    pub vel_x: f64,
    pub vel_z: f64,
    pub move_x: f64,
    pub move_y: f64,
    pub counter: i64,
    /// [`packed`]'s `IN_*` flags over the scene code.
    pub bits: i64,
    /// jump, trick_chain, cover_crouch, trick_delay, air_jump, wall_run, slide, slide_speed.
    pub profile: [f64; 8],
}

/// One bot's tricks with their state kept native between steps (GSSimBotMind holds one per
/// bot): a step takes the inputs and gives back the buttons packed ([`packed`]), the same
/// choices and draws as [`tricks`] over a row the script carried.
pub struct Resident {
    row: [f64; io::LEN],
    counter_in: i64,
}

impl Default for Resident {
    fn default() -> Self {
        let mut r = Resident {
            row: [0.0; io::LEN],
            counter_in: 0,
        };
        r.reset();
        r
    }
}

impl Resident {
    /// GSBotTricks.reset: idle, no wall, no duck.
    pub fn reset(&mut self) {
        self.row[io::PHASE] = phase::IDLE;
        self.row[io::WALL..io::WALL + 3].fill(0.0);
        self.row[io::DUCK_UNTIL] = f64::NEG_INFINITY;
    }

    /// The carried state, `PHASE` to `WAS_CROUCH`.
    pub fn state(&self) -> &[f64] {
        &self.row[..STATE_LEN]
    }

    /// Takes the carried state over (a script that held it hands it here); false when short.
    pub fn load(&mut self, state: &[f64]) -> bool {
        if state.len() < STATE_LEN {
            return false;
        }
        self.row[..STATE_LEN].copy_from_slice(&state[..STATE_LEN]);
        true
    }

    /// The heading a probe wanted by the last step looks along.
    pub fn heading(&self) -> (f64, f64) {
        (self.row[io::HEADING], self.row[io::HEADING + 1])
    }

    /// The intent's move after the last step.
    pub fn move_xy(&self) -> (f64, f64) {
        (self.row[io::MOVE_X], self.row[io::MOVE_Y])
    }

    /// One `apply` from its start on the brain's stream `key`.
    pub fn step(&mut self, i: &Input, key: i64) -> i64 {
        let r = &mut self.row;
        r[io::COUNTER] = i.counter as f64;
        r[io::NOW] = i.now;
        r[io::DT] = i.dt;
        r[io::SCENE] = (i.bits & 7) as f64;
        r[io::UP] = i.up;
        r[io::YAW] = i.yaw;
        r[io::VEL_X] = i.vel_x;
        r[io::VEL_Z] = i.vel_z;
        r[io::GROUNDED] = flag(i.bits & packed::IN_GROUNDED != 0);
        r[io::WALL_RUNNING] = flag(i.bits & packed::IN_WALL_RUNNING != 0);
        r[io::AIR_JUMP_READY] = flag(i.bits & packed::IN_AIR_JUMP_READY != 0);
        r[io::HEALTH] = i.health;
        r[io::CAN_PROBE] = flag(i.bits & packed::IN_CAN_PROBE != 0);
        r[io::P_JUMP..io::P_JUMP + 8].copy_from_slice(&i.profile);
        r[io::MOVE_X] = i.move_x;
        r[io::MOVE_Y] = i.move_y;
        r[io::JUMP] = flag(i.bits & packed::IN_JUMP != 0);
        r[io::CROUCH] = flag(i.bits & packed::IN_CROUCH != 0);
        r[io::AIR_JUMPED] = flag(i.bits & packed::IN_AIR_JUMP != 0);
        r[io::OUT_YAW] = i.out_yaw;
        r[io::STAGE] = 0.0;
        self.counter_in = i.counter;
        tricks(&mut self.row, key);
        self.pack(i.move_x, i.move_y)
    }

    /// The rest of a step that stopped for the probe, with the wall's normal (zero: none).
    pub fn resume(&mut self, wall: [f64; 3], key: i64) -> i64 {
        if self.row[io::STAGE] != 1.0 {
            return self.pack(self.row[io::MOVE_X], self.row[io::MOVE_Y]);
        }
        let (mx, my) = self.move_xy();
        self.row[io::STAGE] = 2.0;
        self.row[io::WALL_IN..io::WALL_IN + 3].copy_from_slice(&wall);
        tricks(&mut self.row, key);
        self.pack(mx, my)
    }

    fn pack(&self, move_x: f64, move_y: f64) -> i64 {
        let r = &self.row;
        if r[io::STAGE] == 1.0 {
            return packed::PROBE;
        }
        let mut out = 0;
        if r[io::JUMP] != 0.0 {
            out |= packed::JUMP;
        }
        if r[io::CROUCH] != 0.0 {
            out |= packed::CROUCH;
        }
        if r[io::AIR_JUMPED] != 0.0 {
            out |= packed::AIR_JUMP;
        }
        if r[io::MOVE_X] != move_x || r[io::MOVE_Y] != move_y {
            out |= packed::MOVED;
        }
        for c in 0..6 {
            let n = (r[io::COUNTS + c] as i64).clamp(0, 15);
            out |= n << (packed::COUNTS_SHIFT + 4 * c as u32);
        }
        out | ((r[io::COUNTER] as i64 - self.counter_in) << packed::COUNTER_SHIFT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num::mix;

    fn row() -> Vec<f64> {
        let mut r = vec![0.0; io::LEN];
        r[io::DUCK_UNTIL] = f64::NEG_INFINITY;
        r[io::P_JUMP] = 0.3;
        r[io::P_TRICK_CHAIN] = 0.5;
        r[io::P_COVER_CROUCH] = 0.5;
        r[io::P_TRICK_DELAY] = 0.2;
        r[io::P_AIR_JUMP] = 0.5;
        r[io::P_WALL_RUN] = 0.5;
        r[io::P_SLIDE] = 1.0;
        r[io::SLIDE_SPEED] = 6.5;
        r[io::GROUNDED] = 1.0;
        r
    }

    #[test]
    fn short_row_is_refused() {
        assert!(!tricks(&mut [0.0; 10], 1));
    }

    #[test]
    fn fast_run_asks_for_the_wall_then_slides() {
        let mut r = row();
        r[io::NOW] = 1.0;
        r[io::DT] = 0.033;
        r[io::VEL_X] = 9.0;
        r[io::SCENE] = scene::ROAM;
        r[io::CAN_PROBE] = 1.0;
        assert!(tricks(&mut r, mix(3)));
        assert_eq!(r[io::STAGE], 1.0);
        assert_eq!(r[io::HEADING], 1.0);
        let drawn = r[io::COUNTER];
        assert_eq!(drawn, 1.0); // the look's own timer
        r[io::STAGE] = 2.0;
        assert!(tricks(&mut r, mix(3)));
        assert_eq!(r[io::STAGE], 0.0);
        // No wall came back, so whatever happened, no wall run started.
        assert!(r[io::WALL..io::WALL + 3].iter().all(|v| *v == 0.0));
        assert!(r[io::COUNTER] > drawn);
    }

    #[test]
    fn slide_wait_crouches_then_slides() {
        let mut r = row();
        r[io::PHASE] = phase::SLIDE_WAIT;
        r[io::AT] = 0.5;
        r[io::NOW] = 1.0;
        r[io::VEL_X] = 8.0;
        assert!(tricks(&mut r, mix(5)));
        assert_eq!(r[io::PHASE], phase::SLIDING);
        assert_eq!(r[io::CROUCH], 1.0);
        assert_eq!(r[io::WAS_CROUCH], 1.0);
        assert_eq!(r[io::COUNTS + C_SLIDE], 1.0);
        assert_eq!(r[io::COUNTER], 1.0);
    }

    #[test]
    fn ride_steers_along_the_wall() {
        let mut r = row();
        r[io::PHASE] = phase::RIDE;
        r[io::UNTIL] = 5.0;
        r[io::NOW] = 1.0;
        r[io::WALL_RUNNING] = 1.0;
        r[io::GROUNDED] = 0.0;
        r[io::WALL] = 1.0;
        r[io::VEL_Z] = -8.0;
        assert!(tricks(&mut r, mix(9)));
        assert_eq!(r[io::PHASE], phase::RIDE);
        let m = Vec2::of(r[io::MOVE_X], r[io::MOVE_Y]);
        assert!(m.length() <= 1.0 + 1e-6);
    }

    #[test]
    fn duck_holds_crouch() {
        let mut r = row();
        r[io::DUCK_UNTIL] = 2.0;
        r[io::NOW] = 1.0;
        r[io::LOOK_AT] = 9.0;
        assert!(tricks(&mut r, mix(1)));
        assert_eq!(r[io::CROUCH], 1.0);
        assert_eq!(r[io::COUNTER], 0.0);
    }

    #[test]
    fn resident_matches_the_carried_row() {
        let mut carried = row();
        let mut res = Resident::default();
        let key = mix(7);
        let mut counter = 0i64;
        for k in 0..400 {
            let now = k as f64 * 0.05;
            let fast = k % 40 < 25;
            let grounded = k % 17 != 0;
            let i = Input {
                now,
                dt: 0.05,
                up: if k % 50 < 5 { 3.0 } else { 0.0 },
                yaw: 10.0 * k as f64,
                out_yaw: 10.0 * k as f64,
                health: 0.4,
                vel_x: if fast { 9.0 } else { 1.0 },
                vel_z: 2.0,
                move_x: 0.0,
                move_y: 1.0,
                counter,
                bits: (k % 6) as i64
                    | if grounded { packed::IN_GROUNDED } else { 0 }
                    | if k % 9 < 4 {
                        packed::IN_WALL_RUNNING
                    } else {
                        0
                    }
                    | packed::IN_AIR_JUMP_READY
                    | packed::IN_CAN_PROBE,
                profile: [0.3, 0.5, 0.5, 0.2, 0.5, 0.5, 1.0, 6.5],
            };
            carried[io::STAGE] = 0.0;
            carried[io::COUNTER] = counter as f64;
            carried[io::NOW] = i.now;
            carried[io::DT] = i.dt;
            carried[io::SCENE] = (i.bits & 7) as f64;
            carried[io::UP] = i.up;
            carried[io::YAW] = i.yaw;
            carried[io::VEL_X] = i.vel_x;
            carried[io::VEL_Z] = i.vel_z;
            carried[io::GROUNDED] = flag(grounded);
            carried[io::WALL_RUNNING] = flag(i.bits & packed::IN_WALL_RUNNING != 0);
            carried[io::AIR_JUMP_READY] = 1.0;
            carried[io::HEALTH] = i.health;
            carried[io::CAN_PROBE] = 1.0;
            carried[io::MOVE_X] = 0.0;
            carried[io::MOVE_Y] = 1.0;
            carried[io::JUMP] = 0.0;
            carried[io::CROUCH] = 0.0;
            carried[io::AIR_JUMPED] = 0.0;
            carried[io::OUT_YAW] = i.out_yaw;
            assert!(tricks(&mut carried, key));
            let wall = if k % 3 == 0 {
                [0.0, 0.0, 1.0]
            } else {
                [0.0; 3]
            };
            if carried[io::STAGE] == 1.0 {
                carried[io::STAGE] = 2.0;
                carried[io::WALL_IN..io::WALL_IN + 3].copy_from_slice(&wall);
                assert!(tricks(&mut carried, key));
            }
            let mut got = res.step(&i, key);
            if got & packed::PROBE != 0 {
                got = res.resume(wall, key);
            }
            assert_eq!(res.state(), &carried[..STATE_LEN], "tick {k}");
            assert_eq!(got & packed::JUMP != 0, carried[io::JUMP] != 0.0);
            assert_eq!(got & packed::CROUCH != 0, carried[io::CROUCH] != 0.0);
            assert_eq!(got & packed::AIR_JUMP != 0, carried[io::AIR_JUMPED] != 0.0);
            assert_eq!(res.move_xy(), (carried[io::MOVE_X], carried[io::MOVE_Y]));
            for c in 0..6 {
                let n = (got >> (packed::COUNTS_SHIFT + 4 * c as u32)) & 15;
                assert_eq!(n as f64, carried[io::COUNTS + c]);
            }
            counter += got >> packed::COUNTER_SHIFT;
            assert_eq!(counter as f64, carried[io::COUNTER]);
        }
        res.reset();
        assert_eq!(res.state()[io::DUCK_UNTIL], f64::NEG_INFINITY);
    }
}
