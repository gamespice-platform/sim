//! `GSSimPvs`: gs-net's `GSNetVisibility` asks (`cluster_of`, `cluster_sees`, `visible`) over
//! a copy of its arrays, so a sight check can drop a ray the map's PVS rules out (the Source
//! engine's `IsVisible` does the same before it traces) for about a tenth of a microsecond.
//!
//! `load` takes `GSNetVisibility.native_dict()` and copies only when its `serial` changes. The
//! same answers as the script to the bit (`gs_sim_pvs`; FC's `headless_bot_pvs` asks both over
//! seeded points on the converted maps, and its `fc_bot_pvs_verify` checks every live ask).
//! Read-only between loads, so worker threads may ask at once.

use gs_sim_pvs::{Pvs, bounds_of};
use godot::prelude::*;

/// A map's PVS, loaded from `GSNetVisibility.native_dict()`.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimPvs {
    pvs: Pvs,
    ready: bool,
    #[init(val = -1)]
    serial: i64,
}

fn field<T: FromGodot>(d: &VarDictionary, key: &str) -> Option<T> {
    d.get(key).and_then(|v| v.try_to::<T>().ok())
}

fn p3(v: Vector3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

#[godot_api]
impl GSSimPvs {
    /// Copies the set in [param vis] (`GSNetVisibility.native_dict()`) unless its `serial` is the
    /// one held. False, with an error and the old set kept, when a field is missing or mistyped;
    /// a set that is not ready loads and answers everything visible, as the script.
    #[func]
    fn load(&mut self, vis: VarDictionary) -> bool {
        let Some(serial) = field::<i64>(&vis, "serial") else {
            godot_error!("GSSimPvs.load: no serial");
            return false;
        };
        if serial == self.serial {
            return true;
        }
        let (
            Some(planes),
            Some(nodes),
            Some(head),
            Some(leaf_cluster),
            Some(rows),
            Some(row_bytes),
            Some(clusters),
            Some(bounds),
        ) = (
            field::<PackedFloat32Array>(&vis, "planes"),
            field::<PackedInt32Array>(&vis, "nodes"),
            field::<i64>(&vis, "head"),
            field::<PackedInt32Array>(&vis, "leaf_cluster"),
            field::<PackedByteArray>(&vis, "rows"),
            field::<i64>(&vis, "row_bytes"),
            field::<i64>(&vis, "clusters"),
            field::<Aabb>(&vis, "bounds"),
        )
        else {
            godot_error!("GSSimPvs.load: a field of native_dict() is missing or of the wrong type");
            return false;
        };
        let Ok(head) = i32::try_from(head) else {
            godot_error!("GSSimPvs.load: head {head} out of range");
            return false;
        };
        self.pvs = Pvs {
            planes: planes.as_slice().to_vec(),
            nodes: nodes.as_slice().to_vec(),
            head,
            leaf_cluster: leaf_cluster.as_slice().to_vec(),
            rows: rows.as_slice().to_vec(),
            row_bytes,
            clusters,
            bounds: bounds_of(p3(bounds.position), p3(bounds.size)),
        };
        self.ready = self.pvs.is_ready();
        self.serial = serial;
        true
    }

    /// The `serial` of the set held, -1 before the first load.
    #[func]
    fn serial(&self) -> i64 {
        self.serial
    }

    /// `GSNetVisibility.is_ready` of the set held.
    #[func]
    fn is_ready(&self) -> bool {
        self.ready
    }

    /// `GSNetVisibility.cluster_of`: the cluster at [param p], -1 for none.
    #[func]
    fn cluster_of(&self, p: Vector3) -> i32 {
        self.pvs.cluster_of(p3(p))
    }

    /// `GSNetVisibility.cluster_sees`.
    #[func]
    fn cluster_sees(&self, from: i32, to: i32) -> bool {
        self.pvs.cluster_sees(from, to)
    }

    /// `GSNetVisibility.visible`: whether anything at [param from] could see anything at [param to].
    #[func]
    fn visible(&self, from: Vector3, to: Vector3) -> bool {
        !self.ready
            || self
                .pvs
                .cluster_sees(self.pvs.cluster_of(p3(from)), self.pvs.cluster_of(p3(to)))
    }

    /// [method visible] for each pair in [param pairs] (from, to, from, to, ...), one byte per
    /// pair (1 visible); an odd last point is dropped.
    #[func]
    fn visible_many(&self, pairs: PackedVector3Array) -> PackedByteArray {
        pairs
            .as_slice()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[from, to]| u8::from(self.visible(from, to)))
            .collect()
    }
}
