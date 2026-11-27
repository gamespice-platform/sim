//! FC's player hull grid (`FcHullGrid`), the cells a server's players' loop files every player in so
//! a script hull gather (a lag-compensated mover) only reads the players near it.
//!
//! A cell key is the script's `(int(floorf(x / CELL)) + BIAS) * 8192 + int(floorf(z / CELL)) + BIAS`:
//! the feet are the engine's f32 widened, divided in f64. [`near`] gives the slots, ascending, filed in
//! any cell the square of half side `sqrt(reach_sq)` around the asker touches: the script grid's
//! `near` answer (a superset; the caller's own distance test decides).

/// Metres per cell side (`FcHullGrid.CELL`).
pub const CELL: f64 = 6.0;
/// Added to a cell coordinate so it is positive (`FcHullGrid.BIAS`).
pub const BIAS: i64 = 4096;
/// The key's row stride.
pub const ROW: i64 = 8192;
/// A slot that is not filed (no controller).
pub const UNFILED: i64 = -1;

fn cell(v: f64) -> i64 {
    (v / CELL).floor() as i64
}

/// The key of the cell feet at `x, z` are filed in (`FcHullGrid.key_at`).
pub fn key_at(x: f32, z: f32) -> i64 {
    (cell(x as f64) + BIAS) * ROW + cell(z as f64) + BIAS
}

/// Fills `out` with every slot of `keys` filed in a cell the reach square around `x, z` touches,
/// ascending (`FcHullGrid.near`). A slot whose key is negative is not filed.
pub fn near(keys: &[i64], x: f32, z: f32, reach_sq: f64, out: &mut Vec<i64>) {
    out.clear();
    let reach = reach_sq.sqrt();
    let (x, z) = (x as f64, z as f64);
    let (x0, x1) = (cell(x - reach) + BIAS, cell(x + reach) + BIAS);
    let (z0, z1) = (cell(z - reach) + BIAS, cell(z + reach) + BIAS);
    for (i, &k) in keys.iter().enumerate() {
        if k < 0 {
            continue;
        }
        for cx in x0..=x1 {
            let cz = k - cx * ROW;
            if cz >= z0 && cz <= z1 {
                out.push(i as i64);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_the_script() {
        assert_eq!(key_at(0.0, 0.0), BIAS * ROW + BIAS);
        assert_eq!(key_at(-0.1, 5.99), (BIAS - 1) * ROW + BIAS);
        assert_eq!(key_at(6.0, -6.0), (BIAS + 1) * ROW + BIAS - 1);
    }

    #[test]
    fn near_reads_the_touched_cells_in_slot_order() {
        let keys = [
            key_at(13.0, 0.5),
            UNFILED,
            key_at(1.0, 1.0),
            key_at(-5.0, 2.0),
            key_at(40.0, 40.0),
        ];
        let mut out = Vec::new();
        near(&keys, 3.0, 3.0, 4.0, &mut out);
        assert_eq!(out, vec![2]);
        near(&keys, 1.0, 1.0, 36.0, &mut out);
        assert_eq!(out, vec![2, 3]);
        near(&keys, 6.5, 1.0, 36.0, &mut out);
        assert_eq!(out, vec![0, 2]);
    }
}
