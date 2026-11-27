//! A bot world's per-tick rows read off the contact table, after the contacts are filled: the
//! think tiers' rows (the live people's eyes and views, the live bots' ids and positions) and the
//! near grid's cells (the live rows by ground cell). The same values, in the same order, as FC's
//! script `FcBotContacts.tier_rows` and `World._near_refresh` read from a `GSBotContactTable`.

use std::collections::HashMap;

/// Offset of a cell coordinate in a [`near_key`], and the key's row width.
const NEAR_OFFSET: i64 = 4096;
const NEAR_WIDTH: i64 = 8192;

/// gs-bot's `GSBotIntent.direction`: the unit view of a yaw and pitch in degrees, worked in f64 as
/// the script's `deg_to_rad`, `sin` and `cos` are, then stored in a `Vector3`'s f32s.
pub fn direction(yaw: f64, pitch: f64) -> [f32; 3] {
    let y = yaw * (std::f64::consts::PI / 180.0);
    let p = pitch * (std::f64::consts::PI / 180.0);
    [
        (-y.sin() * p.cos()) as f32,
        p.sin() as f32,
        (-y.cos() * p.cos()) as f32,
    ]
}

/// FC's `near_key(x, z, cell)`: `(int(floorf(x / cell)) + 4096) * 8192 + int(floorf(z / cell)) + 4096`.
pub fn near_key(x: f32, z: f32, cell: f64) -> i64 {
    ((f64::from(x) / cell).floor() as i64 + NEAR_OFFSET) * NEAR_WIDTH
        + (f64::from(z) / cell).floor() as i64
        + NEAR_OFFSET
}

/// The think tiers' rows: the live people's eyes and views, the live bots' ids and positions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TierRows {
    pub eyes: Vec<[f32; 3]>,
    pub looks: Vec<[f32; 3]>,
    pub ids: Vec<i64>,
    pub at: Vec<[f32; 3]>,
}

/// The rows of one contact table, as columns.
#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    pub ids: &'a [i64],
    /// Each row's flags; [`crate::ROW_ALIVE`] marks the live ones.
    pub flags: &'a [i32],
    pub positions: &'a [[f32; 3]],
    pub eyes: &'a [[f32; 3]],
    /// [`crate::ROW_FLOATS`] per row: eye and aim heights, view yaw and pitch, health.
    pub floats: &'a [f64],
}

impl Table<'_> {
    /// Rows in the table, or None when the columns differ in length.
    pub fn size(&self) -> Option<usize> {
        let n = self.ids.len();
        let whole = self.flags.len() == n
            && self.positions.len() == n
            && self.eyes.len() == n
            && self.floats.len() == n * crate::ROW_FLOATS;
        whole.then_some(n)
    }

    fn alive(&self, i: usize) -> bool {
        self.flags[i] & crate::ROW_ALIVE != 0
    }
}

/// Fills `out` with `t`'s tier rows: a live row whose id is in `bots` (sorted) is a bot (its id
/// and position), any other live row a person (its eye and view direction).
pub fn tier_rows(t: &Table, bots: &[i64], out: &mut TierRows) {
    out.eyes.clear();
    out.looks.clear();
    out.ids.clear();
    out.at.clear();
    let n = t.ids.len();
    for i in 0..n {
        if !t.alive(i) {
            continue;
        }
        let id = t.ids[i];
        if bots.binary_search(&id).is_ok() {
            out.ids.push(id);
            out.at.push(t.positions[i]);
        } else {
            let f = &t.floats[i * crate::ROW_FLOATS..];
            out.eyes.push(t.eyes[i]);
            out.looks.push(direction(f[2], f[3]));
        }
    }
}

/// The near grid's cells: [`Near::keys`] in the order a row first fell in them, and cell `k`'s
/// rows `rows[starts[k]..starts[k + 1]]`, in row order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Near {
    pub keys: Vec<i64>,
    pub starts: Vec<i32>,
    pub rows: Vec<i32>,
}

/// Fills `out` with `t`'s live rows by the ground cell of side `cell` they stand in.
pub fn near_cells(t: &Table, cell: f64, out: &mut Near) {
    out.keys.clear();
    out.starts.clear();
    out.rows.clear();
    let n = t.ids.len();
    let mut slot_of: HashMap<i64, usize> = HashMap::with_capacity(n);
    let mut row_slots: Vec<(usize, i32)> = Vec::with_capacity(n);
    for i in 0..n {
        if !t.alive(i) {
            continue;
        }
        let at = t.positions[i];
        let k = near_key(at[0], at[2], cell);
        let next = out.keys.len();
        let slot = *slot_of.entry(k).or_insert(next);
        if slot == next {
            out.keys.push(k);
        }
        row_slots.push((slot, i as i32));
    }
    let mut counts = vec![0i32; out.keys.len() + 1];
    for &(slot, _) in &row_slots {
        counts[slot + 1] += 1;
    }
    for k in 0..out.keys.len() {
        counts[k + 1] += counts[k];
    }
    out.starts.extend_from_slice(&counts);
    out.rows.resize(row_slots.len(), 0);
    for (slot, row) in row_slots {
        out.rows[counts[slot] as usize] = row;
        counts[slot] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ROW_ALIVE, ROW_FLOATS};

    struct Owned {
        ids: Vec<i64>,
        flags: Vec<i32>,
        positions: Vec<[f32; 3]>,
        eyes: Vec<[f32; 3]>,
        floats: Vec<f64>,
    }

    impl Owned {
        fn table(&self) -> Table<'_> {
            Table {
                ids: &self.ids,
                flags: &self.flags,
                positions: &self.positions,
                eyes: &self.eyes,
                floats: &self.floats,
            }
        }
    }

    fn rows() -> Owned {
        let at = [
            [1.5, 0.0, 2.5],
            [-3.0, 1.0, 9.0],
            [1.9, 0.0, 2.1],
            [40.0, 0.0, -7.0],
            [1.0, 0.0, 2.0],
        ];
        let mut floats = Vec::new();
        for i in 0..at.len() {
            floats.extend_from_slice(&[1.6, 1.2, 30.0 * i as f64, -10.0 + i as f64, 1.0]);
        }
        Owned {
            ids: vec![100, 7, 101, 8, 102],
            flags: vec![ROW_ALIVE, ROW_ALIVE | 2, 0, ROW_ALIVE, ROW_ALIVE],
            positions: at.to_vec(),
            eyes: at.iter().map(|p| [p[0], p[1] + 1.6, p[2]]).collect(),
            floats,
        }
    }

    #[test]
    fn direction_matches_the_script_formula() {
        assert_eq!(direction(0.0, 0.0), [-0.0, 0.0, -1.0]);
        let d = direction(90.0, 0.0);
        assert!((d[0] + 1.0).abs() < 1e-6 && d[2].abs() < 1e-6);
        let d = direction(0.0, 90.0);
        assert!((d[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn near_key_floors_both_axes() {
        assert_eq!(near_key(0.5, 0.5, 1.0), 4096 * 8192 + 4096);
        assert_eq!(near_key(-0.5, 2.5, 1.0), 4095 * 8192 + 4098);
        assert_eq!(near_key(9.0, -9.0, 4.0), 4098 * 8192 + 4093);
    }

    #[test]
    fn tier_rows_split_live_bots_from_people() {
        let o = rows();
        let t = o.table();
        assert_eq!(t.size(), Some(5));
        let mut out = TierRows::default();
        tier_rows(&t, &[100, 101, 102], &mut out);
        assert_eq!(out.ids, vec![100, 102]);
        assert_eq!(out.at, vec![o.positions[0], o.positions[4]]);
        assert_eq!(out.eyes, vec![o.eyes[1], o.eyes[3]]);
        let f1 = &o.floats[ROW_FLOATS..];
        let f3 = &o.floats[3 * ROW_FLOATS..];
        assert_eq!(
            out.looks,
            vec![direction(f1[2], f1[3]), direction(f3[2], f3[3])]
        );
    }

    #[test]
    fn near_cells_group_live_rows_in_first_seen_order() {
        let o = rows();
        let mut out = Near::default();
        near_cells(&o.table(), 4.0, &mut out);
        // Rows 0 and 4 share a cell (row 2 is dead), 1 and 3 have their own.
        assert_eq!(
            out.keys,
            vec![
                near_key(1.5, 2.5, 4.0),
                near_key(-3.0, 9.0, 4.0),
                near_key(40.0, -7.0, 4.0)
            ]
        );
        assert_eq!(out.starts, vec![0, 2, 3, 4]);
        assert_eq!(out.rows, vec![0, 4, 1, 3]);
    }

    #[test]
    fn a_ragged_table_has_no_size() {
        let mut o = rows();
        o.flags.pop();
        assert_eq!(o.table().size(), None);
    }
}
