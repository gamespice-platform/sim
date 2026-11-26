//! The motor's crouch update: gs-player-controller's `GSFpsMotor._update_crouch`, `_set_crouch`
//! and `_position_for_crouch`, spelled operation for operation so the batch can run it in the
//! same call as the tick (gs-player-controller API 43).
//!
//! The state's crouch fraction and held flag ride in two columns past the tick's thirty
//! ([`FRACTION`], [`HELD`]); the crouch's tunables ride in the row's tun at [`TUN_AT`].

use crate::state::ROW_CONSTS_AT;
use crate::tick::GROUND;
use crate::{UP, Vec3};
use gs_sim_math::clampf;

/// `GSFpsStateColumns.CROUCH_FRACTION`.
pub const FRACTION: usize = 30;
/// `GSFpsStateColumns.CROUCH_HELD` (1.0 held, 0.0 not).
pub const HELD: usize = 31;
/// The columns a crouching row needs (`GSFpsStateColumns.STRIDE` since API 43).
pub const COLS_LEN: usize = 32;
/// Where the crouch tun starts in a row's full tun (tick tun 19 + full tun 56).
pub const TUN_AT: usize = 75;
/// `[can_crouch, stand_height, crouch_height, crouch_transition_time, instant_air_crouch, radius]`.
pub const TUN_LEN: usize = 6;
/// `GSFpsCommand.BUTTON_CROUCH`.
pub const BUTTON_CROUCH: i64 = 1 << 1;
/// The deny flag bit for `deny_crouch` in the row's deny flags.
pub const DENY_CROUCH: i64 = 4;

const CMP_EPSILON: f64 = 0.00001;
/// The state's mode column.
const MODE: usize = 9;

/// The tunables the crouch reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tun {
    pub can_crouch: bool,
    pub stand: f64,
    pub crouch: f64,
    pub time: f64,
    pub instant_air: bool,
    pub radius: f64,
}

impl Tun {
    /// Reads the crouch tun from a row's full tun; `None` when it is too short (an older script).
    pub fn read(tun: &[f64]) -> Option<Tun> {
        let t = tun.get(TUN_AT..TUN_AT + TUN_LEN)?;
        Some(Tun {
            can_crouch: t[0] != 0.0,
            stand: t[1],
            crouch: t[2],
            time: t[3],
            instant_air: t[4] != 0.0,
            radius: t[5],
        })
    }

    /// `GSFpsTunables.height_at(fraction)`.
    pub fn height_at(&self, fraction: f64) -> f64 {
        let w = clampf(fraction, 0.0, 1.0);
        self.stand + (self.crouch - self.stand) * w
    }
}

/// `is_equal_approx(a, b)` for floats.
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

/// `move_toward(from, to, delta)` for floats.
pub fn move_toward(from: f64, to: f64, delta: f64) -> f64 {
    let d = to - from;
    if d.abs() <= delta {
        return to;
    }
    let s = if d > 0.0 {
        1.0
    } else if d < 0.0 {
        -1.0
    } else {
        0.0
    };
    from + s * delta
}

/// What one crouch update leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Out {
    pub fraction: f64,
    pub held: bool,
    /// The new position when `_set_crouch` ran; `None` when the position was not written.
    pub position: Option<Vec3>,
}

/// `_position_for_crouch(state, fraction, airborne)`.
fn position_for(tun: &Tun, current: f64, fraction: f64, airborne: bool, pos: Vec3) -> Vec3 {
    if !airborne {
        return pos;
    }
    let mut moved = pos;
    moved.y = (moved.y as f64 + (tun.height_at(current) - tun.height_at(fraction))) as f32;
    moved
}

/// `_update_crouch(state, command, delta)`. `overlaps(centre, height, radius)` is the body's.
#[allow(clippy::too_many_arguments)]
pub fn update(
    tun: &Tun,
    fraction: f64,
    pressed: bool,
    deny: bool,
    airborne: bool,
    delta: f64,
    pos: Vec3,
    mut overlaps: impl FnMut(Vec3, f64, f64) -> bool,
) -> Out {
    let held = tun.can_crouch && !deny && pressed;
    let target = if held { 1.0 } else { 0.0 };
    let mut out = Out {
        fraction,
        held,
        position: None,
    };
    if is_equal_approx(fraction, target) {
        out.fraction = target;
        return out;
    }
    let instant = tun.time <= 0.0 || (airborne && tun.instant_air && held);
    let mut next = target;
    if !instant {
        let rate = 1.0 / tun.time;
        next = move_toward(fraction, target, rate * delta);
    }
    if next < fraction {
        let height = tun.height_at(next);
        let feet = position_for(tun, fraction, next, airborne, pos);
        let centre = feet + UP.scale(height * 0.5);
        if overlaps(centre, height, tun.radius) {
            return out;
        }
    }
    out.position = Some(position_for(tun, fraction, next, airborne, pos));
    out.fraction = next;
    out
}

/// A batch row's crouch, before its tick: `c` the state's columns as the tick began, `extra` the
/// row's 27 values ([`crate::cols::EXTRA_LEN`]; the delta at 5, the buttons at 6), `row` its input
/// row with the effect values in (the deny flags at [`ROW_CONSTS_AT`]). Patches the row's position
/// and height and returns the crouched columns and their height, or `None` when the crouch changed
/// nothing in `c` (the row's values then stand as the script filled them).
pub fn row(
    tun: &Tun,
    c: &[f64],
    extra: &[f64],
    row: &mut [f64],
    overlaps: impl FnMut(Vec3, f64, f64) -> bool,
) -> Option<(Vec<f64>, f64)> {
    let deny = row[ROW_CONSTS_AT] as i64 & DENY_CROUCH != 0;
    let pressed = extra[6] as i64 & BUTTON_CROUCH != 0;
    let airborne = c[MODE] != GROUND as f64;
    let pos = Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32);
    let out = update(
        tun,
        c[FRACTION],
        pressed,
        deny,
        airborne,
        extra[5],
        pos,
        overlaps,
    );
    let held = if out.held { 1.0 } else { 0.0 };
    let p = out
        .position
        .map(|p| [f64::from(p.x), f64::from(p.y), f64::from(p.z)]);
    if out.fraction == c[FRACTION] && held == c[HELD] && p.is_none_or(|p| p[..] == c[..3]) {
        return None;
    }
    let mut cc = c.to_vec();
    if let Some(p) = p {
        cc[..3].copy_from_slice(&p);
        row[..3].copy_from_slice(&p);
    }
    cc[FRACTION] = out.fraction;
    cc[HELD] = held;
    let height = tun.height_at(out.fraction);
    row[12] = height;
    Some((cc, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TUN: Tun = Tun {
        can_crouch: true,
        stand: 1.8,
        crouch: 1.0,
        time: 0.2,
        instant_air: true,
        radius: 0.4,
    };
    const P: Vec3 = Vec3::new(1.0, 2.0, 3.0);

    fn never(_: Vec3, _: f64, _: f64) -> bool {
        false
    }

    #[test]
    fn snaps_when_equal_approx() {
        let o = update(&TUN, 0.999_999_5, true, false, false, 0.01, P, never);
        assert_eq!(o.fraction, 1.0);
        assert!(o.held);
        assert_eq!(o.position, None);
    }

    #[test]
    fn deny_and_can_crouch_release() {
        let o = update(&TUN, 0.0, true, true, false, 0.01, P, never);
        assert!(!o.held);
        assert_eq!(o.fraction, 0.0);
        let off = Tun {
            can_crouch: false,
            ..TUN
        };
        assert!(!update(&off, 0.0, true, false, false, 0.01, P, never).held);
    }

    #[test]
    fn ground_keeps_feet_and_moves_at_rate() {
        let o = update(&TUN, 0.0, true, false, false, 0.01, P, never);
        assert_eq!(o.fraction, move_toward(0.0, 1.0, (1.0 / 0.2) * 0.01));
        assert_eq!(o.position, Some(P));
    }

    #[test]
    fn airborne_keeps_head_and_crouches_instantly() {
        let o = update(&TUN, 0.0, true, false, true, 0.01, P, never);
        assert_eq!(o.fraction, 1.0);
        let y = (2.0f32 as f64 + (1.8 - 1.0)) as f32;
        assert_eq!(o.position, Some(Vec3::new(1.0, y, 3.0)));
    }

    #[test]
    fn blocked_stand_up_holds() {
        let mut asked = None;
        let o = update(&TUN, 1.0, false, false, false, 0.01, P, |c, h, r| {
            asked = Some((c, h, r));
            true
        });
        assert_eq!(o.fraction, 1.0);
        assert_eq!(o.position, None);
        let (c, h, r) = asked.unwrap();
        assert_eq!(h, TUN.height_at(1.0 - 0.05));
        assert_eq!(c, P + UP.scale(h * 0.5));
        assert_eq!(r, 0.4);
    }
    #[test]
    fn row_patches_position_and_height() {
        let mut c = vec![0.0; COLS_LEN];
        c[1] = 2.0;
        c[MODE] = 1.0;
        let mut extra = vec![0.0; 27];
        extra[5] = 0.01;
        extra[6] = BUTTON_CROUCH as f64;
        let mut r = vec![0.0; 55];
        let (cc, h) = row(&TUN, &c, &extra, &mut r, never).unwrap();
        assert_eq!(cc[FRACTION], 1.0);
        assert_eq!(cc[HELD], 1.0);
        assert_eq!(h, 1.0);
        assert_eq!(r[12], 1.0);
        assert_eq!(r[1], cc[1]);
        assert_eq!(cc[1], f64::from((2.0 + 0.8f64) as f32));
        r[ROW_CONSTS_AT] = DENY_CROUCH as f64;
        c.copy_from_slice(&cc);
        c[FRACTION] = 0.0;
        c[HELD] = 0.0;
        assert_eq!(row(&TUN, &c, &extra, &mut r, never), None);
    }
}
