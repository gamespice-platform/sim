//! The trace provider: what answers a [`Body`](crate::Body)'s collision queries.
//!
//! Two providers exist. The default is the engine's physics space, reached through
//! [`Space`](crate::Space) (`cast_motion`, `get_rest_info`, `intersect_shape`, `intersect_ray`)
//! with the body's fallbacks and memos on top; it is what the script asks and stays the
//! specification. A [`TraceWorld`] given to [`Body::set_world`](crate::Body::set_world) replaces it
//! wholesale: every sweep, rest query, overlap and ray is the world's answer and no space is asked.
//! [`FlatWorld`] is one; a collision-model mirror of the map is another, plugged in the same way.

use std::fmt::Debug;
use std::sync::Arc;

use crate::flat::FlatWorld;
use crate::{Contact, Hit, Masks, Vec3};

/// A collision world a body's queries are answered from in place of the engine's physics space.
///
/// Shared between every body on the map (`Arc`), so it is read-only while ticks run. The capsule
/// is `height` tall (caps included) and `radius` wide, centred on the point given; `m` is the
/// body's collision and pass-out masks, which a world without layers may ignore.
pub trait TraceWorld: Send + Sync + Debug {
    /// `GSFpsBody.sweep`: the capsule at `from` moved along `motion` (non-zero).
    fn sweep(&self, from: Vec3, motion: Vec3, height: f64, radius: f64, m: Masks) -> Hit;
    /// `GSFpsBody.rest_contact`: the deepest contact of the capsule resting at `at`.
    fn rest_contact(&self, at: Vec3, height: f64, radius: f64, m: Masks) -> Hit;
    /// `GSFpsBody.overlaps`: whether anything is inside the capsule at `at`.
    fn overlaps(&self, at: Vec3, height: f64, radius: f64, m: Masks) -> bool;
    /// `intersect_ray` from `from` to `to` under `mask`: the first contact, or `None`.
    fn ray(&self, from: Vec3, to: Vec3, mask: i64) -> Option<Contact>;
}

/// A shared trace world, as a body holds it.
pub type SharedWorld = Arc<dyn TraceWorld>;

impl TraceWorld for FlatWorld {
    fn sweep(&self, from: Vec3, motion: Vec3, height: f64, radius: f64, _: Masks) -> Hit {
        FlatWorld::sweep(self, from, motion, height, radius)
    }

    fn rest_contact(&self, at: Vec3, height: f64, radius: f64, _: Masks) -> Hit {
        FlatWorld::rest_contact(self, at, height, radius)
    }

    fn overlaps(&self, at: Vec3, height: f64, radius: f64, _: Masks) -> bool {
        FlatWorld::overlaps(self, at, height, radius)
    }

    /// A ray as a zero-radius sweep: the hull reduces to its centre line, which a zero height pins
    /// to a point.
    fn ray(&self, from: Vec3, to: Vec3, _: i64) -> Option<Contact> {
        let h = FlatWorld::sweep(self, from, to - from, 0.0, 0.0);
        h.hit.then_some(Contact {
            normal: h.normal,
            point: h.point,
            collider_id: h.collider_id,
        })
    }
}
