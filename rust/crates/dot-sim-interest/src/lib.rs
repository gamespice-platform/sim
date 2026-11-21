//! The cell index of `DotNetInterestGrid`: entities binned by horizontal (optionally 3D) cell, queried by the
//! cells an observer's radius touches. Plain arrays in and out; the script keeps the identities and maps the
//! returned slots back. The answer order is the script's: dx outermost, then dy, then dz, each cell's slots in
//! insertion order.

use std::collections::HashMap;

/// The script's cell key: three 21-bit coordinates in one i64 (wrapping like GDScript's int).
pub fn key(x: i64, y: i64, z: i64) -> i64 {
    ((x & 0x1F_FFFF) << 42) | ((y & 0x1F_FFFF) << 21) | (z & 0x1F_FFFF)
}

/// GDScript `int(floorf(v / cell))`: f64 divide, floor, truncating cast (saturating on overflow, as Rust casts do).
fn cell_of(v: f64, cell: f64) -> i64 {
    (v / cell).floor() as i64
}

#[derive(Default)]
pub struct InterestGrid {
    cells: HashMap<i64, Vec<u32>>,
    cell_size: f64,
    ignore_vertical: bool,
}

impl InterestGrid {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds from positions (x, y, z per slot, f32 as Godot stores them).
    pub fn build(&mut self, xyz: &[[f32; 3]], cell_size: f64, ignore_vertical: bool) {
        self.cells.clear();
        self.cell_size = cell_size;
        self.ignore_vertical = ignore_vertical;
        for (slot, p) in xyz.iter().enumerate() {
            let cy = if ignore_vertical {
                0
            } else {
                cell_of(f64::from(p[1]), cell_size)
            };
            let k = key(
                cell_of(f64::from(p[0]), cell_size),
                cy,
                cell_of(f64::from(p[2]), cell_size),
            );
            self.cells.entry(k).or_default().push(slot as u32);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Slots in the cells within `radius` of `centre`, in the script's order.
    pub fn query(&self, centre: [f32; 3], radius: f64, out: &mut Vec<i32>) {
        let reach = (radius / self.cell_size.max(1.0)).ceil() as i64;
        let cx = cell_of(f64::from(centre[0]), self.cell_size);
        let cy = if self.ignore_vertical {
            0
        } else {
            cell_of(f64::from(centre[1]), self.cell_size)
        };
        let cz = cell_of(f64::from(centre[2]), self.cell_size);
        let (y0, y1) = if self.ignore_vertical {
            (0, 0)
        } else {
            (-reach, reach)
        };
        for dx in -reach..=reach {
            for dy in y0..=y1 {
                for dz in -reach..=reach {
                    let k = key(
                        cx.wrapping_add(dx),
                        cy.wrapping_add(dy),
                        cz.wrapping_add(dz),
                    );
                    if let Some(v) = self.cells.get(&k) {
                        out.extend(v.iter().map(|&s| s as i32));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_matches_script_packing() {
        assert_eq!(key(1, 0, 0), 1 << 42);
        assert_eq!(key(0, 1, 0), 1 << 21);
        assert_eq!(key(-1, 0, 0), 0x1F_FFFF << 42);
        assert_eq!(key(0, 0, -1), 0x1F_FFFF);
    }

    #[test]
    fn query_order_and_radius() {
        let mut g = InterestGrid::new();
        // slot 0 far, 1 and 2 share a cell, 3 one cell over
        let p = [
            [1000.0, 0.0, 1000.0],
            [10.0, 5.0, 10.0],
            [20.0, 9.0, 30.0],
            [150.0, 0.0, 10.0],
        ];
        g.build(&p, 128.0, true);
        let mut out = Vec::new();
        g.query([0.0, 0.0, 0.0], 128.0, &mut out);
        assert_eq!(out, vec![1, 2, 3]);
        out.clear();
        g.query([1000.0, 0.0, 1000.0], 10.0, &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn negative_cells_floor() {
        let mut g = InterestGrid::new();
        g.build(&[[-0.5, 0.0, -0.5]], 128.0, true);
        let mut out = Vec::new();
        g.query([-1.0, 0.0, -1.0], 1.0, &mut out);
        assert_eq!(out, vec![0]);
    }
}
