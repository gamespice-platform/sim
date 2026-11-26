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
/// In `extra[1]`: the row's effect and tunable values (row 23..38) are the motor state's kept ones
/// ([`fill_consts`]), so the script did not write them into `extra` this tick.
pub const ROW_CONSTS: i64 = 4;
/// Row values a motor state's kept effect and tunable values cover, and where they start.
pub const CONSTS_LEN: usize = 16;
pub const ROW_CONSTS_AT: usize = 23;
/// Flag a batch adds to a row's answer when any counter in it (the collision counters, 13..19,
/// and the move counters, 40..44) is not zero, so the script reads them only then.
pub const OUT_COUNTED: i64 = 32;
/// In `extra[1]`: the row's columns are the ones the player began its tick with, so the batch may
/// say what the tick changed ([`events`]; gs-player-controller 42).
pub const ROW_EVENTS: i64 = 8;
/// Flags [`events`] adds to a row's answer: events were looked at (the bits after it hold), the
/// mode changed, the height changed, a slide started or ended, a wall run started or ended.
pub const OUT_EVENTS: i64 = 64;
pub const OUT_MODE: i64 = 128;
pub const OUT_MOVED_Y: i64 = 256;
pub const OUT_SLIDE: i64 = 512;
pub const OUT_WALL_RUN: i64 = 1024;
/// Columns of the slide time and the wall-run time (sliding and wall running while at or over 0).
const SLIDE_TIME: usize = 17;
const WALL_RUN_TIME: usize = 22;

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

/// Writes the kept effect and tunable values `consts` into the row (`row` 55 values, `extra` its
/// 27) where the script asked for them ([`ROW_CONSTS`]); else the row keeps `extra`'s.
pub fn fill_consts(consts: Option<&[f64; CONSTS_LEN]>, extra: &[f64], row: &mut [f64]) {
    if extra[1] as i64 & ROW_CONSTS == 0 {
        return;
    }
    if let Some(c) = consts {
        row[ROW_CONSTS_AT..ROW_CONSTS_AT + CONSTS_LEN].copy_from_slice(c);
    }
}

/// Whether a row's answer `out` counts anything ([`OUT_COUNTED`]).
pub fn counted(out: &[f64]) -> bool {
    out[13..20].iter().chain(&out[40..45]).any(|&v| v != 0.0)
}

/// What a tick from columns `c` to `new` changed, as the controller's end of tick compares them
/// (`GSFpsController.end_tick`): [`OUT_EVENTS`] and a bit per change. The height compares as the
/// script's `y - before_y`, so -0 and 0 are no change and a NaN is one.
pub fn events(c: &[f64], new: &[f64]) -> i64 {
    let mut f = OUT_EVENTS;
    if c[MODE] != new[MODE] {
        f |= OUT_MODE;
    }
    if c[1] != new[1] {
        f |= OUT_MOVED_Y;
    }
    if (c[SLIDE_TIME] >= 0.0) != (new[SLIDE_TIME] >= 0.0) {
        f |= OUT_SLIDE;
    }
    if (c[WALL_RUN_TIME] >= 0.0) != (new[WALL_RUN_TIME] >= 0.0) {
        f |= OUT_WALL_RUN;
    }
    f
}

const _: () = assert!(cols::STRIDE > 12 && cols::ROW_OUT == 45);
const _: () = assert!(ROW_CONSTS_AT + CONSTS_LEN == 12 + cols::EXTRA_LEN);

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
    fn fill_consts_only_where_asked_and_kept() {
        let c: [f64; CONSTS_LEN] = std::array::from_fn(|i| 50.0 + i as f64);
        let mut row = vec![9.0; cols::ROW_IN];
        fill_consts(Some(&c), &extra(ROW_SCRIPT_OK | ROW_REUSE), &mut row);
        assert!(row.iter().all(|&v| v == 9.0));
        fill_consts(None, &extra(ROW_CONSTS), &mut row);
        assert!(row.iter().all(|&v| v == 9.0));
        fill_consts(Some(&c), &extra(ROW_CONSTS | ROW_SCRIPT_OK), &mut row);
        assert_eq!(&row[23..39], &c[..]);
        assert_eq!(row[22], 9.0);
        assert_eq!(row[39], 9.0);
        // The rest bits still read as before beside the consts bit.
        let r = Rest::default();
        r.fill_row(&columns(), &extra(ROW_CONSTS), &mut row);
        assert_eq!(row[13], 0.0);
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

    #[test]
    fn events_see_each_change_and_nothing_else() {
        let c = columns();
        let mut c = c;
        c[SLIDE_TIME] = -1.0;
        c[WALL_RUN_TIME] = -1.0;
        let mut new = c.clone();
        new[0] += 1.0;
        new[4] += 2.0;
        assert_eq!(events(&c, &new), OUT_EVENTS);
        new[MODE] = 1.0;
        assert_eq!(events(&c, &new), OUT_EVENTS | OUT_MODE);
        new[1] += 0.5;
        new[SLIDE_TIME] = 0.0;
        new[WALL_RUN_TIME] = 0.25;
        assert_eq!(
            events(&c, &new),
            OUT_EVENTS | OUT_MODE | OUT_MOVED_Y | OUT_SLIDE | OUT_WALL_RUN
        );
        // Still sliding (a later time) is no change; ending is one.
        let mut sliding = c.clone();
        sliding[SLIDE_TIME] = 0.1;
        let mut later = sliding.clone();
        later[SLIDE_TIME] = 0.2;
        assert_eq!(events(&sliding, &later), OUT_EVENTS);
        later[SLIDE_TIME] = -1.0;
        assert_eq!(events(&sliding, &later), OUT_EVENTS | OUT_SLIDE);
        let mut zero = c.clone();
        zero[1] = 0.0;
        let mut neg = zero.clone();
        neg[1] = -0.0;
        assert_eq!(events(&zero, &neg), OUT_EVENTS, "-0 is no height change");
        neg[1] = f64::NAN;
        assert_eq!(events(&zero, &neg), OUT_EVENTS | OUT_MOVED_Y);
    }
}
