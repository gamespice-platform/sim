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

/// One steering step over `row` (layout [`io`]) on `aim` (whose last wanted look carries) and
/// the brain's stream `key`. False, untouched, for a short row or an unknown mode.
pub fn steer(aim: &mut Aim, row: &mut [f64], key: i64) -> bool {
    if row.len() < io::LEN {
        return false;
    }
    let m = row[io::MODE];
    let mut v = View {
        yaw: row[io::YAW],
        pitch: row[io::PITCH],
    };
    let p = [
        row[io::PREDICTION],
        row[io::TURN_SMOOTH],
        row[io::TURN_RATE],
    ];
    let eye = Vec3::new(
        row[io::EYE] as f32,
        row[io::EYE + 1] as f32,
        row[io::EYE + 2] as f32,
    );
    let now = row[io::NOW];
    let dt = row[io::DT];
    let id = row[io::ID] as i64;
    let glance = row[io::GLANCE];
    let legs = if m == mode::WALK {
        let dir = Vec2::of(row[io::DIR], row[io::DIR + 1]);
        let mut look = angles_to(eye, eye + Vec3::new(dir.x, 0.0, dir.y));
        look.x = (look.x as f64 + 25.0 * (now * 0.8 + (id % 97) as f64).sin()) as f32;
        look.y = 0.0;
        if glance > 0.0 {
            let mut next = row[io::GLANCE_NEXT];
            let mut until = row[io::GLANCE_UNTIL];
            let mut off = row[io::GLANCE_OFF];
            if now >= next && now >= until {
                let mut rng = Stream::new(key, row[io::COUNTER] as i64);
                let side = if rng.next_unit() < 0.5 { 1.0 } else { -1.0 };
                off = side * (35.0 + 70.0 * rng.next_unit());
                until = now + 0.35 + 0.6 * rng.next_unit();
                next = until + (1.5 + 4.0 * rng.next_unit()) / (0.3 + glance);
                row[io::COUNTER] = rng.counter as f64;
                row[io::GLANCE_NEXT] = next;
                row[io::GLANCE_UNTIL] = until;
                row[io::GLANCE_OFF] = off;
            }
            if now < until {
                look.x = (look.x as f64 + off) as f32;
            }
        }
        aim.turn_to(&mut v, look, dt, p[0], p[1], p[2]);
        set_move(v.yaw, dir)
    } else if m == mode::HOLD {
        let face = (row[io::DIR], row[io::DIR + 1]);
        if face.0.is_nan() {
            drift(aim, &mut v, 30.0, dt, p);
        } else {
            let mut look = angles_to(eye, Vec3::new(face.0 as f32, eye.y, face.1 as f32));
            look.y = 0.0;
            let sway = 50.0 * (now * 0.5 + (id % 89) as f64).sin() * (0.3 + glance);
            look.x = (look.x as f64 + sway) as f32;
            aim.turn_to(&mut v, look, dt, p[0], p[1], p[2]);
        }
        set_move(v.yaw, Vec2::ZERO)
    } else if m == mode::DRIFT {
        drift(aim, &mut v, row[io::RATE], dt, p);
        set_move(v.yaw, Vec2::ZERO)
    } else {
        return false;
    };
    row[io::YAW] = v.yaw;
    row[io::PITCH] = v.pitch;
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
