//! The motor state a player keeps between ticks beside its `GSFpsState` columns: the rest stamp
//! (gs-player-controller's `_stamp_rest`, read by `_native_rest_ok`). With it a batch row decides
//! natively whether the start ground check may reuse the last rest, and stamps the new one where the
//! tick's end check settled, without the script reading the state's columns either way.
//!
//! Values are compared as the columns hold them (float64s of float32 vectors and whole numbers),
//! which is the script's float32 `Vector3` and int comparison to the bit.

use crate::cols;
use crate::tick::GROUND;

/// Column of the state's mode.
const MODE: usize = 9;
/// Values a rest stamp keeps: position, velocity, ground normal, the ground id's halves.
pub const REST_VALS: usize = 11;
/// Values [`Rest::to_vals`] writes: valid (1 or 0), the stamp's values, height.
pub const REST_LEN: usize = REST_VALS + 2;
/// In `extra[1]` of a row (row value 13; [`crate::cols::EXTRA_LEN`] layout): the script's part of rest ok (the
/// stamp is the native one, still valid on the script's side, and the surface is the stamped one).
pub const ROW_SCRIPT_OK: i64 = 1;
/// In `extra[1]`: a settled end may be stamped (gs-player-controller's `reuse_settled_ground`).
pub const ROW_REUSE: i64 = 2;
/// Flag a batch adds to a row's answer when any counter in it (the collision counters, 13..19,
/// and the move counters, 40..44) is not zero, so the script reads them only then.
pub const OUT_COUNTED: i64 = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rest {
    pub valid: bool,
    /// Position, velocity, ground normal and the ground id's halves, as columns 0..9 and 10..11.
    pub vals: [f64; REST_VALS],
    pub height: f64,
}

impl Rest {
    /// `_native_rest_ok` but the script's part: all the start check's reuse compares but the
    /// position (which depenetration may move) and the surface. `c` the state's columns.
    pub fn ok(&self, c: &[f64], height: f64) -> bool {
        self.valid
            && c[MODE] == GROUND as f64
            && c[3..9] == self.vals[3..9]
            && c[10..12] == self.vals[9..11]
            && height == self.height
    }

    /// `_stamp_rest` from the columns `c` a settled tick ended on.
    pub fn stamp(&mut self, c: &[f64], height: f64, reuse: bool) {
        self.valid = reuse && c[MODE] == GROUND as f64;
        self.vals[..9].copy_from_slice(&c[..9]);
        self.vals[9..].copy_from_slice(&c[10..12]);
        self.height = height;
    }

    /// Writes a run's input row's rest values (`row` the row's 55 values, `extra` its 27): rest ok
    /// at 13 where the script's part and [`Self::ok`] both hold, the stamp's position at 14..16.
    pub fn fill_row(&self, c: &[f64], extra: &[f64], row: &mut [f64]) {
        let script = extra[1] as i64 & ROW_SCRIPT_OK != 0;
        row[13] = if script && self.ok(c, extra[0]) {
            1.0
        } else {
            0.0
        };
        row[14..17].copy_from_slice(&self.vals[..3]);
    }

    /// After a row's tick with answer `out` and new columns `new`: stamps where the end check
    /// settled (flag 8), else drops the stamp, as the script's apply does.
    pub fn after(&mut self, new: &[f64], extra: &[f64], out: &[f64]) {
        if out[12] as i64 & 8 != 0 {
            self.stamp(new, extra[0], extra[1] as i64 & ROW_REUSE != 0);
        } else {
            self.valid = false;
        }
    }

    /// The stamp [`Self::to_vals`] wrote (a shorter `v` leaves the stamp invalid).
    pub fn from_vals(v: &[f64]) -> Rest {
        let mut r = Rest::default();
        if v.len() >= REST_LEN {
            r.valid = v[0] != 0.0;
            r.vals.copy_from_slice(&v[1..=REST_VALS]);
            r.height = v[REST_LEN - 1];
        }
        r
    }

    pub fn to_vals(&self) -> [f64; REST_LEN] {
        let mut v = [0.0; REST_LEN];
        v[0] = if self.valid { 1.0 } else { 0.0 };
        v[1..=REST_VALS].copy_from_slice(&self.vals);
        v[REST_LEN - 1] = self.height;
        v
    }
}

/// Whether a row's answer `out` counts anything ([`OUT_COUNTED`]).
pub fn counted(out: &[f64]) -> bool {
    out[13..20].iter().chain(&out[40..45]).any(|&v| v != 0.0)
}

const _: () = assert!(cols::STRIDE > 12 && cols::ROW_OUT == 45);

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<f64> {
        let mut c = vec![0.0; cols::STRIDE];
        for (i, v) in c.iter_mut().enumerate().take(9) {
            *v = i as f64 * 0.25 + 1.0;
        }
        c[MODE] = GROUND as f64;
        c[10] = 7.0;
        c[11] = 3.0;
        c
    }

    fn extra(bits: i64) -> Vec<f64> {
        let mut e = vec![0.0; cols::EXTRA_LEN];
        e[0] = 1.8;
        e[1] = bits as f64;
        e
    }

    #[test]
    fn stamp_then_ok_until_anything_compared_differs() {
        let c = columns();
        let mut r = Rest::default();
        assert!(!r.ok(&c, 1.8));
        r.stamp(&c, 1.8, true);
        assert!(r.valid && r.ok(&c, 1.8));
        assert!(!r.ok(&c, 1.0));
        let mut moved = c.clone();
        moved[0] += 1.0;
        assert!(r.ok(&moved, 1.8), "the position is the native check's");
        for i in [3, 5, 6, 8, 10, 11] {
            let mut d = c.clone();
            d[i] += 0.5;
            assert!(!r.ok(&d, 1.8), "column {i}");
        }
        let mut air = c.clone();
        air[MODE] = 1.0;
        assert!(!r.ok(&air, 1.8));
        let mut zero = c.clone();
        zero[4] = -0.0;
        r.stamp(&zero, 1.8, true);
        zero[4] = 0.0;
        assert!(r.ok(&zero, 1.8), "-0 equals 0 as in the script");
    }

    #[test]
    fn stamp_in_air_or_without_reuse_is_not_valid() {
        let mut c = columns();
        let mut r = Rest::default();
        r.stamp(&c, 1.8, false);
        assert!(!r.valid);
        c[MODE] = 1.0;
        r.stamp(&c, 1.8, true);
        assert!(!r.valid);
    }

    #[test]
    fn fill_row_needs_the_script_part() {
        let c = columns();
        let mut r = Rest::default();
        r.stamp(&c, 1.8, true);
        let mut row = vec![9.0; cols::ROW_IN];
        r.fill_row(&c, &extra(ROW_REUSE), &mut row);
        assert_eq!(row[13], 0.0);
        assert_eq!(&row[14..17], &c[..3]);
        r.fill_row(&c, &extra(ROW_SCRIPT_OK), &mut row);
        assert_eq!(row[13], 1.0);
    }

    #[test]
    fn after_stamps_on_settled_else_drops() {
        let c = columns();
        let mut out = vec![0.0; cols::ROW_OUT];
        let mut r = Rest::default();
        out[12] = 8.0;
        r.after(&c, &extra(ROW_REUSE), &out);
        assert!(r.valid);
        assert_eq!(r.to_vals()[0], 1.0);
        assert_eq!(r.to_vals()[REST_LEN - 1], 1.8);
        assert_eq!(r.to_vals()[10], 7.0);
        assert_eq!(Rest::from_vals(&r.to_vals()), r);
        assert!(!Rest::from_vals(&[1.0]).valid);
        out[12] = 3.0;
        r.after(&c, &extra(ROW_REUSE), &out);
        assert!(!r.valid);
    }

    #[test]
    fn counted_sees_collision_and_move_counters_only() {
        let mut out = vec![0.0; cols::ROW_OUT];
        out[12] = 8.0;
        out[20] = 4.0;
        out[39] = 1.5;
        assert!(!counted(&out));
        out[16] = 1.0;
        assert!(counted(&out));
        out[16] = 0.0;
        out[44] = 1.0;
        assert!(counted(&out));
    }
}
