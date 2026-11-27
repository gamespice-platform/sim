//! The server tick's player table (FC native server tick, phase P1): one struct-of-arrays row per
//! roster slot, so a native phase reads every player's origin, velocity, posture, health and
//! weapon from contiguous columns instead of asking the script per player and field.
//!
//! A tick fills it twice. [`Table::begin`] takes what the script owns, once per tick in one packed
//! array ([`BEGIN`] floats per row: id, motor slot, alive, health, weapon). [`Table::take_motor`]
//! and [`Table::take_feet`] then refresh the motor's part after the players' loop from the
//! resident motor rows (`GSFpsState` columns, [`COL_POSITION`] and on) and the hull world's feet.
//! [`Table::export`] hands it back as [`STRIDE`] floats per row.

/// Floats per row of [`Table::begin`]'s input.
pub const BEGIN: usize = 5;
/// Floats per row of [`Table::export`]'s output.
pub const STRIDE: usize = 13;

/// Output columns: the player id.
pub const OUT_ID: usize = 0;
/// Origin x, y, z (the feet).
pub const OUT_ORIGIN: usize = 1;
/// Velocity x, y, z.
pub const OUT_VELOCITY: usize = 4;
/// `GSFpsState.mode`.
pub const OUT_MODE: usize = 7;
/// `GSFpsState.crouch_fraction` (the ducking).
pub const OUT_DUCKING: usize = 8;
/// The row's [`FLAG_ALIVE`] and the other flags.
pub const OUT_FLAGS: usize = 9;
/// Health as the script last handed it.
pub const OUT_HEALTH: usize = 10;
/// Weapon state as the script last handed it (the slot in hand).
pub const OUT_WEAPON: usize = 11;
/// The resident motor row, or -1.
pub const OUT_SLOT: usize = 12;

/// The player is alive.
pub const FLAG_ALIVE: u32 = 1;
/// Origin, velocity, mode and ducking came from the resident motor row this tick.
pub const FLAG_MOTOR: u32 = 2;
/// The origin came from the hull world's feet this tick.
pub const FLAG_FEET: u32 = 4;

/// `GSFpsState` column indices (gs-player-controller's `gs_fps_state_columns.gd`).
pub const COL_POSITION: usize = 0;
pub const COL_VELOCITY: usize = 3;
pub const COL_MODE: usize = 9;
pub const COL_CROUCH: usize = 30;
/// Columns a resident motor row holds.
pub const COL_STRIDE: usize = 32;

/// The player table, one entry per roster slot in each column.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Table {
    pub id: Vec<i64>,
    pub slot: Vec<i64>,
    pub origin: Vec<[f64; 3]>,
    pub velocity: Vec<[f64; 3]>,
    pub mode: Vec<i64>,
    pub ducking: Vec<f64>,
    pub flags: Vec<u32>,
    pub health: Vec<f64>,
    pub weapon: Vec<i64>,
}

impl Table {
    /// Rows held.
    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    /// Starts a tick from the script's rows ([`BEGIN`] floats each: id, motor slot or -1, alive 0
    /// or 1, health, weapon). Origin, velocity, mode and ducking keep last tick's values for rows
    /// whose id stayed in place, else zero, until [`Self::take_motor`] refreshes them.
    pub fn begin(&mut self, rows: &[f64]) {
        let n = rows.len() / BEGIN;
        let keep: Vec<bool> = (0..n)
            .map(|i| self.id.get(i) == Some(&(rows[i * BEGIN] as i64)))
            .collect();
        self.resize(n);
        for (i, &kept) in keep.iter().enumerate() {
            let r = &rows[i * BEGIN..(i + 1) * BEGIN];
            self.id[i] = r[0] as i64;
            self.slot[i] = r[1] as i64;
            self.flags[i] = if r[2] != 0.0 { FLAG_ALIVE } else { 0 };
            self.health[i] = r[3];
            self.weapon[i] = r[4] as i64;
            if !kept {
                self.origin[i] = [0.0; 3];
                self.velocity[i] = [0.0; 3];
                self.mode[i] = 0;
                self.ducking[i] = 0.0;
            }
        }
    }

    fn resize(&mut self, n: usize) {
        self.id.resize(n, 0);
        self.slot.resize(n, -1);
        self.origin.resize(n, [0.0; 3]);
        self.velocity.resize(n, [0.0; 3]);
        self.mode.resize(n, 0);
        self.ducking.resize(n, 0.0);
        self.flags.resize(n, 0);
        self.health.resize(n, 0.0);
        self.weapon.resize(n, 0);
    }

    /// Row `i`'s motor part from its resident motor row's columns (`cols`, at least
    /// [`COL_STRIDE`] long); false (row untouched) when the columns are short.
    pub fn take_motor(&mut self, i: usize, cols: &[f64]) -> bool {
        if i >= self.len() || cols.len() < COL_STRIDE {
            return false;
        }
        let p = COL_POSITION;
        let v = COL_VELOCITY;
        self.origin[i] = [cols[p], cols[p + 1], cols[p + 2]];
        self.velocity[i] = [cols[v], cols[v + 1], cols[v + 2]];
        self.mode[i] = cols[COL_MODE] as i64;
        self.ducking[i] = cols[COL_CROUCH];
        self.flags[i] |= FLAG_MOTOR;
        true
    }

    /// Row `i`'s origin from the hull world's feet (where the player ended its whole tick, after
    /// anything that moved it past its motor).
    pub fn take_feet(&mut self, i: usize, feet: [f64; 3]) {
        if i < self.len() {
            self.origin[i] = feet;
            self.flags[i] |= FLAG_FEET;
        }
    }

    /// The table as [`STRIDE`] floats per row into `out` (cleared first).
    pub fn export(&self, out: &mut Vec<f64>) {
        out.clear();
        out.reserve(self.len() * STRIDE);
        for i in 0..self.len() {
            let (o, v) = (self.origin[i], self.velocity[i]);
            out.extend_from_slice(&[
                self.id[i] as f64,
                o[0],
                o[1],
                o[2],
                v[0],
                v[1],
                v[2],
                self.mode[i] as f64,
                self.ducking[i],
                f64::from(self.flags[i]),
                self.health[i],
                self.weapon[i] as f64,
                self.slot[i] as f64,
            ]);
        }
    }

    /// FNV-1a over every exported value's bits: two tables digest alike only when equal to the bit.
    pub fn digest(&self) -> u64 {
        let mut out = Vec::new();
        self.export(&mut out);
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in out {
            for b in x.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(pos: [f64; 3], vel: [f64; 3], mode: f64, crouch: f64) -> Vec<f64> {
        let mut c = vec![0.0; COL_STRIDE];
        c[COL_POSITION..COL_POSITION + 3].copy_from_slice(&pos);
        c[COL_VELOCITY..COL_VELOCITY + 3].copy_from_slice(&vel);
        c[COL_MODE] = mode;
        c[COL_CROUCH] = crouch;
        c
    }

    #[test]
    fn begin_takes_the_script_rows() {
        let mut t = Table::default();
        t.begin(&[7.0, 2.0, 1.0, 100.0, 3.0, 9.0, -1.0, 0.0, 0.0, 0.0]);
        assert_eq!(t.len(), 2);
        assert_eq!(t.id, vec![7, 9]);
        assert_eq!(t.slot, vec![2, -1]);
        assert_eq!(t.flags, vec![FLAG_ALIVE, 0]);
        assert_eq!(t.health, vec![100.0, 0.0]);
        assert_eq!(t.weapon, vec![3, 0]);
    }

    #[test]
    fn motor_and_feet_fill_the_motor_part() {
        let mut t = Table::default();
        t.begin(&[7.0, 0.0, 1.0, 100.0, 1.0]);
        assert!(t.take_motor(0, &cols([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 2.0, 0.5)));
        assert_eq!(t.origin[0], [1.0, 2.0, 3.0]);
        assert_eq!(t.velocity[0], [4.0, 5.0, 6.0]);
        assert_eq!((t.mode[0], t.ducking[0]), (2, 0.5));
        t.take_feet(0, [1.5, 2.0, 3.0]);
        assert_eq!(t.origin[0], [1.5, 2.0, 3.0]);
        assert_eq!(t.flags[0], FLAG_ALIVE | FLAG_MOTOR | FLAG_FEET);
        assert!(!t.take_motor(0, &[0.0; 4]));
        assert!(!t.take_motor(1, &cols([0.0; 3], [0.0; 3], 0.0, 0.0)));
    }

    #[test]
    fn begin_keeps_a_row_whose_id_stayed() {
        let mut t = Table::default();
        t.begin(&[7.0, 0.0, 1.0, 100.0, 1.0, 8.0, 1.0, 1.0, 50.0, 2.0]);
        t.take_motor(0, &cols([1.0, 2.0, 3.0], [0.0; 3], 0.0, 0.0));
        t.take_motor(1, &cols([4.0, 5.0, 6.0], [0.0; 3], 0.0, 0.0));
        // Slot 0 keeps id 7; slot 1 is now id 9 (the roster changed): its motor part starts over.
        t.begin(&[7.0, 0.0, 1.0, 90.0, 1.0, 9.0, 1.0, 1.0, 50.0, 2.0]);
        assert_eq!(t.origin, vec![[1.0, 2.0, 3.0], [0.0; 3]]);
        assert_eq!(t.flags, vec![FLAG_ALIVE, FLAG_ALIVE]);
        assert_eq!(t.health[0], 90.0);
    }

    #[test]
    fn export_and_digest_follow_every_bit() {
        let mut t = Table::default();
        t.begin(&[7.0, 0.0, 1.0, 100.0, 1.0]);
        t.take_motor(0, &cols([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 2.0, 0.25));
        let mut out = Vec::new();
        t.export(&mut out);
        assert_eq!(out.len(), STRIDE);
        assert_eq!(out[OUT_ID], 7.0);
        assert_eq!(&out[OUT_ORIGIN..OUT_ORIGIN + 3], &[1.0, 2.0, 3.0]);
        assert_eq!(&out[OUT_VELOCITY..OUT_VELOCITY + 3], &[4.0, 5.0, 6.0]);
        assert_eq!(out[OUT_MODE], 2.0);
        assert_eq!(out[OUT_DUCKING], 0.25);
        assert_eq!(out[OUT_FLAGS], f64::from(FLAG_ALIVE | FLAG_MOTOR));
        assert_eq!(out[OUT_HEALTH], 100.0);
        assert_eq!(out[OUT_WEAPON], 1.0);
        assert_eq!(out[OUT_SLOT], 0.0);
        let d = t.digest();
        let mut u = t.clone();
        u.velocity[0][1] = f64::from_bits(5.0f64.to_bits() + 1);
        assert_ne!(u.digest(), d);
        assert_eq!(t.clone().digest(), d);
    }
}
