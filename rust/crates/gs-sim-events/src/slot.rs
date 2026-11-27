//! An event row filled from a resident motor row instead of the controller's script (FC's native
//! server tick, phase P1f): the before half from the row's state columns as the tick started
//! ([`before`]), the after half from its columns and the motor's answer once it ran ([`after`]).
//! [`fill`] writes both into an [`IN`](crate::IN) row the derive then reads as any other.
//!
//! The script twin is `GSFpsController.begin_tick`'s captures and `_record_event_row`: position and
//! velocity go through a `Vector3` (one rounding to f32), mode through `int`, the bits through
//! `is_crouched`, `is_sliding` and `is_wall_running`. The counters are the tick's increments (the
//! derive only asks whether they changed), so the before counters are zero.

use crate::{
    BIT_CROUCHED, BIT_SLIDING, BIT_WALL_RUNNING, IN_A_BITS, IN_A_DASHES, IN_A_LAUNCHES, IN_A_MODE,
    IN_A_VY, IN_A_WALL_JUMPS, IN_A_Y, IN_B_BITS, IN_B_DASHES, IN_B_LAUNCHES, IN_B_MODE, IN_B_VY,
    IN_B_WALL_JUMPS, IN_B_Y, IN_VIEW_TURN,
};

/// `GSFpsState` columns read here (gs-player-controller's `GSFpsStateColumns`).
pub const COL_Y: usize = 1;
pub const COL_VY: usize = 4;
pub const COL_MODE: usize = 9;
pub const COL_SLIDE_TIME: usize = 17;
pub const COL_WALL_RUN_TIME: usize = 22;
pub const COL_CROUCH: usize = 30;
/// Columns a state has.
pub const COLS: usize = 32;

/// Values of the motor's answer read here (`GSFpsMotor`'s 45): flags, view turn, launches, dashes,
/// wall jumps.
pub const OUT_FLAGS: usize = 12;
pub const OUT_VIEW_TURN: usize = 39;
pub const OUT_LAUNCHES: usize = 41;
pub const OUT_DASHES: usize = 42;
pub const OUT_WALL_JUMPS: usize = 44;
/// Answer values a row needs.
pub const OUT_LEN: usize = 45;
/// The answer's flag saying the counters counted (else only the view turn is the tick's).
pub const FLAG_COUNTED: i64 = 32;

/// Mode, height, vertical velocity and bits of a state.
pub type Half = [f64; 4];
/// [`Half`] after the tick, then launches, dashes, wall jumps and view turn.
pub type After = [f64; 8];

/// The state part of a row from `cols` (a state's columns); `None` when they are short.
pub fn before(cols: &[f64]) -> Option<Half> {
    (cols.len() >= COLS).then(|| {
        let bit = |on: bool, b: i64| if on { b } else { 0 };
        let bits = bit(cols[COL_CROUCH] > 0.001, BIT_CROUCHED)
            | bit(cols[COL_SLIDE_TIME] >= 0.0, BIT_SLIDING)
            | bit(cols[COL_WALL_RUN_TIME] >= 0.0, BIT_WALL_RUNNING);
        [
            f64::from(cols[COL_MODE] as i64 as i32),
            f64::from(cols[COL_Y] as f32),
            f64::from(cols[COL_VY] as f32),
            bits as f64,
        ]
    })
}

/// The after part from the row's new columns and the motor's answer `out`; `None` when either is short.
pub fn after(cols: &[f64], out: &[f64]) -> Option<After> {
    let h = before(cols)?;
    if out.len() < OUT_LEN {
        return None;
    }
    let counted = out[OUT_FLAGS] as i64 & FLAG_COUNTED != 0;
    let count = |i: usize| if counted { out[i] as i64 as f64 } else { 0.0 };
    Some([
        h[0],
        h[1],
        h[2],
        h[3],
        count(OUT_LAUNCHES),
        count(OUT_DASHES),
        count(OUT_WALL_JUMPS),
        out[OUT_VIEW_TURN],
    ])
}

/// Writes `b` and `a` into an input row (its entity, flags and step height stay).
pub fn fill(row: &mut [f64], b: &Half, a: &After) {
    row[IN_B_MODE] = b[0];
    row[IN_B_Y] = b[1];
    row[IN_B_VY] = b[2];
    row[IN_B_BITS] = b[3];
    row[IN_B_LAUNCHES] = 0.0;
    row[IN_B_DASHES] = 0.0;
    row[IN_B_WALL_JUMPS] = 0.0;
    row[IN_A_MODE] = a[0];
    row[IN_A_Y] = a[1];
    row[IN_A_VY] = a[2];
    row[IN_A_BITS] = a[3];
    row[IN_A_LAUNCHES] = a[4];
    row[IN_A_DASHES] = a[5];
    row[IN_A_WALL_JUMPS] = a[6];
    row[IN_VIEW_TURN] = a[7];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventTable, IN, IN_ENTITY, IN_STEP_HEIGHT, kind};

    fn cols(mode: f64, y: f64, vy: f64) -> [f64; COLS] {
        let mut c = [0.0; COLS];
        c[COL_MODE] = mode;
        c[COL_Y] = y;
        c[COL_VY] = vy;
        c[COL_SLIDE_TIME] = -1.0;
        c[COL_WALL_RUN_TIME] = -1.0;
        c
    }

    #[test]
    fn rounds_through_f32_and_reads_bits() {
        let mut c = cols(1.0, 0.1, -3.3);
        c[COL_CROUCH] = 0.5;
        c[COL_SLIDE_TIME] = 0.0;
        let b = before(&c).unwrap();
        assert_eq!(b[1], f64::from(0.1f32));
        assert_eq!(b[2], f64::from(-3.3f32));
        assert_eq!(b[3] as i64, BIT_CROUCHED | BIT_SLIDING);
        assert!(before(&c[..10]).is_none());
    }

    #[test]
    fn counters_only_when_counted() {
        let c = cols(0.0, 0.0, 0.0);
        let mut out = [0.0; OUT_LEN];
        out[OUT_LAUNCHES] = 1.0;
        out[OUT_VIEW_TURN] = 0.25;
        let a = after(&c, &out).unwrap();
        assert_eq!((a[4], a[7]), (0.0, 0.25));
        out[OUT_FLAGS] = FLAG_COUNTED as f64;
        assert_eq!(after(&c, &out).unwrap()[4], 1.0);
    }

    #[test]
    fn filled_row_derives_landing_and_launch() {
        let mut out = [0.0; OUT_LEN];
        out[OUT_FLAGS] = FLAG_COUNTED as f64;
        out[OUT_LAUNCHES] = 1.0;
        let b = before(&cols(1.0, 2.0, -5.0)).unwrap();
        let a = after(&cols(0.0, 1.5, 0.0), &out).unwrap();
        let mut row = [0.0; IN];
        row[IN_ENTITY] = 3.0;
        row[IN_STEP_HEIGHT] = 0.35;
        fill(&mut row, &b, &a);
        let mut t = EventTable::default();
        t.derive_row(&row);
        let k: Vec<i32> = t.ints.chunks(4).map(|c| c[1]).collect();
        assert_eq!(k, vec![kind::MODE_CHANGED, kind::LANDED, kind::LAUNCHED]);
        assert_eq!(t.floats[2], 5.0);
    }
}
