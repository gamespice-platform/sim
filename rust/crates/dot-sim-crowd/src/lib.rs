//! A crowd's positions in a flat spatial hash rebuilt once a tick: the twin of dot-npc-ai's
//! `DotNpcAiCrowdGrid` to the bit. Cells are linked lists in insertion order and every sum is
//! spelled in the script's order and precision (f32 vectors, f64 script floats).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use dot_sim_math::Vec3;

/// A multiply hasher for the grid's `i64` keys; the default SipHash costs more than the lookup.
#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write_i64(&mut self, v: i64) {
        self.0 = (v as u64 ^ (v as u64 >> 29)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

type Map<V> = HashMap<i64, V, BuildHasherDefault<KeyHasher>>;

/// The script's `floori(v / cell)`: the f32 coordinate widened, divided in f64, floored.
#[inline]
fn cell_of(v: f32, cell: f64) -> i64 {
    cell_of_f64(f64::from(v), cell)
}

/// The script's `(cx << 32) ^ (cz & 0xFFFFFFFF)`.
#[inline]
pub fn key(cx: i64, cz: i64) -> i64 {
    cx.wrapping_shl(32) ^ (cz & 0xFFFF_FFFF)
}

/// One tick's crowd: positions, ids and per-cell lists of indices.
pub struct Grid {
    cell: f64,
    pos: Vec<Vec3>,
    ids: Vec<i64>,
    next: Vec<i32>,
    /// Cell key to its first and last index.
    cells: Map<(i32, i32)>,
    index: Map<i32>,
}

impl Default for Grid {
    fn default() -> Self {
        Self::new(2.0)
    }
}

impl Grid {
    /// An empty grid of [`cell`]-metre cells (at least 0.1).
    pub fn new(cell: f64) -> Self {
        Self {
            cell: cell.max(0.1),
            pos: Vec::new(),
            ids: Vec::new(),
            next: Vec::new(),
            cells: Map::default(),
            index: Map::default(),
        }
    }

    pub fn cell(&self) -> f64 {
        self.cell
    }

    /// Empties the grid, keeping its storage.
    pub fn clear(&mut self) {
        self.pos.clear();
        self.ids.clear();
        self.next.clear();
        self.cells.clear();
        self.index.clear();
    }

    /// Adds `id` standing at `at`; its index for the tick.
    pub fn add(&mut self, id: i64, at: Vec3) -> i32 {
        let i = self.pos.len() as i32;
        self.pos.push(at);
        self.ids.push(id);
        self.next.push(-1);
        let k = key(cell_of(at.x, self.cell), cell_of(at.z, self.cell));
        match self.cells.get_mut(&k) {
            Some(ends) => {
                self.next[ends.1 as usize] = i;
                ends.1 = i;
            }
            None => {
                self.cells.insert(k, (i, i));
            }
        }
        self.index.insert(id, i);
        i
    }

    pub fn size(&self) -> usize {
        self.pos.len()
    }

    pub fn cells(&self) -> usize {
        self.cells.len()
    }

    pub fn index_of(&self, id: i64) -> i32 {
        self.index.get(&id).copied().unwrap_or(-1)
    }

    /// Leaves entry `index` out of every query until the next fill.
    pub fn forget(&mut self, index: i32) {
        if let Some(p) = usize::try_from(index)
            .ok()
            .and_then(|i| self.pos.get_mut(i))
        {
            *p = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        }
    }

    pub fn position_of(&self, index: i32) -> Vec3 {
        self.pos.get(index as usize).copied().unwrap_or_default()
    }

    pub fn id_of(&self, index: i32) -> i64 {
        self.ids.get(index as usize).copied().unwrap_or(0)
    }

    #[inline]
    fn head(&self, cx: i64, cz: i64) -> i32 {
        self.cells.get(&key(cx, cz)).map_or(-1, |e| e.0)
    }

    /// Indices within `radius` of `point` in 3D, in the script's order, appended to `out`.
    pub fn near(&self, point: Vec3, radius: f64, out: &mut Vec<i32>) {
        out.clear();
        let reach = radius * radius;
        let x1 = cell_of_f64(f64::from(point.x) + radius, self.cell);
        let z0 = cell_of_f64(f64::from(point.z) - radius, self.cell);
        let z1 = cell_of_f64(f64::from(point.z) + radius, self.cell);
        let x0 = cell_of_f64(f64::from(point.x) - radius, self.cell);
        let span = x1
            .wrapping_sub(x0)
            .wrapping_add(1)
            .wrapping_mul(z1.wrapping_sub(z0).wrapping_add(1));
        if span > self.pos.len() as i64 {
            for (i, p) in self.pos.iter().enumerate() {
                if f64::from((point - *p).length_squared()) <= reach {
                    out.push(i as i32);
                }
            }
            return;
        }
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                let mut i = self.head(cx, cz);
                while i >= 0 {
                    if f64::from((point - self.pos[i as usize]).length_squared()) <= reach {
                        out.push(i);
                    }
                    i = self.next[i as usize];
                }
            }
        }
    }

    /// `DotNpcAiCrowdGrid.separation`: the spacing push on entry `self_index` from everybody
    /// within `query` metres of where it was added, pushed from `from`.
    pub fn separation(
        &self,
        self_index: i32,
        from: Vec3,
        radius: f64,
        query: f64,
        tie_break: Vec3,
    ) -> Vec3 {
        if radius <= 0.0 || self_index < 0 {
            return Vec3::default();
        }
        let Some(&centre) = self.pos.get(self_index as usize) else {
            return Vec3::default();
        };
        let mut escape = Vec3::new(tie_break.x, 0.0, tie_break.z);
        escape = if f64::from(escape.length()) > 0.001 {
            escape.normalized()
        } else {
            Vec3::default()
        };
        let reach = query * query;
        let tall = radius * 2.0;
        let mut push = Vec3::default();
        let x1 = cell_of_f64(f64::from(centre.x) + query, self.cell);
        let z0 = cell_of_f64(f64::from(centre.z) - query, self.cell);
        let z1 = cell_of_f64(f64::from(centre.z) + query, self.cell);
        let x0 = cell_of_f64(f64::from(centre.x) - query, self.cell);
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                let mut i = self.head(cx, cz);
                while i >= 0 {
                    let other = self.pos[i as usize];
                    if i != self_index && f64::from((centre - other).length_squared()) <= reach {
                        let offset = from - other;
                        let away = Vec3::new(offset.x, 0.0, offset.z);
                        let distance = f64::from(away.length());
                        if distance <= radius && f64::from(offset.y.abs()) <= tall {
                            if distance < 0.05 {
                                push = push + escape;
                            } else {
                                push =
                                    push + away.div_scalar(distance).scale(1.0 - distance / radius);
                            }
                        }
                    }
                    i = self.next[i as usize];
                }
            }
        }
        if f64::from(push.length()) > 0.001 {
            push.normalized()
        } else {
            Vec3::default()
        }
    }

    /// dot-npc-ai's `_spaced_direction` with ZZ's grid `separation`: the seek from `here` to `goal`
    /// plus 0.6 of entry `self_index`'s spacing push (radius `spacing`, query `2 * spacing`, `forward`
    /// turned a quarter turn about Y as the tie-break), flattened and made unit, zero when they
    /// cancel. `None` when the entry is not in the grid: the script then separates by `neighbours`.
    pub fn spaced_direction(
        &self,
        self_index: i32,
        here: Vec3,
        goal: Vec3,
        spacing: f64,
        forward: Vec3,
    ) -> Option<Vec3> {
        if self_index < 0 {
            return None;
        }
        let wanted = seek(here, goal);
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let apart = self.separation(self_index, here, spacing, spacing * 2.0, right);
        let mut direction = wanted.scale(1.0) + apart.scale(0.6);
        direction.y = 0.0;
        Some(if f64::from(direction.length()) > 0.001 {
            direction.normalized()
        } else {
            Vec3::default()
        })
    }
}

/// `DotNpcAiSteering.seek`: the flat unit direction from `from` to `goal`, zero within a millimetre.
#[inline]
pub fn seek(from: Vec3, goal: Vec3) -> Vec3 {
    let mut direction = goal - from;
    direction.y = 0.0;
    if f64::from(direction.length()) > 0.001 {
        direction.normalized()
    } else {
        Vec3::default()
    }
}

/// `floori(x / cell)` for a script float already in f64. The engine's `int64_t(floor(x))` gives
/// `i64::MIN` for infinities, NaN and anything out of range (a forgotten entry stands at INF), where
/// Rust's `as` saturates, so that case is spelled out.
#[inline]
fn cell_of_f64(v: f64, cell: f64) -> i64 {
    let f = (v / cell).floor();
    if (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&f) {
        f as i64
    } else {
        i64::MIN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn near_keeps_insertion_order_per_cell() {
        let mut g = Grid::new(4.0);
        g.add(10, v(0.5, 0.0, 0.5));
        g.add(11, v(5.0, 0.0, 0.5));
        g.add(12, v(1.0, 0.0, 1.0));
        let mut out = Vec::new();
        g.near(v(0.0, 0.0, 0.0), 2.0, &mut out);
        assert_eq!(out, vec![0, 2]);
        assert_eq!(g.index_of(11), 1);
        assert_eq!(g.index_of(99), -1);
        assert_eq!(g.cells(), 2);
    }

    #[test]
    fn wide_query_walks_every_entry() {
        let mut g = Grid::new(1.0);
        g.add(1, v(30.0, 0.0, 0.0));
        g.add(2, v(0.0, 0.0, 0.0));
        let mut out = Vec::new();
        g.near(v(0.0, 0.0, 0.0), 40.0, &mut out);
        assert_eq!(out, vec![0, 1]);
    }

    #[test]
    fn separation_pushes_apart_and_skips_forgotten() {
        let mut g = Grid::new(4.0);
        let a = g.add(1, v(0.0, 0.0, 0.0));
        let b = g.add(2, v(0.5, 0.0, 0.0));
        let push = g.separation(a, v(0.0, 0.0, 0.0), 1.1, 2.2, v(0.0, 0.0, 1.0));
        assert_eq!(push, v(-1.0, 0.0, 0.0));
        g.forget(b);
        assert_eq!(
            g.separation(a, v(0.0, 0.0, 0.0), 1.1, 2.2, v(0.0, 0.0, 1.0)),
            Vec3::default()
        );
    }

    #[test]
    fn coincident_neighbour_takes_the_tie_break() {
        let mut g = Grid::new(4.0);
        let a = g.add(1, v(2.0, 0.0, 2.0));
        g.add(2, v(2.0, 0.0, 2.0));
        let push = g.separation(a, v(2.0, 0.0, 2.0), 1.1, 2.2, v(0.0, 0.0, 3.0));
        assert_eq!(push, v(0.0, 0.0, 1.0));
    }

    #[test]
    fn negative_cells_key_like_the_script() {
        assert_eq!(key(-1, -1), (-1i64 << 32) ^ 0xFFFF_FFFF);
        assert_eq!(cell_of(-0.5, 4.0), -1);
        assert_eq!(cell_of(f32::INFINITY, 4.0), i64::MIN);
        assert_eq!(cell_of_f64(f64::NAN, 4.0), i64::MIN);
    }

    #[test]
    fn spaced_direction_seeks_and_spreads() {
        let mut g = Grid::new(2.0);
        let me = g.add(1, v(0.0, 0.0, 0.0));
        g.add(2, v(0.5, 0.0, 0.0));
        let d = g
            .spaced_direction(
                me,
                v(0.0, 0.0, 0.0),
                v(0.0, 3.0, 10.0),
                1.1,
                v(0.0, 0.0, -1.0),
            )
            .unwrap();
        assert!((d.length() - 1.0).abs() < 1e-6 && d.y == 0.0);
        assert!(
            d.x < 0.0 && d.z > 0.0,
            "pushed off the neighbour on +x while seeking +z: {d:?}"
        );
        let alone = g.spaced_direction(
            me,
            v(0.0, 0.0, 0.0),
            v(0.0, 0.0, 0.0),
            0.1,
            v(0.0, 0.0, -1.0),
        );
        assert_eq!(alone, Some(Vec3::default()));
        assert_eq!(
            g.spaced_direction(
                -1,
                v(0.0, 0.0, 0.0),
                v(1.0, 0.0, 0.0),
                1.1,
                v(0.0, 0.0, -1.0)
            ),
            None
        );
        assert_eq!(seek(v(1.0, 0.0, 1.0), v(1.0, 5.0, 1.0)), Vec3::default());
    }
}
