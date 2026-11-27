//! Godot Physics' box cast (`PhysicsDirectSpaceState3D.cast_motion`) for one shape, in the
//! engine's own f32 order: the same GJK (`gjk_epa.cpp`), the same supports, the same concave
//! cull and the same eight bisection steps, so the safe fraction is Godot's to the bit. The
//! piece SAT in [`crate::piece`] gives a box's true contact; Godot's answer differs from it
//! wherever its GJK ends early (touching within 1e-4, or a degenerate simplex reading as
//! separated), and the gate is Godot's answer.
//!
//! Two shortcuts keep it fast without changing an answer: a step whose swept box stays
//! [`CLEAR`] away from a shape's (or a face's) bounds is separated without running GJK (GJK
//! reports contact only within 1e-4), and the concave cull runs once per cast, not once per
//! step (its box is the cast's, the same every step).

use crate::piece::V3;

/// An f32 vector, as Godot's `Vector3` with `real_t` float.
pub type F3 = [f32; 3];

const MIN_DISTANCE: f32 = 0.0001;
const DUPLICATED_EPS: f32 = 0.0001;
const ACCURACY: f32 = 0.0001;
const MAX_ITERATIONS: u32 = 128;
const IMD3: [usize; 3] = [1, 2, 0];
/// A swept box this far clear of a shape's bounds is separated from it for GJK.
pub const CLEAR: f64 = 0.01;
const LEAF: usize = 4;

#[inline]
fn add(a: F3, b: F3) -> F3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
fn sub(a: F3, b: F3) -> F3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn neg(a: F3) -> F3 {
    [-a[0], -a[1], -a[2]]
}

#[inline]
fn mul(a: F3, s: f32) -> F3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
fn div(a: F3, s: f32) -> F3 {
    [a[0] / s, a[1] / s, a[2] / s]
}

#[inline]
fn dot(a: F3, b: F3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn cross(a: F3, b: F3) -> F3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
fn lsq(a: F3) -> f32 {
    dot(a, a)
}

#[inline]
fn len(a: F3) -> f32 {
    lsq(a).sqrt()
}

#[inline]
fn normalized(a: F3) -> F3 {
    let l = lsq(a);
    if l == 0.0 { [0.0; 3] } else { div(a, l.sqrt()) }
}

/// An f64 vector rounded to f32, as Godot stores a `Vector3`.
pub fn narrow(a: V3) -> F3 {
    a.map(|c| c as f32)
}

/// Godot's `Transform3D` in f32: basis rows and origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub rows: [F3; 3],
    pub origin: F3,
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        origin: [0.0; 3],
    };

    /// `self * b`, as Godot's `Transform3D::operator*`.
    pub fn mul(&self, b: &Xf) -> Xf {
        let (a, br) = (&self.rows, &b.rows);
        let mut rows = [[0.0_f32; 3]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, c) in row.iter_mut().enumerate() {
                *c = br[0][j] * a[i][0] + br[1][j] * a[i][1] + br[2][j] * a[i][2];
            }
        }
        Xf {
            rows,
            origin: self.xform(b.origin),
        }
    }

    /// A point, as `Transform3D::xform`.
    #[inline]
    pub fn xform(&self, v: F3) -> F3 {
        [
            dot(self.rows[0], v) + self.origin[0],
            dot(self.rows[1], v) + self.origin[1],
            dot(self.rows[2], v) + self.origin[2],
        ]
    }

    /// A direction through the transposed basis, as `Basis::xform_inv`.
    #[inline]
    fn xform_inv(&self, v: F3) -> F3 {
        let r = &self.rows;
        [
            r[0][0] * v[0] + r[1][0] * v[1] + r[2][0] * v[2],
            r[0][1] * v[0] + r[1][1] * v[1] + r[2][1] * v[2],
            r[0][2] * v[0] + r[1][2] * v[1] + r[2][2] * v[2],
        ]
    }

    #[inline]
    fn column(&self, i: usize) -> F3 {
        [self.rows[0][i], self.rows[1][i], self.rows[2][i]]
    }

    /// The twelve floats, rows then origin.
    pub fn floats(&self) -> [f32; 12] {
        let r = &self.rows;
        let o = &self.origin;
        [
            r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2], o[0],
            o[1], o[2],
        ]
    }

    pub fn from_floats(f: [f32; 12]) -> Xf {
        Xf {
            rows: [[f[0], f[1], f[2]], [f[3], f[4], f[5]], [f[6], f[7], f[8]]],
            origin: [f[9], f[10], f[11]],
        }
    }
}

/// Godot's `AABB` in f32: position and size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb {
    pub pos: F3,
    pub size: F3,
}

impl Aabb {
    #[inline]
    fn intersects(&self, b: &Aabb) -> bool {
        (0..3).all(|a| self.pos[a] < b.pos[a] + b.size[a] && self.pos[a] + self.size[a] > b.pos[a])
    }

    fn expand_to(&mut self, p: F3) {
        let mut begin = self.pos;
        let mut end = add(self.pos, self.size);
        for a in 0..3 {
            if p[a] < begin[a] {
                begin[a] = p[a];
            }
            if p[a] > end[a] {
                end[a] = p[a];
            }
        }
        self.pos = begin;
        self.size = sub(end, begin);
    }

    /// `Face3::get_aabb`.
    fn of_face(v: &[F3]) -> Aabb {
        let mut b = Aabb {
            pos: v[0],
            size: [0.0; 3],
        };
        b.expand_to(v[1]);
        b.expand_to(v[2]);
        b
    }

    /// `project_range_in_plane(Plane(axis, 0))`.
    fn project(&self, axis: F3) -> (f32, f32) {
        let half = mul(self.size, 0.5);
        let center = add(self.pos, half);
        let length = dot(axis.map(f32::abs), half);
        let distance = dot(axis, center) - 0.0;
        (distance - length, distance + length)
    }
}

/// The box `cast_motion` culls with and hands the concave solver: the query box at `from`
/// merged with itself moved by `motion` (margin 0).
pub fn cast_aabb(from: F3, half: F3, motion: F3) -> Aabb {
    let pos = add(from, neg(half));
    let size = sub(add(from, half), sub(from, half));
    let beg2 = add(pos, motion);
    let (end1, end2) = (add(size, pos), add(size, beg2));
    let mut min = [0.0_f32; 3];
    let mut max = [0.0_f32; 3];
    for a in 0..3 {
        min[a] = if beg2[a] < pos[a] { beg2[a] } else { pos[a] };
        max[a] = if end2[a] > end1[a] { end2[a] } else { end1[a] };
    }
    Aabb {
        pos: min,
        size: sub(max, min),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FNode {
    lo: F3,
    hi: F3,
    /// Leaf: first entry in `order`; inner: the right child (the left is the next node).
    first: u32,
    /// Faces in a leaf; 0 for an inner node.
    count: u32,
}

/// A concave shape's triangles (three verts each, shape space) and a hierarchy over Godot's
/// per-face boxes.
#[derive(Clone, Debug, PartialEq)]
pub struct Faces {
    pub verts: Vec<F3>,
    leaves: Vec<Aabb>,
    nodes: Vec<FNode>,
    order: Vec<u32>,
}

impl Faces {
    fn new(verts: Vec<F3>) -> Option<Faces> {
        if verts.is_empty() || !verts.len().is_multiple_of(3) {
            return None;
        }
        let leaves: Vec<Aabb> = verts
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| Aabb::of_face(c))
            .collect();
        let mut f = Faces {
            order: (0..leaves.len() as u32).collect(),
            leaves,
            nodes: Vec::new(),
            verts,
        };
        let n = f.leaves.len();
        f.build(0, n);
        Some(f)
    }

    fn build(&mut self, first: usize, count: usize) -> usize {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for &i in &self.order[first..first + count] {
            let b = self.leaves[i as usize];
            let end = add(b.pos, b.size);
            for a in 0..3 {
                lo[a] = lo[a].min(b.pos[a]);
                hi[a] = hi[a].max(end[a]);
            }
        }
        let at = self.nodes.len();
        self.nodes.push(FNode {
            lo,
            hi,
            first: first as u32,
            count: count as u32,
        });
        if count <= LEAF {
            return at;
        }
        let axis = (0..3).fold(0, |b, a| if hi[a] - lo[a] > hi[b] - lo[b] { a } else { b });
        let leaves = &self.leaves;
        let centre = |i: u32| leaves[i as usize].pos[axis] * 2.0 + leaves[i as usize].size[axis];
        let half = count / 2;
        self.order[first..first + count]
            .select_nth_unstable_by(half, |a, b| centre(*a).total_cmp(&centre(*b)));
        self.build(first, half);
        let right = self.build(first + half, count - half);
        self.nodes[at].first = right as u32;
        self.nodes[at].count = 0;
        at
    }

    /// Every face whose box Godot's cull keeps for `local` (any order).
    fn cull(&self, local: &Aabb, mut visit: impl FnMut(u32)) {
        let end = add(local.pos, local.size);
        let mut stack = [0u32; 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let idx = stack[top];
            let node = self.nodes[idx as usize];
            if !(0..3).all(|a| local.pos[a] <= node.hi[a] && end[a] >= node.lo[a]) {
                continue;
            }
            if node.count > 0 {
                for &i in &self.order[node.first as usize..(node.first + node.count) as usize] {
                    if local.intersects(&self.leaves[i as usize]) {
                        visit(i);
                    }
                }
            } else if top + 2 <= stack.len() {
                stack[top] = node.first;
                stack[top + 1] = idx + 1;
                top += 2;
            }
        }
    }
}

/// A Godot shape's geometry in shape space.
#[derive(Clone, Debug, PartialEq)]
pub enum Geom {
    /// `BoxShape3D`: half extents.
    Box(F3),
    /// `ConvexPolygonShape3D`: the server's hull vertices and their extreme vertices.
    Hull { verts: Vec<F3>, extreme: Vec<u32> },
    /// `ConcavePolygonShape3D`.
    Faces(Faces),
}

impl Geom {
    /// The hull of the server's vertices (`shape_get_data`), in its order.
    pub fn hull(verts: Vec<F3>) -> Option<Geom> {
        if verts.is_empty() {
            return None;
        }
        let mut extreme: Vec<u32> = Vec::new();
        for x in -1..2_i32 {
            for y in -1..2_i32 {
                for z in -1..2_i32 {
                    if x == 0 && y == 0 && z == 0 {
                        continue;
                    }
                    let dir = normalized([x as f32, y as f32, z as f32]);
                    let (mut max, mut best) = (0.0_f32, usize::MAX);
                    for (i, v) in verts.iter().enumerate() {
                        let s = dot(dir, *v);
                        if best == usize::MAX || s > max {
                            best = i;
                            max = s;
                        }
                    }
                    if !extreme.contains(&(best as u32)) {
                        extreme.push(best as u32);
                    }
                }
            }
        }
        Some(Geom::Hull { verts, extreme })
    }

    /// A trimesh: three verts per face (`get_faces`).
    pub fn faces(verts: Vec<F3>) -> Option<Geom> {
        Faces::new(verts).map(Geom::Faces)
    }

    fn points(&self) -> Vec<F3> {
        match self {
            Geom::Box(h) => (0..8)
                .map(|k| {
                    [
                        if k & 1 == 0 { -h[0] } else { h[0] },
                        if k & 2 == 0 { -h[1] } else { h[1] },
                        if k & 4 == 0 { -h[2] } else { h[2] },
                    ]
                })
                .collect(),
            Geom::Hull { verts, .. } => verts.clone(),
            Geom::Faces(f) => f.verts.clone(),
        }
    }
}

#[derive(Clone, Copy)]
enum Conv<'a> {
    Box(F3),
    Hull(&'a [F3], &'a [u32]),
    Face(&'a [F3]),
}

#[inline]
fn box_support(h: F3, n: F3) -> F3 {
    [
        if n[0] < 0.0 { -h[0] } else { h[0] },
        if n[1] < 0.0 { -h[1] } else { h[1] },
        if n[2] < 0.0 { -h[2] } else { h[2] },
    ]
}

impl Conv<'_> {
    #[inline]
    fn support(&self, n: F3) -> F3 {
        match *self {
            Conv::Box(h) => box_support(h, n),
            Conv::Hull(verts, extreme) => {
                let mut best = extreme[0] as usize;
                let mut max = dot(n, verts[best]);
                for &e in extreme {
                    let s = dot(n, verts[e as usize]);
                    if s > max {
                        best = e as usize;
                        max = s;
                    }
                }
                if extreme.len() != verts.len() {
                    // Godot climbs the hull's edges from here; on a convex hull the climb ends
                    // at the maximum this scan finds.
                    for (i, v) in verts.iter().enumerate() {
                        let s = dot(n, *v);
                        if s > max {
                            best = i;
                            max = s;
                        }
                    }
                }
                verts[best]
            }
            Conv::Face(v) => {
                let mut best = 0;
                let mut max = dot(n, v[0]);
                for (i, p) in v.iter().enumerate().skip(1) {
                    let d = dot(n, *p);
                    if d > max {
                        max = d;
                        best = i;
                    }
                }
                v[best]
            }
        }
    }
}

/// The moving query box: `GodotMotionShape3D` over a box at `from` (identity basis).
#[derive(Clone, Copy)]
struct Probe {
    from: F3,
    half: F3,
    motion: F3,
}

impl Probe {
    #[inline]
    fn support(&self, d: F3) -> F3 {
        let n = normalized(d);
        let mut s = box_support(self.half, n);
        if dot(n, self.motion) > 0.0 {
            s = add(s, self.motion);
        }
        // `Transform3D::xform` with an identity basis.
        [
            s[0] + 0.0 * s[1] + 0.0 * s[2] + self.from[0],
            0.0 * s[0] + s[1] + 0.0 * s[2] + self.from[1],
            0.0 * s[0] + 0.0 * s[1] + s[2] + self.from[2],
        ]
    }
}

fn proj2(a: F3, b: F3, w: &mut [f32], m: &mut u32) -> f32 {
    let d = sub(b, a);
    let l = lsq(d);
    if l > 0.0 {
        let t = -dot(a, d) / l;
        if t >= 1.0 {
            w[0] = 0.0;
            w[1] = 1.0;
            *m = 2;
            return lsq(b);
        } else if t <= 0.0 {
            w[0] = 1.0;
            w[1] = 0.0;
            *m = 1;
            return lsq(a);
        }
        w[1] = t;
        w[0] = 1.0 - t;
        *m = 3;
        return lsq(add(a, mul(d, t)));
    }
    -1.0
}

fn proj3(a: F3, b: F3, c: F3, w: &mut [f32], m: &mut u32) -> f32 {
    let vt = [a, b, c];
    let dl = [sub(a, b), sub(b, c), sub(c, a)];
    let n = cross(dl[0], dl[1]);
    let l = lsq(n);
    if l > 0.0 {
        let mut mindist = -1.0_f32;
        let mut subw = [0.0_f32; 2];
        let mut subm = 0_u32;
        for i in 0..3 {
            if dot(vt[i], cross(dl[i], n)) > 0.0 {
                let j = IMD3[i];
                let subd = proj2(vt[i], vt[j], &mut subw, &mut subm);
                if mindist < 0.0 || subd < mindist {
                    mindist = subd;
                    *m = (if subm & 1 != 0 { 1 << i } else { 0 })
                        + (if subm & 2 != 0 { 1 << j } else { 0 });
                    w[i] = subw[0];
                    w[j] = subw[1];
                    w[IMD3[j]] = 0.0;
                }
            }
        }
        if mindist < 0.0 {
            let d = dot(a, n);
            let s = l.sqrt();
            let p = mul(n, d / l);
            mindist = lsq(p);
            *m = 7;
            w[0] = len(cross(dl[1], sub(b, p))) / s;
            w[1] = len(cross(dl[2], sub(c, p))) / s;
            w[2] = 1.0 - (w[0] + w[1]);
        }
        return mindist;
    }
    -1.0
}

#[inline]
fn det(a: F3, b: F3, c: F3) -> f32 {
    a[1] * b[2] * c[0] + a[2] * b[0] * c[1] - a[0] * b[2] * c[1] - a[1] * b[0] * c[2]
        + a[0] * b[1] * c[2]
        - a[2] * b[1] * c[0]
}

fn proj4(a: F3, b: F3, c: F3, d: F3, w: &mut [f32; 4], m: &mut u32) -> f32 {
    let vt = [a, b, c];
    let dl = [sub(a, d), sub(b, d), sub(c, d)];
    let vl = det(dl[0], dl[1], dl[2]);
    let ng = (vl * dot(a, cross(sub(b, c), sub(a, b)))) <= 0.0;
    if ng && vl.abs() > 0.0 {
        let mut mindist = -1.0_f32;
        let mut subw = [0.0_f32; 3];
        let mut subm = 0_u32;
        for i in 0..3 {
            let j = IMD3[i];
            let s = vl * dot(d, cross(dl[i], dl[j]));
            if s > 0.0 {
                let subd = proj3(vt[i], vt[j], d, &mut subw, &mut subm);
                if mindist < 0.0 || subd < mindist {
                    mindist = subd;
                    *m = (if subm & 1 != 0 { 1 << i } else { 0 })
                        + (if subm & 2 != 0 { 1 << j } else { 0 })
                        + (if subm & 4 != 0 { 8 } else { 0 });
                    w[i] = subw[0];
                    w[j] = subw[1];
                    w[IMD3[j]] = 0.0;
                    w[3] = subw[2];
                }
            }
        }
        if mindist < 0.0 {
            mindist = 0.0;
            *m = 15;
            w[0] = det(c, b, d) / vl;
            w[1] = det(a, c, d) / vl;
            w[2] = det(b, a, d) / vl;
            w[3] = 1.0 - (w[0] + w[1] + w[2]);
        }
        return mindist;
    }
    -1.0
}

/// `gjk_epa_calculate_distance` with the probe as shape A and `conv` posed by `tb`: true when
/// GJK ends Valid (separated), false for Inside or Failed (touching).
fn separated(probe: &Probe, conv: Conv, tb: &Xf) -> bool {
    let support = |v: F3| -> F3 {
        let d = div(v, len(v));
        let a = probe.support(d);
        let n = normalized(tb.xform_inv(neg(d)));
        sub(a, tb.xform(conv.support(n)))
    };
    let mut sim = [[[0.0_f32; 3]; 4]; 2];
    let mut rank = [0_usize; 2];
    let mut cur = 0;
    let guess = sub(tb.origin, probe.from);
    let sqrl = lsq(guess);
    sim[0][0] = support(if sqrl > 0.0 {
        neg(guess)
    } else {
        [1.0, 0.0, 0.0]
    });
    rank[0] = 1;
    let mut ray = sim[0][0];
    let mut lastw = [ray; 4];
    let mut clastw = 0_usize;
    let mut alpha = 0.0_f32;
    let mut iterations = 0_u32;
    loop {
        let next = 1 - cur;
        let rl = len(ray);
        if rl < MIN_DISTANCE {
            return false;
        }
        let w = support(neg(ray));
        let r = rank[cur];
        sim[cur][r] = w;
        rank[cur] = r + 1;
        if lastw.iter().any(|l| lsq(sub(w, *l)) < DUPLICATED_EPS) {
            return true;
        }
        clastw = (clastw + 1) & 3;
        lastw[clastw] = w;
        let omega = dot(ray, w) / rl;
        alpha = if omega > alpha { omega } else { alpha };
        if ((rl - alpha) - (ACCURACY * rl)) <= 0.0 {
            return true;
        }
        let mut weights = [0.0_f32; 4];
        let mut mask = 0_u32;
        let s = sim[cur];
        let sqdist = match rank[cur] {
            2 => proj2(s[0], s[1], &mut weights, &mut mask),
            3 => proj3(s[0], s[1], s[2], &mut weights, &mut mask),
            _ => proj4(s[0], s[1], s[2], s[3], &mut weights, &mut mask),
        };
        if sqdist < 0.0 {
            return true;
        }
        let mut nr = 0;
        let mut acc = [0.0_f32; 3];
        for (i, v) in s.iter().enumerate().take(rank[cur]) {
            if mask & (1 << i) != 0 {
                sim[next][nr] = *v;
                nr += 1;
                acc = add(acc, mul(*v, weights[i]));
            }
        }
        rank[next] = nr;
        ray = acc;
        cur = next;
        if mask == 15 {
            return false;
        }
        iterations += 1;
        if iterations >= MAX_ITERATIONS {
            return false;
        }
    }
}

/// A Godot collision shape posed in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct GShape {
    pub geom: Geom,
    /// `body transform * shape owner transform`, as the server composes it.
    pub xf: Xf,
    pub layer: u32,
    /// The caller's shape id (the pieces it was cut into carry the same).
    pub shape: u32,
    /// The pieces it was cut into, `[first, end)` in the mirror's list, for the hit normal.
    pub pieces: [u32; 2],
    /// World bounds of every point a support can return.
    pub lo: V3,
    pub hi: V3,
    face_bounds: Vec<[V3; 2]>,
}

fn bounds(points: &[F3]) -> [V3; 2] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for a in 0..3 {
            lo[a] = lo[a].min(f64::from(p[a]));
            hi[a] = hi[a].max(f64::from(p[a]));
        }
    }
    [lo, hi]
}

/// What a cast tests on each step.
enum Plan<'a> {
    Conv(Conv<'a>, f64),
    Faces(&'a Faces, &'a [(u32, f64)]),
}

impl GShape {
    pub fn new(geom: Geom, xf: Xf, layer: u32, shape: u32, pieces: [u32; 2]) -> GShape {
        let mut s = GShape {
            geom,
            xf,
            layer,
            shape,
            pieces,
            lo: [0.0; 3],
            hi: [0.0; 3],
            face_bounds: Vec::new(),
        };
        s.pose(xf);
        s
    }

    /// Moves the shape to `xf` (a mover's new pose times its shape transform).
    pub fn pose(&mut self, xf: Xf) {
        self.xf = xf;
        let world: Vec<F3> = self.geom.points().iter().map(|p| xf.xform(*p)).collect();
        [self.lo, self.hi] = bounds(&world);
        self.face_bounds = match self.geom {
            Geom::Faces(_) => world.as_chunks::<3>().0.iter().map(|c| bounds(c)).collect(),
            _ => Vec::new(),
        };
    }

    fn step(&self, plan: &Plan, from: F3, half: F3, motion: F3, f: f32) -> bool {
        let probe = Probe { from, half, motion };
        let f = f64::from(f);
        match *plan {
            Plan::Conv(conv, t0) => f < t0 || separated(&probe, conv, &self.xf),
            Plan::Faces(faces, list) => list.iter().all(|&(i, t)| {
                let k = i as usize * 3;
                f < t || separated(&probe, Conv::Face(&faces.verts[k..k + 3]), &self.xf)
            }),
        }
    }

    /// Godot's `(safe, unsafe)` fractions for the box of `half` at `from` moved by `motion`, or
    /// `None` when `cast_motion` skips this shape (never touched, or touching at the start).
    /// `aabb` is [`cast_aabb`]; `entry(lo, hi)` is the fraction where the swept box first
    /// comes within [`CLEAR`] of those bounds, `None` for never. `scratch` is reused.
    pub fn cast(
        &self,
        from: F3,
        motion: F3,
        half: F3,
        aabb: &Aabb,
        entry: impl Fn(V3, V3) -> Option<f64>,
        scratch: &mut Vec<(u32, f64)>,
    ) -> Option<(f32, f32)> {
        let t0 = entry(self.lo, self.hi)?;
        let plan = match &self.geom {
            Geom::Box(h) => Plan::Conv(Conv::Box(*h), t0),
            Geom::Hull { verts, extreme } => Plan::Conv(Conv::Hull(verts, extreme), t0),
            Geom::Faces(faces) => {
                scratch.clear();
                faces.cull(&self.local_hint(aabb), |i| {
                    let [lo, hi] = self.face_bounds[i as usize];
                    if let Some(t) = entry(lo, hi) {
                        scratch.push((i, t));
                    }
                });
                if scratch.is_empty() {
                    return None;
                }
                Plan::Faces(faces, scratch)
            }
        };
        if self.step(&plan, from, half, motion, 1.0) || !self.step(&plan, from, half, [0.0; 3], 0.0)
        {
            return None;
        }
        let (mut low, mut hi, mut coeff) = (0.0_f32, 1.0_f32, 0.5_f32);
        for j in 0..8 {
            let fraction = low + (hi - low) * coeff;
            if !self.step(&plan, from, half, mul(motion, fraction), fraction) {
                hi = fraction;
                coeff = if j == 0 || low > 0.0 { 0.5 } else { 0.25 };
            } else {
                low = fraction;
                coeff = if j == 0 || hi < 1.0 { 0.5 } else { 0.75 };
            }
        }
        Some((low, hi))
    }

    /// The concave solver's local box from the cast's box (`solve_distance`, hint branch).
    fn local_hint(&self, aabb: &Aabb) -> Aabb {
        let hint = Aabb {
            pos: sub(aabb.pos, self.xf.origin),
            size: aabb.size,
        };
        let mut local = Aabb::default();
        for i in 0..3 {
            let mut axis = self.xf.column(i);
            let scale = 1.0 / len(axis);
            axis = mul(axis, scale);
            let (smin, smax) = hint.project(axis);
            let (smin, smax) = (smin * scale, smax * scale);
            local.pos[i] = smin;
            local.size[i] = smax - smin;
        }
        local
    }
}
