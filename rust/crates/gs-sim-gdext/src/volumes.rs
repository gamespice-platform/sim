//! `GSSimVolumes`: a map's static volume boxes and, for a tick's players in one call, which stand
//! clear of all of them (`gs_sim_volumes`). A conservative prefilter: the caller skips its own
//! per-player volume scripts only where this says clear, so the script stays the rule.

use godot::prelude::*;
use gs_sim_volumes::{Bounds, VolumeSet};

fn v3(v: Vector3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimVolumes {
    set: VolumeSet,
}

#[godot_api]
impl GSSimVolumes {
    /// Forgets every box (a new map).
    #[func]
    fn clear(&mut self) {
        self.set.clear();
    }

    /// Adds one volume (a negative size is flipped, as `AABB.abs()`).
    #[func]
    fn add_box(&mut self, aabb: Aabb) {
        self.set
            .add(Bounds::from_position_size(v3(aabb.position), v3(aabb.size)));
    }

    /// Adds every box of [param boxes].
    #[func]
    fn add_boxes(&mut self, boxes: Array<Aabb>) {
        for b in boxes.iter_shared() {
            self.set
                .add(Bounds::from_position_size(v3(b.position), v3(b.size)));
        }
    }

    #[func]
    fn count(&self) -> i64 {
        self.set.len() as i64
    }

    /// One byte per point of [param points]: 1 where the box `point + below .. point + above`
    /// touches no volume (faces meeting count as touching), 0 where it does or the point is not finite.
    #[func]
    fn clear_of(
        &self,
        points: PackedVector3Array,
        below: Vector3,
        above: Vector3,
    ) -> PackedByteArray {
        let pts: Vec<[f32; 3]> = points.as_slice().iter().map(|v| v3(*v)).collect();
        let out = self.set.clear_of(&pts, v3(below), v3(above));
        PackedByteArray::from(out.as_slice())
    }
}
