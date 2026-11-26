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
}
