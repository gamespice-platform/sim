//! `DotSimFlatTrace`: GameSpice Combat's `DotTraceFlat` box walk natively. Stateless (the boxes
//! come in each call as a packed array the script keeps), so brains on worker threads may share it.

use godot::prelude::*;

fn arr(v: Vector3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// The nearest box along a ray over a caller's packed box corners.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimFlatTrace {}

#[godot_api]
impl DotSimFlatTrace {
    /// The index of the box the ray from [param from] along [param direction] enters first within
    /// [param limit], or -1. [param corners] holds each box as `position` then `position + size`.
    #[func]
    fn nearest(
        &self,
        corners: PackedVector3Array,
        from: Vector3,
        direction: Vector3,
        limit: f64,
    ) -> i64 {
        let c = corners.as_slice();
        let hit = dot_sim_trace::nearest_by(
            c.len() / 2,
            |i| (arr(c[2 * i]), arr(c[2 * i + 1])),
            arr(from),
            arr(direction),
            limit,
        );
        hit.map_or(-1, |(i, _)| i as i64)
    }
}
