//! `GSSimInterestGrid`: GameSpice Net's `GSNetInterestGrid` cell index natively. The script keeps the identities;
//! this takes their positions once a tick (`build`) and answers each observer's slots (`query`), in the script's order.
//! The script original stays the specification and the fallback.

use dot_sim_interest::InterestGrid;
use godot::prelude::*;

/// The cell index of one interest grid.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimInterestGrid {
    grid: InterestGrid,
}

#[godot_api]
impl GSSimInterestGrid {
    /// Rebuilds the index from [param positions] (slot = array index).
    #[func]
    fn build(&mut self, positions: PackedVector3Array, cell_size: f64, ignore_vertical: bool) {
        let xyz: Vec<[f32; 3]> = positions
            .as_slice()
            .iter()
            .map(|p| [p.x, p.y, p.z])
            .collect();
        self.grid.build(&xyz, cell_size, ignore_vertical);
    }

    /// Whether nothing is indexed.
    #[func]
    fn is_empty(&self) -> bool {
        self.grid.is_empty()
    }

    /// Number of occupied cells.
    #[func]
    fn cell_count(&self) -> i64 {
        self.grid.cell_count() as i64
    }

    /// The slots in the cells [param radius] around [param centre] touches.
    #[func]
    fn query(&self, centre: Vector3, radius: f64) -> PackedInt32Array {
        let mut out = Vec::new();
        self.grid
            .query([centre.x, centre.y, centre.z], radius, &mut out);
        PackedInt32Array::from(out)
    }
}
