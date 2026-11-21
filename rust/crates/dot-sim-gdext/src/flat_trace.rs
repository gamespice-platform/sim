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

/// A box world's bounding-volume hierarchy, built once by [method build] and then read-only: a ray
/// visits the few boxes near its line, and a whole tick's rays go in one call. Same answers as
/// [method DotSimFlatTrace.nearest] (the least entry distance, a later box winning a tie).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimFlatBvh {
    bvh: Option<dot_sim_trace::Bvh>,
}

#[godot_api]
impl DotSimFlatBvh {
    /// Builds over [param corners], each box as `position` then `position + size`.
    #[func]
    fn build(&mut self, corners: PackedVector3Array) {
        let c: Vec<[f32; 3]> = corners.as_slice().iter().map(|v| arr(*v)).collect();
        self.bvh = Some(dot_sim_trace::Bvh::new(&c));
    }

    /// How many boxes it was built over.
    #[func]
    fn size(&self) -> i64 {
        self.bvh.as_ref().map_or(0, |b| b.len() as i64)
    }

    /// The index of the box the ray enters first within [param limit], or -1.
    #[func]
    fn nearest(&self, from: Vector3, direction: Vector3, limit: f64) -> i64 {
        self.bvh
            .as_ref()
            .and_then(|b| b.nearest(arr(from), arr(direction), limit))
            .map_or(-1, |(i, _)| i as i64)
    }

    /// Every ray's world hit at once, struct-of-arrays in and out: ray `k` runs from `froms[k]`
    /// along `directions[k]` for at most `limits[k]`, over the ground at [param floor_y] (pass
    /// `-INF` for none) and the boxes. Returns `kind` (a PackedByteArray: 0 miss, 1 ground, 2 box),
    /// `index` (the box, else -1) and `t` (the distance, else the limit), parallel to the rays.
    #[func]
    fn world_rays(
        &self,
        floor_y: f64,
        froms: PackedVector3Array,
        directions: PackedVector3Array,
        limits: PackedFloat64Array,
    ) -> VarDictionary {
        let n = froms.len().min(directions.len()).min(limits.len());
        let (f, d, l) = (froms.as_slice(), directions.as_slice(), limits.as_slice());
        let mut kind = vec![0u8; n];
        let mut index = vec![-1i32; n];
        let mut t = l[..n].to_vec();
        if let Some(bvh) = self.bvh.as_ref() {
            for k in 0..n {
                match bvh.world_ray(floor_y, arr(f[k]), arr(d[k]), l[k]) {
                    dot_sim_trace::Found::Miss => {}
                    dot_sim_trace::Found::Floor(dist) => {
                        kind[k] = 1;
                        t[k] = dist;
                    }
                    dot_sim_trace::Found::Box(i, dist) => {
                        kind[k] = 2;
                        index[k] = i as i32;
                        t[k] = dist;
                    }
                }
            }
        }
        let mut out = VarDictionary::new();
        out.set("kind", &PackedByteArray::from(kind.as_slice()).to_variant());
        out.set(
            "index",
            &PackedInt32Array::from(index.as_slice()).to_variant(),
        );
        out.set("t", &PackedFloat64Array::from(t.as_slice()).to_variant());
        out
    }
}
