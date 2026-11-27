//! The values game-fatal-contact's `FcPlayerNet._pull_rest` derives from a player each tick
//! (P6d), bit for bit with the script: the owner's exact float32 copy of the movement state
//! (`_pull_exact`), `GSCombatNetSync.pull`'s health and armour, and the challenge clamps. The
//! values the script copies unchanged (booleans, plain integers) need no kernel.

use crate::to_int;

/// Floats in the exact copy (`FcPlayerNet.EXACT_FLOATS`).
pub const EXACT_FLOATS: usize = 14;

/// `GSFpsState._f` columns the exact copy reads (`gs_fps_state_columns.gd`).
pub mod col {
    pub const POSITION: usize = 0;
    pub const VELOCITY: usize = 3;
    pub const TIME_SINCE_JUMP: usize = 14;
    pub const TIME_SINCE_GROUNDED: usize = 15;
    pub const TIME_SINCE_JUMP_PRESSED: usize = 16;
    pub const WALL_RUN_COOLDOWN_LEFT: usize = 21;
    pub const CROUCH_FRACTION: usize = 30;
}

/// The fewest `_f` columns [`exact_row`] reads.
pub const MIN_COLUMNS: usize = col::CROUCH_FRACTION + 1;

/// `_pull_exact`'s fourteen floats from a state's columns [param f] and its three stamina
/// fields, each rounded once to f32 as the script's store into a `PackedFloat32Array` does
/// (the `Vector3` getters round to the same f32 first). None when [param f] is short.
pub fn exact_row(
    f: &[f64],
    stamina: f64,
    exhausted: bool,
    delay_left: f64,
) -> Option<[f32; EXACT_FLOATS]> {
    if f.len() < MIN_COLUMNS {
        return None;
    }
    let (p, v) = (col::POSITION, col::VELOCITY);
    Some([
        f[p] as f32,
        f[p + 1] as f32,
        f[p + 2] as f32,
        f[v] as f32,
        f[v + 1] as f32,
        f[v + 2] as f32,
        f[col::CROUCH_FRACTION] as f32,
        f[col::TIME_SINCE_GROUNDED] as f32,
        f[col::TIME_SINCE_JUMP] as f32,
        f[col::TIME_SINCE_JUMP_PRESSED] as f32,
        f[col::WALL_RUN_COOLDOWN_LEFT] as f32,
        stamina as f32,
        if exhausted { 1.0 } else { 0.0 },
        delay_left as f32,
    ])
}

/// Values `FcNetPullBatch` appends after a state's columns in each move row: crouch held, then
/// stamina, exhausted and the delay left ([`exact_from_moves`] reads those three), then the rest.
pub const MOVE_STAMINA: usize = 1;

/// [`exact_row`] for row [param row] of the batch's move rows ([param columns] state columns then
/// [param extra] values each): the state's `_f` and its stamina fields as `FcNetPullBatch._gather`
/// copied them, so nothing is read from the state again. None when the row is out of range.
pub fn exact_from_moves(
    moves: &[f64],
    row: usize,
    columns: usize,
    extra: usize,
) -> Option<[f32; EXACT_FLOATS]> {
    if extra < MOVE_STAMINA + 3 {
        return None;
    }
    let start = row.checked_mul(columns + extra)?;
    let r = moves.get(start..start + columns + extra)?;
    let s = columns + MOVE_STAMINA;
    exact_row(&r[..columns], r[s], r[s + 1] != 0.0, r[s + 2])
}

/// `GSCombatNetSync.pull`'s `clampi(int(ceil(value)), 0, (1 << bits) - 1)` for health and armour.
pub fn health_wire(value: f64, bits: u32) -> i64 {
    to_int(value.ceil()).clamp(0, (1i64 << bits.min(62)) - 1)
}

/// `clampi(challenge + 1, 0, 127)` (GDScript's integer add wraps).
pub fn challenge_wire(challenge: i64) -> i64 {
    challenge.wrapping_add(1).clamp(0, 127)
}

/// `clampi(challenge_value, 0, 65535)`.
pub fn challenge_value_wire(value: i64) -> i64 {
    value.clamp(0, 65535)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_row_rounds_each_column_once_in_the_script_order() {
        let mut f = vec![0.0f64; 32];
        for (i, x) in f.iter_mut().enumerate() {
            *x = 0.1 + i as f64 * 1.000_000_1;
        }
        let row = exact_row(&f, 0.3, true, 0.7).unwrap();
        let want = [
            f[0], f[1], f[2], f[3], f[4], f[5], f[30], f[15], f[14], f[16], f[21], 0.3, 1.0, 0.7,
        ];
        for (got, w) in row.iter().zip(want) {
            assert_eq!(got.to_bits(), (w as f32).to_bits());
        }
        assert_eq!(exact_row(&f, 1.0, false, 0.0).unwrap()[12], 0.0);
        assert!(exact_row(&f[..30], 1.0, false, 0.0).is_none());
    }

    #[test]
    fn exact_from_moves_reads_the_row_and_its_stamina() {
        let (columns, extra) = (32, 14);
        let mut moves = vec![0.0f64; (columns + extra) * 2];
        for (i, x) in moves.iter_mut().enumerate() {
            *x = 0.25 + i as f64 * 0.5;
        }
        let r = columns + extra;
        moves[r + columns + 2] = 1.0;
        let f = &moves[r..r + columns];
        let want = exact_row(f, moves[r + columns + 1], true, moves[r + columns + 3]).unwrap();
        assert_eq!(exact_from_moves(&moves, 1, columns, extra), Some(want));
        assert!(exact_from_moves(&moves, 2, columns, extra).is_none());
        assert!(exact_from_moves(&moves, 0, 30, 16).is_none());
        assert!(exact_from_moves(&moves, 0, columns, 3).is_none());
    }

    #[test]
    fn health_ceils_and_clamps_like_gdscript() {
        assert_eq!(health_wire(0.4, 11), 1);
        assert_eq!(health_wire(100.0, 11), 100);
        assert_eq!(health_wire(-3.5, 11), 0);
        assert_eq!(health_wire(5000.0, 11), 2047);
        assert_eq!(health_wire(-0.0, 11), 0);
        // int() of a NaN or an out-of-range float is i64::MIN on x86-64, clamped to 0; on the
        // web it saturates (and a NaN is 0).
        assert_eq!(health_wire(f64::NAN, 11), 0);
        #[cfg(not(target_family = "wasm"))]
        assert_eq!(health_wire(f64::INFINITY, 11), 0);
    }

    #[test]
    fn challenge_clamps_and_wraps() {
        assert_eq!(challenge_wire(-1), 0);
        assert_eq!(challenge_wire(0), 1);
        assert_eq!(challenge_wire(500), 127);
        assert_eq!(challenge_wire(i64::MAX), 0);
        assert_eq!(challenge_value_wire(-5), 0);
        assert_eq!(challenge_value_wire(70000), 65535);
        assert_eq!(challenge_value_wire(1234), 1234);
    }
}
