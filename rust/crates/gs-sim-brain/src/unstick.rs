//! GSBotUnstick's `step`: a bot that pushes to move and gets nowhere escalates from a re-plan and
//! a hop to a crouch-hop and side-step, then to a nudge toward a standable point. The script's
//! arithmetic in its order and precision (the position `f32`, the clocks `f64`).
//!
//! The nudge's target (`GSBotWorld.walkable_near`, a world query, and the random fallback on the
//! brain's stream) stays in the script: a step that wants one says so ([`NUDGE`]) and the script
//! asks the world, as it would have.

use crate::num::Vec3;

/// Seconds of no progress that raise the level one step (GSBotUnstick.WINDOW).
pub const WINDOW: f64 = 1.0;
/// The distance that counts as progress (GSBotUnstick.PROGRESS).
pub const PROGRESS: f64 = 0.45;
/// From this level the bot is walked toward a standable point (GSBotUnstick.NUDGE_LEVEL).
pub const NUDGE_LEVEL: i64 = 3;

/// [`Unstick::step`]'s answer bits; the level is the rest, from [`LEVEL_SHIFT`].
pub const JUMP: i64 = 1;
pub const CROUCH: i64 = 2;
pub const REPLAN: i64 = 4;
pub const STRANDED: i64 = 8;
/// The side-step is on: right, or left with [`STRAFE_LEFT`].
pub const STRAFE: i64 = 16;
pub const STRAFE_LEFT: i64 = 32;
/// A window ended stuck this step (the script's `total` counts them).
pub const STUCK: i64 = 64;
/// The step wants the nudge toward a standable point.
pub const NUDGE: i64 = 128;
pub const LEVEL_SHIFT: u32 = 8;

/// The kept state's length ([`Unstick::state`]).
pub const STATE_LEN: usize = 9;

/// GSBotUnstick's private state, kept between steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unstick {
    pub level: i64,
    stuck_since: f64,
    anchor: Vec3,
    anchor_at: f64,
    pushed: f64,
    seen: f64,
    side: f64,
}

impl Default for Unstick {
    fn default() -> Self {
        Self {
            level: 0,
            stuck_since: f64::INFINITY,
            anchor: inf3(),
            anchor_at: 0.0,
            pushed: 0.0,
            seen: 0.0,
            side: 1.0,
        }
    }
}

fn inf3() -> Vec3 {
    Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY)
}

impl Unstick {
    /// GSBotUnstick.step up to the nudge's query: [`JUMP`] .. [`NUDGE`] or'd with the level from
    /// [`LEVEL_SHIFT`]. `can_nudge`: the brain has a world to ask. `after` is `bot_stranded_sec`.
    pub fn step(
        &mut self,
        position: Vec3,
        pushing: bool,
        now: f64,
        can_nudge: bool,
        after: f64,
    ) -> i64 {
        let mut bits = 0;
        if self.anchor == inf3() {
            self.reset(position, now);
        }
        self.seen += 1.0;
        self.pushed += if pushing { 1.0 } else { 0.0 };
        if now - self.anchor_at >= WINDOW {
            let moved = position.distance_to(self.anchor) as f64;
            if self.pushed / self.seen.max(1.0) > 0.6 && moved < PROGRESS {
                if self.level == 0 {
                    self.stuck_since = self.anchor_at;
                }
                self.level += 1;
                bits |= STUCK;
                self.side = -self.side;
            } else if moved >= PROGRESS * 2.0 {
                self.level = 0;
                self.stuck_since = f64::INFINITY;
            }
            self.anchor = position;
            self.anchor_at = now;
            self.pushed = 0.0;
            self.seen = 0.0;
        }
        let out = bits | (self.level << LEVEL_SHIFT);
        if self.level == 0 || !pushing {
            return out;
        }
        bits |= REPLAN | JUMP;
        if after > 0.0 && self.level >= NUDGE_LEVEL && now - self.stuck_since >= after {
            bits |= STRANDED;
            self.stuck_since = now;
        }
        if self.level >= 2 {
            if ((now * 4.0) as i64) % 2 == 0 {
                bits |= CROUCH;
            }
            if self.side != 0.0 {
                bits |= STRAFE;
                if self.side < 0.0 {
                    bits |= STRAFE_LEFT;
                }
            }
        }
        if self.level >= NUDGE_LEVEL && can_nudge {
            bits |= NUDGE;
        }
        bits | (self.level << LEVEL_SHIFT)
    }

    /// GSBotUnstick.reset: forget the history (an infinite `position` waits for the next step).
    pub fn reset(&mut self, position: Vec3, now: f64) {
        self.level = 0;
        self.stuck_since = f64::INFINITY;
        self.anchor = position;
        self.anchor_at = now;
        self.pushed = 0.0;
        self.seen = 0.0;
    }

    /// level, stuck_since, anchor x y z, anchor_at, pushed, seen, side.
    pub fn state(&self) -> [f64; STATE_LEN] {
        [
            self.level as f64,
            self.stuck_since,
            self.anchor.x as f64,
            self.anchor.y as f64,
            self.anchor.z as f64,
            self.anchor_at,
            self.pushed,
            self.seen,
            self.side,
        ]
    }

    /// Takes the state over in [`Self::state`]'s layout; false (and keeps its own) when short.
    pub fn load(&mut self, s: &[f64]) -> bool {
        if s.len() < STATE_LEN {
            return false;
        }
        self.level = s[0] as i64;
        self.stuck_since = s[1];
        self.anchor = Vec3::new(s[2] as f32, s[3] as f32, s[4] as f32);
        self.anchor_at = s[5];
        self.pushed = s[6];
        self.seen = s[7];
        self.side = s[8];
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> Vec3 {
        Vec3::new(x, 0.0, 0.0)
    }

    #[test]
    fn pushing_in_place_climbs_a_level_a_window() {
        let mut u = Unstick::default();
        let mut now = 0.0;
        let mut levels = vec![];
        while now < 4.05 {
            let r = u.step(at(0.0), true, now, true, 0.0);
            levels.push(r >> LEVEL_SHIFT);
            now += 0.25;
        }
        assert_eq!(*levels.last().unwrap(), 4);
        let r = u.step(at(0.0), true, now, true, 0.0);
        assert_ne!(r & NUDGE, 0);
        assert_ne!(r & STRAFE, 0);
        assert_ne!(r & (REPLAN | JUMP), 0);
    }

    #[test]
    fn walking_away_clears_the_level() {
        let mut u = Unstick::default();
        let mut now = 0.0;
        while now < 2.05 {
            u.step(at(0.0), true, now, false, 0.0);
            now += 0.25;
        }
        assert!(u.level >= 2);
        u.step(at(5.0), true, now + 1.0, false, 0.0);
        assert_eq!(u.level, 0);
    }

    #[test]
    fn stranded_fires_once_an_interval() {
        let mut u = Unstick::default();
        let mut now = 0.0;
        let mut fired = 0;
        while now < 12.0 {
            if u.step(at(0.0), true, now, true, 4.0) & STRANDED != 0 {
                fired += 1;
            }
            now += 0.125;
        }
        assert_eq!(fired, 2);
    }

    #[test]
    fn state_round_trips() {
        let mut u = Unstick::default();
        for i in 0..40 {
            u.step(at(0.01 * i as f32), i % 3 != 0, i as f64 * 0.1, true, 0.0);
        }
        let mut v = Unstick::default();
        assert!(v.load(&u.state()));
        assert_eq!(u, v);
        assert!(!v.load(&[0.0; 3]));
    }
}
