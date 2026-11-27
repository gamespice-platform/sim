//! Every player's net pull in one pass: `net_pack::pull` (movement, with the sub-quantum hold) and
//! `weapon_net::pull` (the rig's weapon integers) over flat per-player rows, so a server pulls all of its
//! players with one native call a tick instead of two per player. Same integers as the per-player calls,
//! bit for bit; those and the scripts stay the specification.

use crate::cols::STRIDE;
use crate::net_pack::{Extra, Held, Wire, pull as move_pull};
use crate::weapon_net::{WeaponIn, pull as weapon_pull};

/// A movement row is the state's columns (`GSFpsState._f`, at least [constant STRIDE]; a controller may carry
/// more, which are skipped), then these [constant MOVE_EXTRA]: `[crouch_held, stamina, stamina_exhausted,
/// stamina_delay_left, pitch, held_velocity x, y, z, held_yaw, held_pitch, max_speed, velocity_bits, yaw_bits,
/// pitch_bits]`. Booleans are 0 or 1.
pub const MOVE_EXTRA: usize = 14;
/// A weapon row: `[current_slot, has_slot, fire_mode_index, magazine, reserve, fire_seq, fire_kind,
/// charge_shown, windup]`.
pub const WEAPON_STRIDE: usize = 9;
/// An output row: `[net_flags, net_timers2, hold, net_slot, net_fire_mode, net_magazine, net_reserve,
/// net_fire_seq, net_fire_kind, net_charge, net_weapon]`.
pub const OUT_STRIDE: usize = 11;

/// How many whole players [param moves] (rows of [param columns] state columns then [constant MOVE_EXTRA]),
/// [param weapons] and [param weapon_codes] all have rows for; none when [param columns] is under [constant STRIDE].
pub fn players(moves: &[f64], columns: usize, weapons: &[f64], weapon_codes: &[i64]) -> usize {
    if columns < STRIDE {
        return 0;
    }
    (moves.len() / (columns + MOVE_EXTRA))
        .min(weapons.len() / WEAPON_STRIDE)
        .min(weapon_codes.len())
}

/// One player's output row from his movement row ([param columns] state columns, at least [constant STRIDE],
/// then [constant MOVE_EXTRA]), weapon row and held weapon's wire code.
pub fn pull_one(
    m: &[f64],
    columns: usize,
    w: &[f64],
    timer_scale: f64,
    weapon_code: i64,
) -> [i64; OUT_STRIDE] {
    let x = &m[columns..];
    let extra = Extra {
        crouch_held: x[0] != 0.0,
        timer_scale,
        stamina: x[1],
        stamina_exhausted: x[2] != 0.0,
        stamina_delay_left: x[3],
    };
    let held = Held {
        velocity: [x[5] as f32, x[6] as f32, x[7] as f32],
        yaw: x[8],
        pitch: x[9],
    };
    let wire = Wire {
        speed: x[10],
        velocity_bits: x[11] as i64,
        yaw_bits: x[12] as i64,
        pitch_bits: x[13] as i64,
    };
    let mv = move_pull(&m[..STRIDE], &extra, x[4], &held, &wire);
    let wi = WeaponIn {
        current_slot: w[0] as i64,
        has_slot: w[1] != 0.0,
        fire_mode_index: w[2] as i64,
        magazine: w[3] as i64,
        reserve: w[4] as i64,
        fire_seq: w[5] as i64,
        fire_kind: w[6] as i64,
        charge_shown: w[7],
        windup: w[8],
    };
    let [slot, mode, mag, reserve, seq, kind, charge] = weapon_pull(&wi);
    [
        mv[0],
        mv[2],
        mv[4],
        slot,
        mode,
        mag,
        reserve,
        seq,
        kind,
        charge,
        weapon_code,
    ]
}

/// Floats per player in the apply row a caller hands [fn applied] beside the movement row: `[crouch_fraction,
/// modifiers, fitted, reloading, switching]` (the packed modifiers and the fitted code as whole numbers, booleans
/// 0 or 1): the replicated fields the pull copies but the motor rows do not carry.
pub const APPLY_EXTRA: usize = 5;

/// The movement and weapon fields one pull writes on a player's net object, from his movement row, apply row
/// ([constant APPLY_EXTRA]) and output row: what the game's script apply writes, value for value. A `None` is a
/// field the sub-quantum hold keeps (left as it was).
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    /// `Vector3(f[0], f[1], f[2])`: the columns rounded to the float32 a `Vector3` holds.
    pub position: [f32; 3],
    pub velocity: Option<[f32; 3]>,
    pub yaw: Option<f64>,
    pub pitch: Option<f64>,
    pub crouch: f64,
    pub modifiers: i64,
    pub fitted: i64,
    pub reloading: bool,
    pub switching: bool,
}

/// What [fn pull_one]'s output row [param out] writes for the player of movement row [param m] ([param columns]
/// state columns then [constant MOVE_EXTRA]) and apply row [param a].
pub fn applied(m: &[f64], columns: usize, a: &[f64], out: &[i64]) -> Applied {
    let hold = out[2];
    Applied {
        position: [m[0] as f32, m[1] as f32, m[2] as f32],
        velocity: (hold & 1 == 0).then(|| [m[3] as f32, m[4] as f32, m[5] as f32]),
        yaw: (hold & 2 == 0).then(|| m[12]),
        pitch: (hold & 4 == 0).then(|| m[columns + 4]),
        crouch: a[0],
        modifiers: a[1] as i64,
        fitted: a[2] as i64,
        reloading: a[3] != 0.0,
        switching: a[4] != 0.0,
    }
}

/// Every player's output row, [constant OUT_STRIDE] each, in row order (see [fn players]).
pub fn pull_all(
    moves: &[f64],
    columns: usize,
    weapons: &[f64],
    timer_scale: f64,
    weapon_codes: &[i64],
) -> Vec<i64> {
    let n = players(moves, columns, weapons, weapon_codes);
    let stride = columns + MOVE_EXTRA;
    let mut out = Vec::with_capacity(n * OUT_STRIDE);
    for i in 0..n {
        out.extend_from_slice(&pull_one(
            &moves[i * stride..(i + 1) * stride],
            columns,
            &weapons[i * WEAPON_STRIDE..(i + 1) * WEAPON_STRIDE],
            timer_scale,
            weapon_codes[i],
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A controller carrying two columns past gs-sim's own, as gs-player-controller's state does.
    const COLUMNS: usize = STRIDE + 2;

    fn rows(seed: u64) -> (Vec<f64>, Vec<f64>) {
        let mut s = seed;
        let mut next = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 33) as f64) / ((1u64 << 31) as f64)
        };
        let mut m: Vec<f64> = (0..COLUMNS).map(|_| next() * 40.0 - 20.0).collect();
        m.extend_from_slice(&[
            1.0,
            next() * 100.0,
            0.0,
            next(),
            next() * 180.0 - 90.0,
            (next() * 10.0) as f32 as f64,
            0.0,
            -1.5,
            next() * 360.0,
            0.0,
            10.0,
            16.0,
            12.0,
            10.0,
        ]);
        let w = vec![2.0, 1.0, 1.0, 30.0, 90.0, 37.0, 1.0, 0.4, -1.0];
        (m, w)
    }

    #[test]
    fn batch_matches_the_per_player_calls() {
        let mut moves = Vec::new();
        let mut weapons = Vec::new();
        let mut codes = Vec::new();
        for p in 0..9u64 {
            let (m, w) = rows(p * 7919 + 3);
            moves.extend_from_slice(&m);
            weapons.extend_from_slice(&w);
            codes.push(p as i64);
        }
        let out = pull_all(&moves, COLUMNS, &weapons, 0.5, &codes);
        assert_eq!(out.len(), 9 * OUT_STRIDE);
        for p in 0..9usize {
            let stride = COLUMNS + MOVE_EXTRA;
            let m = &moves[p * stride..(p + 1) * stride];
            let x = &m[COLUMNS..];
            let extra = Extra {
                crouch_held: true,
                timer_scale: 0.5,
                stamina: x[1],
                stamina_exhausted: false,
                stamina_delay_left: x[3],
            };
            let held = Held {
                velocity: [x[5] as f32, 0.0, -1.5],
                yaw: x[8],
                pitch: 0.0,
            };
            let wire = Wire {
                speed: 10.0,
                velocity_bits: 16,
                yaw_bits: 12,
                pitch_bits: 10,
            };
            let mv = move_pull(&m[..STRIDE], &extra, x[4], &held, &wire);
            let wp = weapon_pull(&WeaponIn {
                current_slot: 2,
                has_slot: true,
                fire_mode_index: 1,
                magazine: 30,
                reserve: 90,
                fire_seq: 37,
                fire_kind: 1,
                charge_shown: 0.4,
                windup: -1.0,
            });
            let row = &out[p * OUT_STRIDE..(p + 1) * OUT_STRIDE];
            assert_eq!(&row[..3], &[mv[0], mv[2], mv[4]]);
            assert_eq!(&row[3..10], &wp[..]);
            assert_eq!(row[10], p as i64);
        }
    }

    #[test]
    fn applied_keeps_what_the_hold_keeps() {
        let (m, _) = rows(5);
        let a = [0.25, 6.0, 3.0, 1.0, 0.0];
        let mut out = [0i64; OUT_STRIDE];
        let all = applied(&m, COLUMNS, &a, &out);
        assert_eq!(all.position, [m[0] as f32, m[1] as f32, m[2] as f32]);
        assert_eq!(all.velocity, Some([m[3] as f32, m[4] as f32, m[5] as f32]));
        assert_eq!(all.yaw, Some(m[12]));
        assert_eq!(all.pitch, Some(m[COLUMNS + 4]));
        assert_eq!((all.crouch, all.modifiers, all.fitted), (0.25, 6, 3));
        assert!(all.reloading && !all.switching);
        out[2] = 7;
        let held = applied(&m, COLUMNS, &a, &out);
        assert_eq!((held.velocity, held.yaw, held.pitch), (None, None, None));
        assert_eq!(held.position, all.position);
    }

    #[test]
    fn short_rows_pull_only_whole_players() {
        let (m, w) = rows(1);
        assert_eq!(players(&m[..m.len() - 1], COLUMNS, &w, &[0]), 0);
        assert_eq!(players(&m, COLUMNS, &w[..WEAPON_STRIDE - 1], &[0]), 0);
        assert_eq!(players(&m, COLUMNS, &w, &[]), 0);
        assert_eq!(players(&m, STRIDE - 1, &w, &[0]), 0);
        assert_eq!(pull_all(&m, COLUMNS, &w, 1.0, &[4]).len(), OUT_STRIDE);
    }
}
