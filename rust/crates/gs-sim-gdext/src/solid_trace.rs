//! `GSSimSolid`: an imported map's static solid (its brushes' convex hulls and its terrain's
//! triangles) behind one box hierarchy, so a bot's line of sight there is answered in Rust instead
//! of a physics ray under the game's query lock. Built once per map, off the main thread (every
//! method may run on a worker thread while nothing else touches the object), then read-only and
//! shared with a [`crate::sight::GSSimSightRay`] by `bind_solid`. The answer is the physics ray's
//! with `hit_from_inside` off ([`gs_sim_trace::solid`]), not bit-identical on a grazing line.

use godot::prelude::*;
use gs_sim_trace::solid::{Solid, SolidBuilder};
use std::sync::Arc;

/// `xform * v` in `f64` (the engine's order: basis rows, then the origin).
fn place(xform: &Transform3D, v: Vector3) -> [f64; 3] {
    let r = &xform.basis.rows;
    let o = xform.origin;
    let (x, y, z) = (f64::from(v.x), f64::from(v.y), f64::from(v.z));
    let row = |a: Vector3, t: f32| {
        f64::from(a.x) * x + f64::from(a.y) * y + f64::from(a.z) * z + f64::from(t)
    };
    [row(r[0], o.x), row(r[1], o.y), row(r[2], o.z)]
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimSolid {
    builder: Option<SolidBuilder>,
    /// Shared with a sight ray bound to it.
    solid: Option<Arc<Solid>>,
}

#[godot_api]
impl GSSimSolid {
    /// Adds the convex hull of [param points] (a `ConvexPolygonShape3D`'s) placed by [param xform]
    /// (the shape's global transform). False when the points span no volume (nothing added).
    #[func]
    fn add_hull(&mut self, xform: Transform3D, points: PackedVector3Array) -> bool {
        let p: Vec<[f64; 3]> = points
            .as_slice()
            .iter()
            .map(|v| place(&xform, *v))
            .collect();
        self.builder.get_or_insert_with(SolidBuilder::new).hull(&p)
    }

    /// Adds [param faces] (a `ConcavePolygonShape3D`'s, three corners a triangle) placed by
    /// [param xform]; one-sided unless [param two_sided] (the shape's `backface_collision`).
    /// Returns how many triangles had an area and were added.
    #[func]
    fn add_faces(&mut self, xform: Transform3D, faces: PackedVector3Array, two_sided: bool) -> i64 {
        let b = self.builder.get_or_insert_with(SolidBuilder::new);
        let f = faces.as_slice();
        let mut added = 0;
        for t in f.as_chunks::<3>().0 {
            let (a, bb, c) = (
                place(&xform, t[0]),
                place(&xform, t[1]),
                place(&xform, t[2]),
            );
            added += i64::from(b.triangle(a, bb, c, two_sided));
        }
        added
    }

    /// Builds the hierarchy over everything added (once; later adds start a new solid). Returns
    /// the primitive count.
    #[func]
    fn build(&mut self) -> i64 {
        let solid = self.builder.take().unwrap_or_default().build();
        let n = (solid.hull_count() + solid.triangle_count()) as i64;
        self.solid = Some(Arc::new(solid));
        n
    }

    #[func]
    fn is_built(&self) -> bool {
        self.solid.is_some()
    }

    #[func]
    fn hull_count(&self) -> i64 {
        self.solid.as_ref().map_or(0, |s| s.hull_count() as i64)
    }

    #[func]
    fn triangle_count(&self) -> i64 {
        self.solid.as_ref().map_or(0, |s| s.triangle_count() as i64)
    }

    /// Whether nothing in the built solid blocks [param from] to [param to] (true unbuilt).
    #[func]
    fn clear(&self, from: Vector3, to: Vector3) -> bool {
        self.solid
            .as_ref()
            .is_none_or(|s| s.clear([from.x, from.y, from.z], [to.x, to.y, to.z]))
    }
}

impl GSSimSolid {
    /// The built solid, for a sight ray to share.
    pub fn shared(&self) -> Option<Arc<Solid>> {
        self.solid.clone()
    }
}
