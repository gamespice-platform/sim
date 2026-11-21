//! `GSSimCrowdGrid`: dot-npc-ai's `GSNpcAiCrowdGrid` with the same methods and the same
//! answers to the bit (`dot_sim_crowd::Grid`), plus `fill` to add a whole tick's crowd in one call.

use dot_sim_crowd::Grid;
use dot_sim_math::Vec3;
use godot::prelude::*;

#[inline]
fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[inline]
fn gv(v: Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// One tick's crowd; [method setup] sets the cell side (2 m until then).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimCrowdGrid {
    pub(crate) grid: Grid,
    scratch: Vec<i32>,
}

#[godot_api]
impl GSSimCrowdGrid {
    /// Empties the grid with [param cell]-metre cells (at least 0.1).
    #[func]
    fn setup(&mut self, cell: f64) {
        self.grid = Grid::new(cell);
    }

    /// The cell side in metres.
    #[func]
    fn cell(&self) -> f64 {
        self.grid.cell()
    }

    #[func]
    fn clear(&mut self) {
        self.grid.clear();
    }

    /// Adds [param id] standing at [param at]; its index for the tick.
    #[func]
    fn add(&mut self, id: i64, at: Vector3) -> i32 {
        self.grid.add(id, v3(at))
    }

    /// Adds every id at its position, in order, as that many [method add] calls.
    #[func]
    fn fill(&mut self, ids: PackedInt64Array, at: PackedVector3Array) {
        for (id, p) in ids.as_slice().iter().zip(at.as_slice()) {
            self.grid.add(*id, v3(*p));
        }
    }

    #[func]
    fn size(&self) -> i32 {
        self.grid.size() as i32
    }

    #[func]
    fn has(&self, id: i64) -> bool {
        self.grid.index_of(id) >= 0
    }

    #[func]
    fn index_of(&self, id: i64) -> i32 {
        self.grid.index_of(id)
    }

    #[func]
    fn forget(&mut self, index: i32) {
        self.grid.forget(index);
    }

    #[func]
    fn position_of(&self, index: i32) -> Vector3 {
        gv(self.grid.position_of(index))
    }

    #[func]
    fn id_of(&self, index: i32) -> i64 {
        self.grid.id_of(index)
    }

    /// Indices within [param radius] of [param point]; a new array, [param _out] is not written
    /// (the script's is, so callers use the return).
    #[func]
    fn near(&mut self, point: Vector3, radius: f64, _out: PackedInt32Array) -> PackedInt32Array {
        self.grid.near(v3(point), radius, &mut self.scratch);
        PackedInt32Array::from(self.scratch.as_slice())
    }

    #[func]
    fn separation(
        &self,
        self_index: i32,
        from: Vector3,
        radius: f64,
        query: f64,
        tie_break: Vector3,
    ) -> Vector3 {
        gv(self
            .grid
            .separation(self_index, v3(from), radius, query, v3(tie_break)))
    }

    /// [method separation] for [param id]'s entry, zero when it is not in the grid: one call for two.
    #[func]
    fn separation_of(
        &self,
        id: i64,
        from: Vector3,
        radius: f64,
        query: f64,
        tie_break: Vector3,
    ) -> Vector3 {
        gv(self.grid.separation(
            self.grid.index_of(id),
            v3(from),
            radius,
            query,
            v3(tie_break),
        ))
    }

    /// A zombie's heading before avoidance (dot-npc-ai's `_spaced_direction` over this grid): the seek
    /// from [param here] to [param goal] plus 0.6 of [param id]'s spacing push, the tie-break
    /// [param forward] turned a quarter turn, flat and unit or zero. INF when [param id] is not in
    /// the grid, for the script's own pass.
    #[func]
    fn spaced_direction(
        &self,
        id: i64,
        here: Vector3,
        goal: Vector3,
        spacing: f64,
        forward: Vector3,
    ) -> Vector3 {
        self.grid
            .spaced_direction(
                self.grid.index_of(id),
                v3(here),
                v3(goal),
                spacing,
                v3(forward),
            )
            .map_or(Vector3::INF, gv)
    }

    #[func]
    fn describe(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        d.set("entries", self.grid.size() as i64);
        d.set("cells", self.grid.cells() as i64);
        d.set("cell", self.grid.cell());
        d
    }
}
