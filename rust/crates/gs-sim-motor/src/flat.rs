//! `GSFpsFlatBody`: the built-in maps' collision body (an analytic floor plane, axis-aligned boxes
//! and half-space planes, the capsule swept as a box), spelled operation for operation so the
//! native motor answers what the script answers and needs no physics server.
//!
//! The script's broad phase (`perf_caches`) and per-tick candidate list only skip boxes the slab
//! test would reject, so this always rejects with the margin and walks every box in order.

use crate::slide::{Aabb, comp, flat_point, fold_boxes};
use crate::{Hit, UP, Vec3, capsule_support};
use gs_sim_math::minf;

/// `GSFpsFlatBody.FLOOR_ID`.
pub const FLOOR_ID: i64 = 1;
/// `GSFpsFlatBody.BOX_ID_BASE`.
pub const BOX_ID_BASE: i64 = 2;
/// `GSFpsFlatBody.PLANE_ID_BASE`.
pub const PLANE_ID_BASE: i64 = 100_000;

/// A half-space: everything behind `normal` is solid. Engine `Plane`, `f32`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub d: f32,
}

/// The floor, boxes and planes of a `GSFpsFlatBody`.
#[derive(Clone, Debug, Default)]
pub struct FlatWorld {
    /// `floor_y`; negative infinity for no floor.
    pub floor_y: f64,
    pub boxes: Vec<Aabb>,
    pub planes: Vec<Plane>,
}

fn support(n: Vec3, height: f64, radius: f64) -> f64 {
    radius * (n.x as f64).abs() + height * 0.5 * (n.y as f64).abs() + radius * (n.z as f64).abs()
}

fn bounds(at: Vec3, height: f64, radius: f64) -> Aabb {
    let half = Vec3::new(radius as f32, (height * 0.5) as f32, radius as f32);
    Aabb {
        pos: at - half,
        size: half.scale(2.0),
    }
}

fn rest_hit(normal: Vec3, depth: f64, id: i64) -> Hit {
    Hit {
        hit: true,
        fraction: 0.0,
        normal,
        point: Vec3::new(0.0, 0.0, 0.0),
        collider_id: id,
        depth,
    }
}

impl FlatWorld {
    /// `GSFpsFlatBody.sweep`.
    pub fn sweep(&self, from: Vec3, motion: Vec3, height: f64, radius: f64) -> Hit {
        if (motion.length_squared() as f64) <= 0.0 {
            return Hit::MISS;
        }
        let mut best = 1.0f64;
        let mut normal = UP;
        let mut id = 0i64;
        let mut found = false;

        if (motion.y as f64) < 0.0 && self.floor_y > f64::NEG_INFINITY {
            let feet = from.y as f64 - height * 0.5;
            let mut t = 0.0;
            if feet > self.floor_y {
                t = (self.floor_y - feet) / motion.y as f64;
            }
            if t < best {
                best = t;
                normal = UP;
                id = FLOOR_ID;
                found = true;
            }
        }

        let mut index = 0usize;
        let mut box_normal = UP;
        if fold_boxes(
            from,
            motion,
            height,
            radius,
            &self.boxes,
            &mut best,
            &mut box_normal,
            &mut index,
        ) {
            normal = box_normal;
            id = BOX_ID_BASE + index as i64;
            found = true;
        }

        for (i, p) in self.planes.iter().enumerate() {
            let t = plane_sweep(from, motion, height, radius, p);
            if t < 0.0 || t >= best {
                continue;
            }
            best = t;
            normal = p.normal;
            id = PLANE_ID_BASE + i as i64;
            found = true;
        }
        if !found {
            return Hit::MISS;
        }
        let (fraction, point) = flat_point(from, motion, height, radius, best, normal);
        Hit {
            hit: true,
            fraction,
            normal,
            point,
            collider_id: id,
            depth: 0.0,
        }
    }

    /// `GSFpsFlatBody.rest_contact`.
    pub fn rest_contact(&self, at: Vec3, height: f64, radius: f64) -> Hit {
        let mut best = Hit::MISS;
        let mut deepest = 0.0f64;
        if self.floor_y > f64::NEG_INFINITY {
            let depth = self.floor_y - (at.y as f64 - height * 0.5);
            if depth > deepest {
                deepest = depth;
                best = rest_hit(UP, depth, FLOOR_ID);
            }
        }
        for (i, p) in self.planes.iter().enumerate() {
            let s = capsule_support(p.normal, height, radius);
            let depth = s - (p.normal.dot(at) as f64 - p.d as f64);
            if depth > deepest {
                deepest = depth;
                best = rest_hit(p.normal, depth, PLANE_ID_BASE + i as i64);
            }
        }
        let b = bounds(at, height, radius);
        for (i, bx) in self.boxes.iter().enumerate() {
            if !b.intersects(bx) {
                continue;
            }
            let mut least = f64::INFINITY;
            let mut n = Vec3::new(0.0, 0.0, 0.0);
            for axis in 0..3 {
                let out_positive = comp(bx.pos, axis) as f64 + comp(bx.size, axis) as f64
                    - comp(b.pos, axis) as f64;
                let out_negative = comp(b.pos, axis) as f64 + comp(b.size, axis) as f64
                    - comp(bx.pos, axis) as f64;
                let depth = minf(out_positive, out_negative);
                if depth > 0.0 && depth < least {
                    least = depth;
                    let v = if out_positive < out_negative {
                        1.0
                    } else {
                        -1.0
                    };
                    n = match axis {
                        0 => Vec3::new(v, 0.0, 0.0),
                        1 => Vec3::new(0.0, v, 0.0),
                        _ => Vec3::new(0.0, 0.0, v),
                    };
                }
            }
            if least < f64::INFINITY && least > deepest {
                deepest = least;
                best = rest_hit(n, least, BOX_ID_BASE + i as i64);
            }
        }
        best
    }

    /// `GSFpsFlatBody.overlaps`.
    pub fn overlaps(&self, at: Vec3, height: f64, radius: f64) -> bool {
        let b = bounds(at, height, radius);
        if self.floor_y > f64::NEG_INFINITY && (b.pos.y as f64) < self.floor_y {
            return true;
        }
        if self.boxes.iter().any(|bx| b.intersects(bx)) {
            return true;
        }
        self.planes
            .iter()
            .any(|p| p.normal.dot(at) as f64 - p.d as f64 - support(p.normal, height, radius) < 0.0)
    }
}

/// `GSFpsFlatBody._plane_sweep`: the entry fraction, or -1.
fn plane_sweep(from: Vec3, motion: Vec3, height: f64, radius: f64, p: &Plane) -> f64 {
    let s = support(p.normal, height, radius);
    let distance = p.normal.dot(from) as f64 - p.d as f64 - s;
    if distance <= 0.0 {
        return 0.0;
    }
    let closing = p.normal.dot(motion) as f64;
    if closing >= 0.0 {
        return -1.0;
    }
    let t = distance / -closing;
    if t > 1.0 {
        return -1.0;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> FlatWorld {
        FlatWorld {
            floor_y: 0.0,
            boxes: vec![Aabb {
                pos: Vec3::new(2.0, 0.0, -1.0),
                size: Vec3::new(1.0, 2.0, 2.0),
            }],
            planes: vec![Plane {
                normal: Vec3::new(0.0, 1.0, 0.0).scale(1.0),
                d: -5.0,
            }],
        }
    }

    #[test]
    fn floor_catches_a_fall_with_the_floor_id() {
        let h = world().sweep(
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(0.0, -4.0, 0.0),
            1.8,
            0.4,
        );
        assert!(h.hit);
        assert_eq!(h.collider_id, FLOOR_ID);
        assert_eq!(h.normal, UP);
    }

    #[test]
    fn box_stops_a_walk_with_its_index_id() {
        let h = world().sweep(Vec3::new(0.0, 1.0, 0.0), Vec3::new(3.0, 0.0, 0.0), 1.8, 0.4);
        assert!(h.hit);
        assert_eq!(h.collider_id, BOX_ID_BASE);
        assert_eq!(h.normal, Vec3::new(-1.0, 0.0, 0.0));
        assert!((h.fraction - (2.0 - 0.4) / 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_miss_is_the_miss() {
        let h = world().sweep(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.5, 0.0), 1.8, 0.4);
        assert_eq!(h, Hit::MISS);
    }

    #[test]
    fn rest_contact_reports_the_floor_depth() {
        let h = world().rest_contact(Vec3::new(0.0, 0.8, 0.0), 1.8, 0.4);
        assert!(h.hit);
        assert_eq!(h.collider_id, FLOOR_ID);
        assert!((h.depth - 0.1).abs() < 1e-6);
    }

    #[test]
    fn rest_contact_in_a_box_takes_the_shallowest_axis() {
        let h = world().rest_contact(Vec3::new(1.7, 1.0, 0.0), 1.8, 0.4);
        assert_eq!(h.collider_id, BOX_ID_BASE);
        assert_eq!(h.normal, Vec3::new(-1.0, 0.0, 0.0));
    }

    #[test]
    fn overlaps_floor_box_and_clear() {
        let w = world();
        assert!(w.overlaps(Vec3::new(0.0, 0.5, 0.0), 1.8, 0.4));
        assert!(w.overlaps(Vec3::new(2.5, 1.5, 0.0), 1.8, 0.4));
        assert!(!w.overlaps(Vec3::new(0.0, 1.5, 0.0), 1.8, 0.4));
    }
}
