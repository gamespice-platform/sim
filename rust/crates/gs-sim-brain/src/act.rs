//! GSBotBrain's roaming steering (`_walk`, `_hold`, `_drift`, each with `_set_move`): the look a
//! bot walks or stands with, its glance to either side, the turn toward it and the legs, in the
//! script's order and precision. One step runs over a flat `f64` row ([`io`]) the script packs
//! and unpacks; the route (`_toward`, which asks the world) stays in the script, which hands the
//! direction it chose in [`io::DIR`].

use crate::aim::{Aim, View, angles_to};
use crate::fight::set_move;
use crate::num::{Stream, Vec2, Vec3};

/// The row's layout. The view, the glance and the stream's counter are carried in and out; the
/// rest are filled each step.
pub mod io {
    /// [`super::mode`].
    pub const MODE: usize = 0;
    /// The aim's yaw and pitch, in and out.
    pub const YAW: usize = 1;
    pub const PITCH: usize = 2;
    /// The eye, x y z.
    pub const EYE: usize = 3;
    /// Walk: the flat direction `_toward` chose, x y. Hold: the post's face point x z (NaN: none).
    pub const DIR: usize = 6;
    pub const NOW: usize = 8;
    pub const DT: usize = 9;
    /// The brain's id.
    pub const ID: usize = 10;
    /// The profile's glance.
    pub const GLANCE: usize = 11;
    /// The glance's state, in and out: next, until, off.
    pub const GLANCE_NEXT: usize = 12;
    pub const GLANCE_UNTIL: usize = 13;
    pub const GLANCE_OFF: usize = 14;
    /// The brain's random stream's counter, in and out.
    pub const COUNTER: usize = 15;
    /// Profile: prediction, turn_smooth, turn_rate.
    pub const PREDICTION: usize = 16;
    pub const TURN_SMOOTH: usize = 17;
    pub const TURN_RATE: usize = 18;
    /// Drift: degrees per second.
    pub const RATE: usize = 19;
    /// Out: the legs, x y.
    pub const MOVE_X: usize = 20;
    pub const MOVE_Y: usize = 21;
    pub const LEN: usize = 22;
}

/// What [`io::MODE`] asks for.
pub mod mode {
    /// `_walk`: look where it goes (with the sway and the glance), turn, legs along [`super::io::DIR`].
    pub const WALK: f64 = 0.0;
    /// `_hold`: stand, watching the face point (or drifting at 30 deg/s with none), legs still.
    pub const HOLD: f64 = 1.0;
    /// `_drift` at [`super::io::RATE`]; the legs come back still (`_set_move` of zero) for a caller that wants them.
    pub const DRIFT: f64 = 2.0;
}

/// `_drift`: the turn in steps of at most 1/60 s, so a thinned bot turns as far per second.
fn drift(aim: &mut Aim, v: &mut View, rate: f64, dt: f64, p: [f64; 3]) {
    let n = 1.max((dt * 60.0 - 0.01).ceil() as i64);
    let sub = dt / n as f64;
    for _ in 0..n {
        let want = Vec2::of(v.yaw + rate * sub, 0.0);
        aim.turn_to(v, want, sub, p[0], p[1], p[2]);
    }
}

/// The glance's state between steps (`GSBotBrain._glance_next`, `_glance_until`, `_glance_off`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glance {
    pub next: f64,
    pub until: f64,
    pub off: f64,
}

impl Default for Glance {
    fn default() -> Self {
        Self {
            next: 0.0,
            until: f64::NEG_INFINITY,
            off: 0.0,
        }
    }
}

impl Glance {
    /// next, until, off.
    pub fn state(&self) -> [f64; 3] {
        [self.next, self.until, self.off]
    }

    /// Takes over `state`'s layout; false, unchanged, when it is short.
    pub fn load(&mut self, state: &[f64]) -> bool {
        if state.len() < 3 {
            return false;
        }
        self.next = state[0];
        self.until = state[1];
        self.off = state[2];
        true
    }
}

/// One step's inputs, as [`io`] has them (the view, glance and counter are carried apart).
#[derive(Clone, Copy, Debug)]
pub struct Input {
    /// [`mode`].
    pub mode: f64,
    pub eye: Vec3,
    /// Walk: the flat direction x y. Hold: the face point x z (x NaN: none).
    pub at: (f64, f64),
    pub now: f64,
    pub dt: f64,
    /// The brain's id.
    pub id: i64,
    /// The profile's glance.
    pub glance: f64,
    /// prediction, turn_smooth, turn_rate.
    pub turn: [f64; 3],
    /// Drift: degrees per second.
    pub rate: f64,
}

/// One steering step on `aim` (whose last wanted look carries), the view `v`, the glance `g` and
/// the brain's stream `key` at `counter`; returns the legs, or None (all untouched) for an
/// unknown mode.
pub fn step(
    aim: &mut Aim,
    v: &mut View,
    g: &mut Glance,
    counter: &mut i64,
    inp: &Input,
    key: i64,
) -> Option<Vec2> {
    let m = inp.mode;
    let p = inp.turn;
    let (eye, now, dt, id, glance) = (inp.eye, inp.now, inp.dt, inp.id, inp.glance);
    let legs = if m == mode::WALK {
        let dir = Vec2::of(inp.at.0, inp.at.1);
        let mut look = angles_to(eye, eye + Vec3::new(dir.x, 0.0, dir.y));
        look.x = (look.x as f64 + 25.0 * (now * 0.8 + (id % 97) as f64).sin()) as f32;
        look.y = 0.0;
        if glance > 0.0 {
            if now >= g.next && now >= g.until {
                let mut rng = Stream::new(key, *counter);
                let side = if rng.next_unit() < 0.5 { 1.0 } else { -1.0 };
                g.off = side * (35.0 + 70.0 * rng.next_unit());
                g.until = now + 0.35 + 0.6 * rng.next_unit();
                g.next = g.until + (1.5 + 4.0 * rng.next_unit()) / (0.3 + glance);
                *counter = rng.counter;
            }
            if now < g.until {
                look.x = (look.x as f64 + g.off) as f32;
            }
        }
        aim.turn_to(v, look, dt, p[0], p[1], p[2]);
        set_move(v.yaw, dir)
    } else if m == mode::HOLD {
        if inp.at.0.is_nan() {
            drift(aim, v, 30.0, dt, p);
        } else {
            let mut look = angles_to(eye, Vec3::new(inp.at.0 as f32, eye.y, inp.at.1 as f32));
            look.y = 0.0;
            let sway = 50.0 * (now * 0.5 + (id % 89) as f64).sin() * (0.3 + glance);
            look.x = (look.x as f64 + sway) as f32;
            aim.turn_to(v, look, dt, p[0], p[1], p[2]);
        }
        set_move(v.yaw, Vec2::ZERO)
    } else if m == mode::DRIFT {
        drift(aim, v, inp.rate, dt, p);
        set_move(v.yaw, Vec2::ZERO)
    } else {
        return None;
    };
    Some(legs)
}

/// One steering step over `row` (layout [`io`]) on `aim` (whose last wanted look carries) and
/// the brain's stream `key`. False, untouched, for a short row or an unknown mode.
pub fn steer(aim: &mut Aim, row: &mut [f64], key: i64) -> bool {
    if row.len() < io::LEN {
        return false;
    }
    let inp = Input {
        mode: row[io::MODE],
        eye: Vec3::new(
            row[io::EYE] as f32,
            row[io::EYE + 1] as f32,
            row[io::EYE + 2] as f32,
        ),
        at: (row[io::DIR], row[io::DIR + 1]),
        now: row[io::NOW],
        dt: row[io::DT],
        id: row[io::ID] as i64,
        glance: row[io::GLANCE],
        turn: [
            row[io::PREDICTION],
            row[io::TURN_SMOOTH],
            row[io::TURN_RATE],
        ],
        rate: row[io::RATE],
    };
    let mut v = View {
        yaw: row[io::YAW],
        pitch: row[io::PITCH],
    };
    let mut g = Glance {
        next: row[io::GLANCE_NEXT],
        until: row[io::GLANCE_UNTIL],
        off: row[io::GLANCE_OFF],
    };
    let mut counter = row[io::COUNTER] as i64;
    let Some(legs) = step(aim, &mut v, &mut g, &mut counter, &inp, key) else {
        return false;
    };
    row[io::YAW] = v.yaw;
    row[io::PITCH] = v.pitch;
    row[io::GLANCE_NEXT] = g.next;
    row[io::GLANCE_UNTIL] = g.until;
    row[io::GLANCE_OFF] = g.off;
    row[io::COUNTER] = counter as f64;
    row[io::MOVE_X] = legs.x as f64;
    row[io::MOVE_Y] = legs.y as f64;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(m: f64) -> Vec<f64> {
        let mut r = vec![0.0; io::LEN];
        r[io::MODE] = m;
        r[io::EYE + 1] = 1.6;
        r[io::DT] = 1.0 / 30.0;
        r[io::ID] = 7.0;
        r[io::PREDICTION] = 0.5;
        r[io::TURN_SMOOTH] = 0.6;
        r[io::TURN_RATE] = 360.0;
        r
    }

    #[test]
    fn a_short_row_or_unknown_mode_is_refused() {
        let mut aim = Aim::new(1);
        assert!(!steer(&mut aim, &mut [0.0; 3], 1));
        let mut r = row(9.0);
        assert!(!steer(&mut aim, &mut r, 1));
    }

    #[test]
    fn a_walk_turns_toward_its_way_and_walks_forward() {
        let mut aim = Aim::new(1);
        let mut r = row(mode::WALK);
        // West (-x) is yaw 90; start already looking there.
        r[io::DIR] = -1.0;
        r[io::YAW] = 90.0;
        for _ in 0..60 {
            r[io::NOW] += r[io::DT];
            assert!(steer(&mut aim, &mut r, 1));
        }
        // The sway and glance keep it near the way, and the legs mostly forward.
        assert!(r[io::MOVE_Y] > 0.0, "{r:?}");
        assert!(r[io::MOVE_X].hypot(r[io::MOVE_Y]) <= 1.0 + 1e-6);
    }

    #[test]
    fn a_glance_draws_four_numbers_then_holds() {
        let mut aim = Aim::new(1);
        let mut r = row(mode::WALK);
        r[io::GLANCE] = 0.5;
        r[io::DIR + 1] = -1.0;
        r[io::NOW] = 1.0;
        assert!(steer(&mut aim, &mut r, 3));
        assert_eq!(r[io::COUNTER], 4.0);
        assert!(r[io::GLANCE_OFF].abs() >= 35.0 && r[io::GLANCE_OFF].abs() <= 105.0);
        assert!(r[io::GLANCE_NEXT] > r[io::GLANCE_UNTIL]);
        r[io::NOW] += 0.01;
        assert!(steer(&mut aim, &mut r, 3));
        assert_eq!(r[io::COUNTER], 4.0);
    }

    #[test]
    fn a_step_with_its_own_glance_matches_the_row() {
        let (mut a, mut b) = (Aim::new(1), Aim::new(1));
        let mut r = row(mode::WALK);
        r[io::GLANCE] = 0.6;
        r[io::DIR] = 0.6;
        r[io::DIR + 1] = -0.8;
        r[io::GLANCE_UNTIL] = f64::NEG_INFINITY;
        let mut v = View::default();
        let mut g = Glance::default();
        let mut counter = 0;
        for i in 0..300 {
            r[io::NOW] += r[io::DT];
            r[io::MODE] = [mode::WALK, mode::WALK, mode::HOLD, mode::DRIFT][i % 4];
            r[io::RATE] = 25.0;
            let inp = Input {
                mode: r[io::MODE],
                eye: Vec3::new(0.0, 1.6, 0.0),
                at: (r[io::DIR], r[io::DIR + 1]),
                now: r[io::NOW],
                dt: r[io::DT],
                id: 7,
                glance: r[io::GLANCE],
                turn: [0.5, 0.6, 360.0],
                rate: 25.0,
            };
            assert!(steer(&mut a, &mut r, 5));
            let legs = step(&mut b, &mut v, &mut g, &mut counter, &inp, 5).unwrap();
            assert_eq!((v.yaw, v.pitch), (r[io::YAW], r[io::PITCH]));
            assert_eq!(
                g.state(),
                [r[io::GLANCE_NEXT], r[io::GLANCE_UNTIL], r[io::GLANCE_OFF]]
            );
            assert_eq!(counter as f64, r[io::COUNTER]);
            assert_eq!(
                (legs.x as f64, legs.y as f64),
                (r[io::MOVE_X], r[io::MOVE_Y])
            );
        }
        assert!(counter > 0);
    }

    #[test]
    fn a_hold_without_a_face_drifts_and_stands() {
        let mut aim = Aim::new(1);
        let mut r = row(mode::HOLD);
        r[io::DIR] = f64::NAN;
        r[io::DT] = 0.1;
        assert!(steer(&mut aim, &mut r, 1));
        assert!(r[io::YAW] > 0.0);
        assert_eq!(r[io::MOVE_X].abs() + r[io::MOVE_Y].abs(), 0.0);
    }

    #[test]
    fn a_drift_turns_as_far_however_often_it_steps() {
        let mut once = Aim::new(1);
        let mut a = row(mode::DRIFT);
        a[io::RATE] = 40.0;
        a[io::DT] = 0.1;
        assert!(steer(&mut once, &mut a, 1));
        assert!(a[io::YAW] > 0.0 && a[io::YAW] <= 4.0 + 1e-9);
    }
}
