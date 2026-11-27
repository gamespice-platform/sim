//! One convex piece of a collision mirror: a solid (a box or convex hull, like a Source brush) or
//! one triangle of a trimesh. Solids have an inside (a ray starting in one passes out of it, a
//! point in one is in the solid's contents); triangles have none. Both are convex polytopes for
//! the swept separating-axis test, which is how a box sweep finds its time of first contact.

/// A point or direction in metres, `f64` (the engine's `f32` widened on the way in).
pub type V3 = [f64; 3];

#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `a` scaled to length 1, or `None` when it is too short to have a direction.
#[inline]
pub fn unit(a: V3) -> Option<V3> {
    let l = dot(a, a).sqrt();
    if l < 1e-12 {
        None
    } else {
        Some(scale(a, 1.0 / l))
    }
}

#[inline]
fn normal_of(p: &[f64; 4]) -> V3 {
    [p[0], p[1], p[2]]
}

/// A 3x3 matrix by columns (Godot's `Basis` columns: x, y, z axes) and a translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub cols: [V3; 3],
    pub origin: V3,
}

impl Xform {
    pub const IDENTITY: Xform = Xform {
        cols: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        origin: [0.0; 3],
    };

    #[inline]
    pub fn vector(&self, v: V3) -> V3 {
        add(
            add(scale(self.cols[0], v[0]), scale(self.cols[1], v[1])),
            scale(self.cols[2], v[2]),
        )
    }

    #[inline]
    pub fn point(&self, p: V3) -> V3 {
        add(self.vector(p), self.origin)
    }

    /// Where a plane normal goes: the cofactor matrix (inverse transpose up to scale), unit length.
    pub fn normal(&self, n: V3) -> V3 {
        let [a, b, c] = self.cols;
        let cof = [cross(b, c), cross(c, a), cross(a, b)];
        let det = dot(a, cof[0]);
        let m = add(
            add(scale(cof[0], n[0]), scale(cof[1], n[1])),
            scale(cof[2], n[2]),
        );
        unit(scale(m, det.signum())).unwrap_or(n)
    }
}

/// One convex piece. `planes` are outward `(n, d)` with the inside at `n . x <= d`; a triangle
/// has the one plane Godot's `Plane(a, b, c)` gives it. `edges` are the distinct edge directions.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub verts: Vec<V3>,
    pub planes: Vec<[f64; 4]>,
    pub edges: Vec<V3>,
    pub solid: bool,
    /// The collision layer bits of the body it came from; a query's mask must share one.
    pub layer: u32,
    /// Which engine shape it came from (a trimesh gives many pieces one id): a box sweep ignores
    /// every shape it starts inside of, as Godot's `cast_motion` does.
    pub shape: u32,
    pub lo: V3,
    pub hi: V3,
}

/// How far a point may sit off a plane and still be on it, for points near `extent` metres from
/// the origin (the engine's `f32` loses about 6e-8 of the magnitude).
fn tolerance(points: &[V3]) -> f64 {
    let extent = points
        .iter()
        .flat_map(|p| p.iter())
        .fold(1.0_f64, |m, c| m.max(c.abs()));
    1e-6 * extent
}

fn push_plane(planes: &mut Vec<[f64; 4]>, n: V3, d: f64, eps: f64) {
    let dup = planes
        .iter()
        .any(|p| dot(normal_of(p), n) > 1.0 - 1e-9 && (p[3] - d).abs() <= eps);
    if !dup {
        planes.push([n[0], n[1], n[2], d]);
    }
}

fn push_dir(dirs: &mut Vec<V3>, e: V3) {
    if !dirs.iter().any(|q| dot(*q, e).abs() > 1.0 - 1e-9) {
        dirs.push(e);
    }
}

/// The side of the point set `n . p - d` falls on: `Some(true)` all at or below, `Some(false)`
/// all at or above, `None` both sides.
fn side(points: &[V3], n: V3, d: f64, eps: f64) -> Option<bool> {
    let (mut below, mut above) = (true, true);
    for p in points {
        let s = dot(n, *p) - d;
        below &= s <= eps;
        above &= s >= -eps;
        if !below && !above {
            return None;
        }
    }
    Some(below)
}

/// Pushes `(n, d)` facing away from the points, when every point lies on one side of it.
fn push_supporting(planes: &mut Vec<[f64; 4]>, pts: &[V3], n: V3, d: f64, eps: f64) {
    match side(pts, n, d, eps) {
        Some(true) => push_plane(planes, n, d, eps),
        Some(false) => push_plane(planes, scale(n, -1.0), -d, eps),
        None => {}
    }
}

fn dedup_points(points: &[V3], eps: f64) -> Vec<V3> {
    let mut pts: Vec<V3> = Vec::with_capacity(points.len());
    for p in points {
        if !pts
            .iter()
            .any(|q| dot(sub(*p, *q), sub(*p, *q)) <= eps * eps)
        {
            pts.push(*p);
        }
    }
    pts
}

/// The hull's faces: every supporting plane through three of the points. A flat set gets its two
/// faces plus a plane through each rim edge, so it stays bounded.
fn hull_planes(pts: &[V3], eps: f64) -> Vec<[f64; 4]> {
    let n = pts.len();
    let mut planes: Vec<[f64; 4]> = Vec::new();
    let mut flat: Option<V3> = None;
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let Some(nn) = unit(cross(sub(pts[j], pts[i]), sub(pts[k], pts[i]))) else {
                    continue;
                };
                let d = dot(nn, pts[i]);
                if pts.iter().all(|p| (dot(nn, *p) - d).abs() <= eps) {
                    flat = Some(nn);
                    push_plane(&mut planes, nn, d, eps);
                    push_plane(&mut planes, scale(nn, -1.0), -d, eps);
                } else {
                    push_supporting(&mut planes, pts, nn, d, eps);
                }
            }
        }
    }
    if let Some(face) = flat {
        for i in 0..n {
            for j in i + 1..n {
                if let Some(m) = unit(cross(sub(pts[j], pts[i]), face)) {
                    push_supporting(&mut planes, pts, m, dot(m, pts[i]), eps);
                }
            }
        }
    }
    planes
}

/// The hull's edge directions: the line of each pair of faces that share two or more points.
fn hull_edges(pts: &[V3], planes: &[[f64; 4]], eps: f64) -> Vec<V3> {
    let mut edges: Vec<V3> = Vec::new();
    let on = |p: &[f64; 4], v: &V3| (dot(normal_of(p), *v) - p[3]).abs() <= eps;
    for a in 0..planes.len() {
        for b in a + 1..planes.len() {
            let Some(e) = unit(cross(normal_of(&planes[a]), normal_of(&planes[b]))) else {
                continue;
            };
            let shared = pts
                .iter()
                .filter(|v| on(&planes[a], v) && on(&planes[b], v))
                .count();
            if shared >= 2 {
                push_dir(&mut edges, e);
            }
        }
    }
    edges
}

impl Piece {
    fn bounded(
        verts: Vec<V3>,
        planes: Vec<[f64; 4]>,
        edges: Vec<V3>,
        solid: bool,
        layer: u32,
    ) -> Piece {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &verts {
            for a in 0..3 {
                lo[a] = lo[a].min(v[a]);
                hi[a] = hi[a].max(v[a]);
            }
        }
        Piece {
            verts,
            planes,
            edges,
            solid,
            layer,
            shape: 0,
            lo,
            hi,
        }
    }

    /// A piece read back from its parts (a baked mirror), bounds recomputed.
    pub fn from_parts(
        verts: Vec<V3>,
        planes: Vec<[f64; 4]>,
        edges: Vec<V3>,
        solid: bool,
        layer: u32,
    ) -> Piece {
        Piece::bounded(verts, planes, edges, solid, layer)
    }

    /// The convex hull of `points` as a solid (a `ConvexPolygonShape3D`, a brush), or `None` for
    /// fewer than 3 distinct points. Cubic in the point count: brushes have a few dozen.
    pub fn convex(points: &[V3], layer: u32) -> Option<Piece> {
        let eps = tolerance(points);
        let pts = dedup_points(points, eps);
        if pts.len() < 3 {
            return None;
        }
        let planes = hull_planes(&pts, eps);
        if planes.len() < 4 {
            return None;
        }
        let edges = hull_edges(&pts, &planes, eps);
        Some(Piece::bounded(pts, planes, edges, true, layer))
    }

    /// A box of `half` extents under `xform` (a `BoxShape3D` of size `2 * half`), as a solid.
    pub fn cuboid(xform: &Xform, half: V3, layer: u32) -> Option<Piece> {
        let mut pts = Vec::with_capacity(8);
        for i in 0..8 {
            let s = |bit: usize, h: f64| if i & bit != 0 { h } else { -h };
            pts.push(xform.point([s(1, half[0]), s(2, half[1]), s(4, half[2])]));
        }
        Piece::convex(&pts, layer)
    }

    /// One trimesh triangle, `None` when degenerate. Its plane is Godot's `Plane(a, b, c)`:
    /// normal `(a - c) x (a - b)`.
    pub fn triangle(a: V3, b: V3, c: V3, layer: u32) -> Option<Piece> {
        let n = unit(cross(sub(a, c), sub(a, b)))?;
        let mut edges = Vec::with_capacity(3);
        for e in [sub(b, a), sub(c, b), sub(a, c)] {
            push_dir(&mut edges, unit(e)?);
        }
        let d = dot(n, a);
        Some(Piece::bounded(
            vec![a, b, c],
            vec![[n[0], n[1], n[2], d]],
            edges,
            false,
            layer,
        ))
    }

    /// This piece moved by `xform` (a door's or platform's pose; the piece is in its body's space).
    pub fn transformed(&self, xform: &Xform) -> Piece {
        let verts: Vec<V3> = self.verts.iter().map(|v| xform.point(*v)).collect();
        let planes = self
            .planes
            .iter()
            .map(|p| {
                let n0 = normal_of(p);
                let n = xform.normal(n0);
                let on = xform.point(scale(n0, p[3]));
                [n[0], n[1], n[2], dot(n, on)]
            })
            .collect();
        let edges = self
            .edges
            .iter()
            .map(|e| unit(xform.vector(*e)).unwrap_or(*e))
            .collect();
        let mut moved = Piece::bounded(verts, planes, edges, self.solid, self.layer);
        moved.shape = self.shape;
        moved
    }

    /// Whether `p` is inside the solid (on a face counts); always false for a triangle.
    pub fn contains(&self, p: V3) -> bool {
        self.solid
            && self
                .planes
                .iter()
                .all(|q| dot(normal_of(q), p) - q[3] <= 1e-6)
    }

    /// The segment `from + t * delta`, `t` in `[0, 1]`, against the piece: the entry fraction and
    /// the surface normal. A solid the segment starts inside is passed through (Godot's
    /// `hit_from_inside = false`); a triangle is hit from its front only, as a trimesh without
    /// `backface_collision` is (a two-sided trimesh is staged with each face both ways).
    pub fn ray(&self, from: V3, delta: V3) -> Option<(f64, V3)> {
        if self.solid {
            self.ray_solid(from, delta)
        } else {
            self.ray_triangle(from, delta)
        }
    }

    fn ray_solid(&self, from: V3, delta: V3) -> Option<(f64, V3)> {
        let mut enter = f64::NEG_INFINITY;
        let mut exit = 1.0_f64;
        let mut normal = None;
        for p in &self.planes {
            let n = normal_of(p);
            let dist = dot(n, from) - p[3];
            let denom = dot(n, delta);
            if denom.abs() < 1e-15 {
                if dist > 0.0 {
                    return None;
                }
                continue;
            }
            let t = -dist / denom;
            if denom < 0.0 {
                if t > enter {
                    enter = t;
                    normal = Some(n);
                }
            } else if t < exit {
                exit = t;
            }
            if enter > exit {
                return None;
            }
        }
        if !(0.0..=1.0).contains(&enter) {
            return None;
        }
        normal.map(|n| (enter, n))
    }

    fn ray_triangle(&self, from: V3, delta: V3) -> Option<(f64, V3)> {
        let [a, b, c] = [self.verts[0], self.verts[1], self.verts[2]];
        let e1 = sub(b, a);
        let e2 = sub(c, a);
        let h = cross(delta, e2);
        let det = dot(e1, h);
        if det.abs() < 1e-15 {
            return None;
        }
        let inv = 1.0 / det;
        let s = sub(from, a);
        let u = inv * dot(s, h);
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = cross(s, e1);
        let v = inv * dot(delta, q);
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = inv * dot(e2, q);
        let n = normal_of(&self.planes[0]);
        if t <= 0.0 || t > 1.0 || dot(n, delta) >= 0.0 {
            return None;
        }
        Some((t, n))
    }

    /// The axis-aligned box of `half` extents centred at `from` swept by `delta` against the
    /// piece, by the separating-axis theorem over the box axes, the piece's face normals and
    /// every box axis crossed with every piece edge. Returns the first contact fraction, the
    /// normal facing the box, and whether the box starts overlapping (fraction 0 then).
    pub fn sweep(&self, from: V3, delta: V3, half: V3) -> Option<(f64, V3, bool)> {
        let (enter, exit, normal) = self.overlap_span(from, delta, half)?;
        if enter > 1.0 || exit < 0.0 {
            return None;
        }
        if enter <= 0.0 {
            return Some((0.0, normal, true));
        }
        Some((enter, normal, false))
    }

    /// The whole interval of motion fractions over which the swept box overlaps the piece
    /// (unbounded either way, a convex pair overlaps over one interval), with the normal facing
    /// the box at the entry; `None` when the box never touches the piece along the line.
    pub fn overlap_span(&self, from: V3, delta: V3, half: V3) -> Option<(f64, f64, V3)> {
        const AXES: [V3; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut span = Span {
            enter: f64::NEG_INFINITY,
            exit: f64::INFINITY,
            normal: [0.0, 1.0, 0.0],
        };
        for a in AXES {
            if !span.axis(self, a, from, delta, half) {
                return None;
            }
        }
        for plane in &self.planes {
            if !span.axis(self, normal_of(plane), from, delta, half) {
                return None;
            }
        }
        for e in &self.edges {
            for a in AXES {
                if let Some(axis) = unit(cross(a, *e))
                    && !span.axis(self, axis, from, delta, half)
                {
                    return None;
                }
            }
        }
        Some((span.enter, span.exit, span.normal))
    }
}

/// The swept test's running interval of overlap times.
struct Span {
    enter: f64,
    exit: f64,
    normal: V3,
}

impl Span {
    /// Narrows the interval by axis `a`; false once the box and piece are separated for good.
    fn axis(&mut self, piece: &Piece, a: V3, from: V3, delta: V3, half: V3) -> bool {
        let (mut pmin, mut pmax) = (f64::INFINITY, f64::NEG_INFINITY);
        for v in &piece.verts {
            let s = dot(a, *v);
            pmin = pmin.min(s);
            pmax = pmax.max(s);
        }
        let r = a[0].abs() * half[0] + a[1].abs() * half[1] + a[2].abs() * half[2];
        let c = dot(a, from);
        let lo = pmin - r - c;
        let hi = pmax + r - c;
        let s = dot(a, delta);
        if s.abs() < 1e-15 {
            return lo <= 0.0 && 0.0 <= hi;
        }
        let (t0, t1, n) = if s > 0.0 {
            (lo / s, hi / s, scale(a, -1.0))
        } else {
            (hi / s, lo / s, a)
        };
        if t0 > self.enter {
            self.enter = t0;
            self.normal = n;
        }
        self.exit = self.exit.min(t1);
        self.enter <= self.exit
    }
}
