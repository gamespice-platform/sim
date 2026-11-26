//! A static world of convex hulls and triangles, as a Godot physics ray sees it: whether anything
//! blocks a segment. The hulls are an imported map's brushes (`ConvexPolygonShape3D` points), the
//! triangles its terrain (`ConcavePolygonShape3D` faces); a [`Bvh`] over their bounds finds the few
//! near a line.
//!
//! The answer is GodotPhysics3D's `intersect_ray` with `hit_from_inside` off: a segment starting
//! inside a hull is not blocked by it, a one-sided triangle blocks only from its front (the side
//! its `Plane(a, b, c)` normal faces). The engine clips in `f32` against its own hull faces and this
//! kernel in `f64` against the planes of the points, so the two can differ on a line that grazes an
//! edge or a face; FC's `headless_bot_sight_native` counts those against physics.

use crate::Bvh;

type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A primitive's bounds grow by this much (metres) each way, so the box hierarchy, which treats a
/// direction under [`crate::PARALLEL`] on an axis as flat, never drops a primitive the line touches.
const PAD: f32 = 0.001;

/// One triangle: a corner, two edges and its front (`Plane(a, b, c)` normal, unnormalised).
struct Tri {
    a: V,
    e1: V,
    e2: V,
    front: V,
    two_sided: bool,
}

/// Collects hulls and triangles, then [`SolidBuilder::build`]s the [`Solid`].
#[derive(Default)]
pub struct SolidBuilder {
    planes: Vec<[f64; 4]>,
    hulls: Vec<(u32, u32)>,
    tris: Vec<Tri>,
    /// Hull bounds then triangle bounds, low then high corner, in the order added.
    hull_corners: Vec<[f32; 3]>,
    tri_corners: Vec<[f32; 3]>,
}

fn bounds(points: &[V]) -> [[f32; 3]; 2] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    [
        [lo[0] as f32 - PAD, lo[1] as f32 - PAD, lo[2] as f32 - PAD],
        [hi[0] as f32 + PAD, hi[1] as f32 + PAD, hi[2] as f32 + PAD],
    ]
}

/// The outward face planes `(n, d)` (unit `n`, inside where `n . p <= d`) of the convex hull of
/// `points`: every plane through three points that has all points on one side, once each.
/// Empty when the points span no volume.
pub fn hull_planes(points: &[V]) -> Vec<[f64; 4]> {
    let n = points.len();
    let mut extent = 0.0_f64;
    for p in points {
        for a in 0..3 {
            extent = extent.max((p[a] - points[0][a]).abs());
        }
    }
    let eps = 1e-6 + extent * 1e-6;
    let mut out: Vec<[f64; 4]> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            let e1 = sub(points[j], points[i]);
            for k in j + 1..n {
                let c = cross(e1, sub(points[k], points[i]));
                let len = dot(c, c).sqrt();
                if len <= eps * eps.max(extent) {
                    continue;
                }
                let mut nrm = [c[0] / len, c[1] / len, c[2] / len];
                let mut d = dot(nrm, points[i]);
                let (mut above, mut below) = (false, false);
                for p in points {
                    let s = dot(nrm, *p) - d;
                    above |= s > eps;
                    below |= s < -eps;
                    if above && below {
                        break;
                    }
                }
                if above == below {
                    continue;
                }
                if above {
                    nrm = [-nrm[0], -nrm[1], -nrm[2]];
                    d = -d;
                }
                let seen = out
                    .iter()
                    .any(|q| dot(nrm, [q[0], q[1], q[2]]) > 1.0 - 1e-9 && (q[3] - d).abs() <= eps);
                if !seen {
                    out.push([nrm[0], nrm[1], nrm[2], d]);
                }
            }
        }
    }
    out
}

impl SolidBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the convex hull of `points` (world space). False, and nothing added, when they span no volume.
    pub fn hull(&mut self, points: &[V]) -> bool {
        let planes = hull_planes(points);
        if planes.len() < 4 {
            return false;
        }
        self.hulls
            .push((self.planes.len() as u32, planes.len() as u32));
        self.planes.extend(planes);
        self.hull_corners.extend(bounds(points));
        true
    }

    /// Adds the triangle `a`, `b`, `c` (world space); one-sided unless `two_sided`. False for a
    /// triangle with no area.
    pub fn triangle(&mut self, a: V, b: V, c: V, two_sided: bool) -> bool {
        let e1 = sub(b, a);
        let e2 = sub(c, a);
        if dot(cross(e1, e2), cross(e1, e2)) == 0.0 {
            return false;
        }
        self.tris.push(Tri {
            a,
            e1,
            e2,
            front: cross(sub(a, c), sub(a, b)),
            two_sided,
        });
        self.tri_corners.extend(bounds(&[a, b, c]));
        true
    }

    pub fn build(self) -> Solid {
        let mut corners = self.hull_corners;
        corners.extend(self.tri_corners);
        Solid {
            bvh: Bvh::new(&corners),
            planes: self.planes,
            hulls: self.hulls,
            tris: self.tris,
        }
    }
}

/// The built world: hull planes and triangles behind one box hierarchy (hulls first, then triangles).
pub struct Solid {
    bvh: Bvh,
    planes: Vec<[f64; 4]>,
    hulls: Vec<(u32, u32)>,
    tris: Vec<Tri>,
}

impl Solid {
    pub fn hull_count(&self) -> usize {
        self.hulls.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// Whether nothing blocks the segment `from` to `to`.
    pub fn clear(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        let o = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let len = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
        if len <= 0.0 {
            return true;
        }
        let dir = [o[0] / len, o[1] / len, o[2] / len];
        let f = [f64::from(from[0]), f64::from(from[1]), f64::from(from[2])];
        let seg = sub([f64::from(to[0]), f64::from(to[1]), f64::from(to[2])], f);
        let hulls = self.hulls.len();
        !self.bvh.any(from, dir, f64::from(len), |i| {
            if i < hulls {
                self.hull_blocks(i, f, seg)
            } else {
                self.tri_blocks(&self.tris[i - hulls], f, seg)
            }
        })
    }

    /// Whether the segment `f + s * seg`, `s` in 0..=1, enters hull `i` from outside.
    fn hull_blocks(&self, i: usize, f: V, seg: V) -> bool {
        let (first, count) = self.hulls[i];
        let mut s_in = 0.0_f64;
        let mut s_out = 1.0_f64;
        let mut outside = false;
        for q in &self.planes[first as usize..(first + count) as usize] {
            let n = [q[0], q[1], q[2]];
            let dist = dot(n, f) - q[3];
            let den = dot(n, seg);
            outside |= dist > 0.0;
            if den == 0.0 {
                if dist > 0.0 {
                    return false;
                }
                continue;
            }
            let s = -dist / den;
            if den < 0.0 {
                s_in = s_in.max(s);
            } else {
                s_out = s_out.min(s);
            }
            if s_in > s_out {
                return false;
            }
        }
        outside
    }

    /// Whether the segment meets the triangle's front (either side when two-sided), past its start.
    fn tri_blocks(&self, t: &Tri, f: V, seg: V) -> bool {
        if !t.two_sided && dot(t.front, seg) > 0.0 {
            return false;
        }
        let h = cross(seg, t.e2);
        let a = dot(t.e1, h);
        if a == 0.0 {
            return false;
        }
        let inv = 1.0 / a;
        let s = sub(f, t.a);
        let u = inv * dot(s, h);
        if !(0.0..=1.0).contains(&u) {
            return false;
        }
        let q = cross(s, t.e1);
        let v = inv * dot(seg, q);
        if v < 0.0 || u + v > 1.0 {
            return false;
        }
        let k = inv * dot(t.e2, q);
        k > 0.0 && k <= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(at: V, size: f64) -> Vec<V> {
        let mut p = Vec::new();
        for x in [0.0, size] {
            for y in [0.0, size] {
                for z in [0.0, size] {
                    p.push([at[0] + x, at[1] + y, at[2] + z]);
                }
            }
        }
        p
    }

    #[test]
    fn a_cube_has_six_planes_and_a_flat_set_none() {
        assert_eq!(hull_planes(&cube([0.0; 3], 1.0)).len(), 6);
        let flat = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
        ];
        assert!(hull_planes(&flat).is_empty());
        let mut b = SolidBuilder::new();
        assert!(!b.hull(&flat));
    }

    #[test]
    fn a_hull_blocks_lines_through_it_not_beside_short_or_from_inside() {
        let mut b = SolidBuilder::new();
        assert!(b.hull(&cube([2.0, 0.0, 0.0], 1.0)));
        let s = b.build();
        assert!(!s.clear([0.0, 0.5, 0.5], [5.0, 0.5, 0.5]));
        assert!(!s.clear([5.0, 0.7, 0.2], [0.0, 0.4, 0.6]));
        assert!(s.clear([0.0, 1.5, 0.5], [5.0, 1.5, 0.5]));
        assert!(s.clear([0.0, 0.5, 0.5], [1.9, 0.5, 0.5]));
        // Starting inside: not blocked by that hull (hit_from_inside off).
        assert!(s.clear([2.5, 0.5, 0.5], [9.0, 0.5, 0.5]));
        // Ending inside is blocked.
        assert!(!s.clear([0.0, 0.5, 0.5], [2.5, 0.5, 0.5]));
    }

    #[test]
    fn a_slanted_hull_blocks_by_its_planes_not_its_bounds() {
        // A wedge: the diagonal face from (0,0,0)-(0,0,1) up to (1,1,*), solid below it.
        let w = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let mut b = SolidBuilder::new();
        assert!(b.hull(&w));
        let s = b.build();
        // Above the slope but inside the bounds: clear. Below it: blocked.
        assert!(s.clear([-1.0, 0.8, 0.5], [0.4, 0.8, 0.5]));
        assert!(!s.clear([-1.0, 0.2, 0.5], [0.9, 0.2, 0.5]));
    }

    #[test]
    fn a_one_sided_triangle_blocks_from_its_front_only() {
        // Plane(a, b, c) normal = (a - c) x (a - b): for this floor it points up (+y).
        let (a, b2, c) = ([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 4.0]);
        let front = cross(sub(a, c), sub(a, b2));
        assert!(front[1] > 0.0);
        let mut b = SolidBuilder::new();
        assert!(b.triangle(a, b2, c, false));
        let s = b.build();
        assert!(!s.clear([1.0, 1.0, 1.0], [1.0, -1.0, 1.0]));
        assert!(s.clear([1.0, -1.0, 1.0], [1.0, 1.0, 1.0]));
        assert!(s.clear([3.0, 1.0, 3.0], [3.0, -1.0, 3.0]));
        let mut b = SolidBuilder::new();
        assert!(b.triangle(a, b2, c, true));
        assert!(!b.build().clear([1.0, -1.0, 1.0], [1.0, 1.0, 1.0]));
    }

    fn rng(state: &mut u64) -> f64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state >> 11) as f64 / (1u64 << 53) as f64
    }

    #[test]
    fn the_hierarchy_answers_as_every_primitive_over_many_worlds() {
        let mut st = 0x51_6D_u64 ^ 0x9E37_79B9_7F4A_7C15;
        let mut blocked = 0;
        for world in 0..20 {
            let mut b = SolidBuilder::new();
            let mut all: Vec<Vec<V>> = Vec::new();
            for _ in 0..[1, 7, 40, 150][world % 4] {
                let at = [
                    rng(&mut st) * 40.0 - 20.0,
                    rng(&mut st) * 6.0,
                    rng(&mut st) * 40.0 - 20.0,
                ];
                let p: Vec<V> = (0..8)
                    .map(|_| {
                        [
                            at[0] + rng(&mut st) * 3.0,
                            at[1] + rng(&mut st) * 3.0,
                            at[2] + rng(&mut st) * 3.0,
                        ]
                    })
                    .collect();
                if b.hull(&p) {
                    all.push(p);
                }
            }
            let s = b.build();
            let mut one = Vec::new();
            for p in &all {
                let mut sb = SolidBuilder::new();
                assert!(sb.hull(p));
                one.push(sb.build());
            }
            for _ in 0..200 {
                let f = [
                    (rng(&mut st) * 50.0 - 25.0) as f32,
                    (rng(&mut st) * 8.0) as f32,
                    (rng(&mut st) * 50.0 - 25.0) as f32,
                ];
                let t = [
                    (rng(&mut st) * 50.0 - 25.0) as f32,
                    (rng(&mut st) * 8.0) as f32,
                    (rng(&mut st) * 50.0 - 25.0) as f32,
                ];
                let each = one.iter().all(|o| o.clear(f, t));
                assert_eq!(s.clear(f, t), each, "world {world}");
                blocked += usize::from(!each);
            }
        }
        assert!(blocked > 100, "{blocked}");
    }
}
