//! A player's motor values as gs-player-controller 36's `GSFpsState` keeps them: one packed
//! float64 array per state (`GSFpsStateColumns`), so a batch takes that array and a short
//! per-player input instead of a 55-value row the script copies field by field.
//!
//! [`fill_row`] builds the whole tick's input row from them (the script's `_fill_full_in`), and
//! [`apply_out`] writes the tick's answer into a copy of the columns as `_native_full_apply` and
//! `batch_apply` write it through the state's properties: vectors rounded to the float32 a
//! `Vector3` holds, the mode and ground id through `int`, the clocks as they are.

/// Values in a state's columns (`GSFpsStateColumns.STRIDE`).
pub const STRIDE: usize = 30;
/// Where the clock block starts in the columns (`GSFpsStateColumns.CLOCKS`), and its length.
pub const CLOCKS: usize = 14;
pub const CLOCK_COUNT: usize = 16;
/// Where the previous buttons sit in the columns.
pub const PREVIOUS_BUTTONS: usize = 13;
/// Where the wall normal sits in the columns (the clock block's last three values).
pub const WALL_NORMAL: usize = 27;

/// Per-player input beside the columns: the row's values 12..38 as the script fills them (height,
/// rest ok, rest position, delta, buttons, previous buttons, move, yaw, then the 16 effect and
/// tunable values), so one fill serves the row and the columns path.
pub const EXTRA_LEN: usize = 27;
/// Where the extra values sit in the row, and where the command's buttons sit in them.
const ROW_EXTRA: usize = 12;
const EXTRA_BUTTONS: usize = 6;
/// Values in the whole tick's input row and its answer.
pub const ROW_IN: usize = 55;
pub const ROW_OUT: usize = 45;

/// Row offsets: where the clock block starts, input and output.
const ROW_CLOCKS: usize = 39;
const OUT_CLOCKS: usize = 23;
/// Clock values the answer gives as they are (the wall normal after them is rounded).
const OUT_CLOCK_SCALARS: usize = 13;

/// The whole tick's input row from a state's columns and its extra values. `row` is [`ROW_IN`]
/// long, `cols` at least [`STRIDE`], `extra` at least [`EXTRA_LEN`].
pub fn fill_row(cols: &[f64], extra: &[f64], row: &mut [f64]) {
    row[..ROW_EXTRA].copy_from_slice(&cols[..ROW_EXTRA]);
    row[ROW_EXTRA..ROW_CLOCKS].copy_from_slice(&extra[..EXTRA_LEN]);
    row[ROW_CLOCKS..ROW_IN].copy_from_slice(&cols[CLOCKS..CLOCKS + CLOCK_COUNT]);
}

/// GDScript's `int(x)` of a float: truncation toward zero.
fn gd_int(x: f64) -> i64 {
    x as i64
}

/// The state's columns after the tick: `cols` with the answer `out` ([`ROW_OUT`] values) written
/// as the script writes it, and the previous buttons set to the command's (`extra`'s buttons).
/// `new` is as long as `cols`; values past [`STRIDE`] are kept.
pub fn apply_out(cols: &[f64], extra: &[f64], out: &[f64], new: &mut [f64]) {
    new.copy_from_slice(cols);
    // Position, velocity, ground normal: through Vector3, so float32.
    for (n, &o) in new[..9].iter_mut().zip(&out[..9]) {
        *n = o as f32 as f64;
    }
    new[9] = gd_int(out[9]) as f64;
    let id = gd_int(out[10]) | gd_int(out[11]).wrapping_shl(32);
    new[10] = (id & 0xFFFF_FFFF) as f64;
    new[11] = (id >> 32) as f64;
    new[CLOCKS..CLOCKS + OUT_CLOCK_SCALARS]
        .copy_from_slice(&out[OUT_CLOCKS..OUT_CLOCKS + OUT_CLOCK_SCALARS]);
    let wall = &out[OUT_CLOCKS + OUT_CLOCK_SCALARS..OUT_CLOCKS + OUT_CLOCK_SCALARS + 3];
    for (n, &o) in new[WALL_NORMAL..WALL_NORMAL + 3].iter_mut().zip(wall) {
        *n = o as f32 as f64;
    }
    new[PREVIOUS_BUTTONS] = gd_int(extra[EXTRA_BUTTONS]) as f64;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<f64> {
        (0..STRIDE).map(|i| 100.0 + i as f64).collect()
    }

    fn extra() -> Vec<f64> {
        (0..EXTRA_LEN).map(|i| 200.0 + i as f64).collect()
    }

    #[test]
    fn row_takes_the_script_order() {
        let (c, e) = (cols(), extra());
        let mut row = vec![0.0; ROW_IN];
        fill_row(&c, &e, &mut row);
        // The script's _fill_full_in, index by index.
        let mut want = vec![0.0; ROW_IN];
        want[..12].copy_from_slice(&c[..12]);
        want[12..39].copy_from_slice(&e);
        want[39..].copy_from_slice(&c[14..30]);
        assert_eq!(row, want);
    }

    #[test]
    fn answer_rounds_like_the_properties() {
        let c = cols();
        let mut e = extra();
        e[6] = 1029.0;
        let mut out: Vec<f64> = (0..ROW_OUT).map(|i| 0.1 + i as f64 / 3.0).collect();
        out[9] = 1.0;
        out[10] = 4_000_000_000.0;
        out[11] = -2.0;
        let mut new = vec![0.0; STRIDE + 2];
        let mut wide = c.clone();
        wide.extend([7.0, 8.0]);
        apply_out(&wide, &e, &out, &mut new);
        for i in 0..9 {
            assert_eq!(new[i], out[i] as f32 as f64);
            assert_ne!(new[i], out[i]);
        }
        assert_eq!(new[9], 1.0);
        let id: i64 = 4_000_000_000 | (-2i64 << 32);
        assert_eq!(new[10], (id & 0xFFFF_FFFF) as f64);
        assert_eq!(new[11], (id >> 32) as f64);
        assert_eq!(new[12], c[12]);
        assert_eq!(new[13], 1029.0);
        for i in 0..13 {
            assert_eq!(new[14 + i], out[23 + i]);
        }
        for i in 0..3 {
            assert_eq!(new[27 + i], out[36 + i] as f32 as f64);
        }
        assert_eq!(&new[STRIDE..], &[7.0, 8.0]);
    }
}
