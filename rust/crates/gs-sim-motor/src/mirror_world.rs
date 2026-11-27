//! A [`TraceWorld`] over gs-sim-trace's collision mirror: the motor's capsule collides with the
//! map's pieces instead of the engine's physics space.
//!
//! The mirror answers box sweeps and rays; the motor sweeps a capsule. This adapter does not ask
//! the mirror's box sweep. It answers the four primitive queries the engine's space answers for
//! [`Space`] (`cast_motion`, `get_rest_info`, `intersect_shape`, `intersect_ray`) over the
//! mirror's public pieces, and runs them through a fresh [`Body`]'s space path, so the body's
//! late-contact correction, retries and normal flips apply exactly as they do over the engine.
//!
//! - `cast_motion`: shapes the capsule starts inside of are ignored, and so are shapes the capsule
//!   misses over the whole motion. A trimesh is one shape: starting inside any of its triangles
//!   ignores all of them. The rest go through the engine's 8-step search for the first contact,
//!   with its 0.5/0.25 step coefficients, giving the same `(safe, unsafe)` pair.
//! - `get_rest_info`: the deepest contact deeper than [`MIN_REST_DEPTH`]. The normal points out of
//!   the piece and the point lies on the piece.
//! - `intersect_shape`: anything closer to the capsule's core segment than its radius.
//! - `intersect_ray`: the mirror's own ray.
//!
//! Distances come from GJK between the capsule's core segment and a piece's vertices. A capsule
//! deeper than its radius is pushed out along the piece's face plane that needs the least travel.
//! Movers (doors, platforms) are the mirror's dynamic pieces, posed by the caller once a tick
//! ([`Mirror::pose_movers`]); every query sees them where they were last posed. A mover's
//! contacts carry its id (the engine body's instance id), so a motor standing on one probes it as
//! ground and rides it, as it does over the engine.

use std::sync::Arc;

use gs_sim_trace::mirror::Mirror;
use gs_sim_trace::piece::{Piece, V3, add, dot, scale, sub};

use crate::trace::TraceWorld;
use crate::{Body, Contact, Hit, Masks, Space, Vec3};

/// The engine's `test_motion_min_contact_depth`: shallower rest contacts are not reported.
pub const MIN_REST_DEPTH: f64 = 1e-5;
/// `cast_motion`'s search steps.
const CAST_STEPS: usize = 8;
/// GJK's iteration cap and relative tolerance.
const GJK_ITERS: usize = 64;
const GJK_EPS: f64 = 1e-10;
/// How close counts as touching while advancing to first contact.
const TOUCH: f64 = 1e-7;

/// The motor's trace world over a collision mirror. Build once per map; share it between bodies.
#[derive(Debug)]
pub struct MirrorWorld {
    mirror: Arc<Mirror>,
    bvh: Bvh,
}

impl MirrorWorld {
    pub fn new(mirror: Arc<Mirror>) -> MirrorWorld {
        let bvh = Bvh::build(mirror.pieces());
        MirrorWorld { mirror, bvh }
    }

    pub fn mirror(&self) -> &Arc<Mirror> {
        &self.mirror
    }

    fn pieces(&self) -> &[Piece] {
        self.mirror.pieces()
    }
}

impl TraceWorld for MirrorWorld {
    fn sweep(&self, from: Vec3, motion: Vec3, height: f64, radius: f64, m: Masks) -> Hit {
        Body::default().sweep(&mut Probe::new(self), from, motion, height, radius, m)
    }

    fn rest_contact(&self, at: Vec3, height: f64, radius: f64, m: Masks) -> Hit {
        Body::default().rest_contact(&mut Probe::new(self), at, height, radius, m)
    }

    fn overlaps(&self, at: Vec3, height: f64, radius: f64, m: Masks) -> bool {
        Body::default().overlaps(&mut Probe::new(self), at, height, radius, m)
    }

    fn ray(&self, from: Vec3, to: Vec3, mask: i64) -> Option<Contact> {
        Probe::new(self).ray(from, to, mask)
    }
}

fn v3(v: Vec3) -> V3 {
    [v.x as f64, v.y as f64, v.z as f64]
}

fn vec(v: V3) -> Vec3 {
    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)
}

fn collider_id(p: &Piece) -> i64 {
    p.shape as i64 + 1
}

/// One body's queries against the world: the capsule as the body last sized it.
struct Probe<'a> {
    w: &'a MirrorWorld,
    radius: f64,
    height: f64,
    mask: u32,
}

impl<'a> Probe<'a> {
    fn new(w: &'a MirrorWorld) -> Self {
        Probe {
            w,
            radius: 0.0,
            height: 0.0,
            mask: u32::MAX,
        }
    }

    fn capsule(&self) -> Capsule {
        Capsule {
            half: (self.height * 0.5 - self.radius).max(0.0),
            radius: self.radius,
        }
    }

    /// Every piece on the mask whose box meets `lo`..`hi`, static then movers, with the collider
    /// id a contact on it reports.
    fn near(&self, lo: V3, hi: V3, mut f: impl FnMut(&Piece, i64)) {
        let pieces = self.w.pieces();
        self.w.bvh.query(lo, hi, |i| {
            let p = &pieces[i];
            if p.layer & self.mask != 0 {
                f(p, collider_id(p));
            }
        });
        self.w.mirror.dynamic_near(lo, hi, self.mask, f);
    }
}

impl Space for Probe<'_> {
    fn set_radius(&mut self, radius: f32) {
        self.radius = radius as f64;
    }

    fn set_height(&mut self, height: f32) {
        self.height = height as f64;
    }

    fn set_mask(&mut self, mask: i64) {
        self.mask = mask as u32;
    }

    fn cast(&mut self, from: Vec3, motion: Vec3) -> Option<(f32, f32)> {
        let c = self.capsule();
        let (from, motion) = (v3(from), v3(motion));
        let (lo, hi) = c.bounds(from);
        let to = add(from, motion);
        let (lo2, hi2) = c.bounds(to);
        let lo = [lo[0].min(lo2[0]), lo[1].min(lo2[1]), lo[2].min(lo2[2])];
        let hi = [hi[0].max(hi2[0]), hi[1].max(hi2[1]), hi[2].max(hi2[2])];
        // Per engine shape (collider, shape): (key, started inside, first contact).
        let mut shapes: Vec<((i64, u32), bool, f64)> = Vec::new();
        self.near(lo, hi, |p, id| {
            let key = (id, p.shape);
            let k = match shapes.iter().position(|s| s.0 == key) {
                Some(k) => k,
                None => {
                    shapes.push((key, false, f64::INFINITY));
                    shapes.len() - 1
                }
            };
            if shapes[k].1 {
                return;
            }
            if c.closest(from, p).dist < c.radius {
                shapes[k].1 = true;
                return;
            }
            if let Some(t) = c.first_contact(from, motion, p) {
                shapes[k].2 = shapes[k].2.min(t);
            }
        });
        let (mut safe, mut unsafe_) = (1.0_f64, 1.0_f64);
        for &(_, inside, t) in &shapes {
            if inside || t > 1.0 {
                continue;
            }
            let (low, hi) = cast_search(t);
            if low < safe {
                safe = low;
                unsafe_ = hi;
            }
        }
        Some((safe as f32, unsafe_ as f32))
    }

    fn rest(&mut self, at: Vec3) -> Option<Contact> {
        let c = self.capsule();
        let at = v3(at);
        let (lo, hi) = c.bounds(at);
        let mut best: Option<(f64, V3, V3, i64)> = None;
        self.near(lo, hi, |p, id| {
            if let Some((n, point, depth)) = c.contact(at, p)
                && depth > MIN_REST_DEPTH
                && best.is_none_or(|b| depth > b.0)
            {
                best = Some((depth, n, point, id));
            }
        });
        best.map(|(_, n, point, id)| Contact {
            normal: vec(n),
            point: vec(point),
            collider_id: id,
        })
    }

    fn overlap(&mut self, at: Vec3) -> bool {
        let c = self.capsule();
        let at = v3(at);
        let (lo, hi) = c.bounds(at);
        let mut any = false;
        self.near(lo, hi, |p, _| {
            if !any && c.closest(at, p).dist < c.radius {
                any = true;
            }
        });
        any
    }

    fn ray(&mut self, from: Vec3, to: Vec3, mask: i64) -> Option<Contact> {
        let (a, b) = (v3(from), v3(to));
        let h = self.w.mirror.ray(a, b, mask as u32)?;
        Some(Contact {
            normal: vec(h.normal),
            point: vec(Mirror::at(a, b, h.fraction)),
            collider_id: match self.w.pieces().get(h.piece) {
                Some(p) => collider_id(p),
                None => self.w.mirror.dynamic_collider(h.piece - self.w.pieces().len()),
            },
        })
    }
}

/// The engine's `cast_motion` search over one shape whose first contact is at `t` (0 < t <= 1):
/// `(safe, unsafe)`.
fn cast_search(t: f64) -> (f64, f64) {
    let (mut low, mut hi, mut coeff) = (0.0_f64, 1.0_f64, 0.5_f64);
    for j in 0..CAST_STEPS {
        let f = low + (hi - low) * coeff;
        if f >= t {
            hi = f;
            coeff = if j == 0 || low > 0.0 { 0.5 } else { 0.25 };
        } else {
            low = f;
            coeff = 0.5;
        }
    }
    (low, hi)
}

/// An upright capsule: a core segment `half` up and down from the centre, swept by `radius`.
#[derive(Clone, Copy, Debug)]
struct Capsule {
    half: f64,
    radius: f64,
}

/// The closest points between a capsule's core and a piece: `dist` 0 when they meet.
#[derive(Clone, Copy, Debug)]
struct Closest {
    dist: f64,
    on_core: V3,
    on_piece: V3,
}

impl Capsule {
    fn core(&self, at: V3) -> [V3; 2] {
        [
            [at[0], at[1] - self.half, at[2]],
            [at[0], at[1] + self.half, at[2]],
        ]
    }

    fn bounds(&self, at: V3) -> (V3, V3) {
        let e = [self.radius, self.half + self.radius, self.radius];
        (sub(at, e), add(at, e))
    }

    /// How far the capsule reaches along unit `n` from its centre.
    fn support(&self, n: V3) -> f64 {
        self.half * n[1].abs() + self.radius
    }

    fn closest(&self, at: V3, p: &Piece) -> Closest {
        gjk(&self.core(at), &p.verts)
    }

    /// The contact of the capsule at `at` with `p`: the normal out of the piece, the point on the
    /// piece, and the depth; `None` when apart.
    fn contact(&self, at: V3, p: &Piece) -> Option<(V3, V3, f64)> {
        let c = self.closest(at, p);
        if c.dist >= self.radius {
            return None;
        }
        if c.dist > 1e-9 {
            let n = scale(sub(c.on_core, c.on_piece), 1.0 / c.dist);
            return Some((n, c.on_piece, self.radius - c.dist));
        }
        // The core itself is in the piece: out along the face that needs the least travel.
        let mut best: Option<(V3, f64)> = None;
        for pl in &p.planes {
            let (mut n, mut d) = ([pl[0], pl[1], pl[2]], pl[3]);
            if !p.solid && dot(n, at) < d {
                n = scale(n, -1.0);
                d = -d;
            }
            let depth = d - (dot(n, at) - self.support(n));
            if best.is_none_or(|b| depth < b.1) {
                best = Some((n, depth));
            }
        }
        let (n, depth) = best?;
        let deepest = sub(at, scale(n, self.support(n)));
        Some((n, add(deepest, scale(n, depth)), depth.max(0.0)))
    }

    /// The fraction of `motion` at which the capsule from `from` (apart from `p`) first touches
    /// it, or `None` when it never does within the motion. The distance between two convex sets
    /// moving apart linearly is convex in time, so Newton steps from the start never pass the
    /// root.
    fn first_contact(&self, from: V3, motion: V3, p: &Piece) -> Option<f64> {
        let mut t = 0.0_f64;
        for _ in 0..GJK_ITERS {
            let c = self.closest(add(from, scale(motion, t)), p);
            let gap = c.dist - self.radius;
            if gap <= TOUCH || c.dist <= 1e-12 {
                return Some(t);
            }
            let n = scale(sub(c.on_core, c.on_piece), 1.0 / c.dist);
            let rate = dot(n, motion);
            if rate >= -1e-12 {
                return None;
            }
            t += gap / -rate;
            if t > 1.0 {
                return None;
            }
        }
        Some(t)
    }
}

/// A simplex vertex of the Minkowski difference `core - piece`, with the points it came from.
#[derive(Clone, Copy, Debug, Default)]
struct Sv {
    w: V3,
    a: V3,
    b: V3,
}

fn support(points: &[V3], d: V3) -> V3 {
    let mut best = points[0];
    let mut bd = dot(best, d);
    for &p in &points[1..] {
        let pd = dot(p, d);
        if pd > bd {
            best = p;
            bd = pd;
        }
    }
    best
}

/// GJK distance between the convex hulls of `a` and `b`.
fn gjk(a: &[V3], b: &[V3]) -> Closest {
    let mut s = [Sv::default(); 4];
    s[0] = Sv {
        w: sub(a[0], b[0]),
        a: a[0],
        b: b[0],
    };
    let mut n = 1;
    let mut v = s[0].w;
    let (mut ca, mut cb) = (a[0], b[0]);
    for _ in 0..GJK_ITERS {
        let vv = dot(v, v);
        if vv <= 1e-24 {
            return Closest {
                dist: 0.0,
                on_core: ca,
                on_piece: cb,
            };
        }
        let pa = support(a, scale(v, -1.0));
        let pb = support(b, v);
        let w = sub(pa, pb);
        if vv - dot(v, w) <= GJK_EPS * vv || s[..n].iter().any(|x| x.w == w) {
            break;
        }
        s[n] = Sv { w, a: pa, b: pb };
        n += 1;
        let (keep, lambda, m) = reduce(&s[..n]);
        if m == 4 {
            return Closest {
                dist: 0.0,
                on_core: ca,
                on_piece: cb,
            };
        }
        let mut next = [Sv::default(); 4];
        let (mut nv, mut na, mut nb) = ([0.0; 3], [0.0; 3], [0.0; 3]);
        for k in 0..m {
            let x = s[keep[k]];
            next[k] = x;
            nv = add(nv, scale(x.w, lambda[k]));
            na = add(na, scale(x.a, lambda[k]));
            nb = add(nb, scale(x.b, lambda[k]));
        }
        if dot(nv, nv) >= vv {
            break;
        }
        s = next;
        n = m;
        v = nv;
        ca = na;
        cb = nb;
    }
    Closest {
        dist: dot(v, v).sqrt(),
        on_core: ca,
        on_piece: cb,
    }
}

/// Which simplex vertices span a nearest point, their weights, and how many.
type Spanned = ([usize; 4], [f64; 4], usize);

/// The point of simplex `s` nearest the origin: the vertices that span it and their weights. All
/// four kept means the origin is inside the tetrahedron.
fn reduce(s: &[Sv]) -> Spanned {
    match s.len() {
        1 => ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0], 1),
        2 => segment(s[0].w, s[1].w, [0, 1]),
        3 => triangle(s[0].w, s[1].w, s[2].w, [0, 1, 2]),
        _ => tetrahedron(s),
    }
}

fn segment(a: V3, b: V3, ids: [usize; 2]) -> Spanned {
    let ab = sub(b, a);
    let len = dot(ab, ab);
    let t = if len > 0.0 { -dot(a, ab) / len } else { 0.0 };
    if t <= 0.0 {
        ([ids[0], 0, 0, 0], [1.0, 0.0, 0.0, 0.0], 1)
    } else if t >= 1.0 {
        ([ids[1], 0, 0, 0], [1.0, 0.0, 0.0, 0.0], 1)
    } else {
        ([ids[0], ids[1], 0, 0], [1.0 - t, t, 0.0, 0.0], 2)
    }
}

/// Ericson's closest point on a triangle to the origin, by Voronoi region.
fn triangle(a: V3, b: V3, c: V3, ids: [usize; 3]) -> Spanned {
    let one = |i: usize| ([ids[i], 0, 0, 0], [1.0, 0.0, 0.0, 0.0], 1);
    let two = |i: usize, j: usize, t: f64| ([ids[i], ids[j], 0, 0], [1.0 - t, t, 0.0, 0.0], 2);
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = scale(a, -1.0);
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return one(0);
    }
    let bp = scale(b, -1.0);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return one(1);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return two(0, 1, d1 / (d1 - d3));
    }
    let cp = scale(c, -1.0);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return one(2);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return two(0, 2, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return two(1, 2, (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let sum = va + vb + vc;
    if sum.abs() < 1e-300 {
        return one(0);
    }
    let (v, w) = (vb / sum, vc / sum);
    ([ids[0], ids[1], ids[2], 0], [1.0 - v - w, v, w, 0.0], 3)
}

fn tetrahedron(s: &[Sv]) -> Spanned {
    let faces = [[0, 1, 2, 3], [0, 2, 3, 1], [0, 3, 1, 2], [1, 3, 2, 0]];
    let mut best: Option<(f64, Spanned)> = None;
    for f in faces {
        let (a, b, c, d) = (s[f[0]].w, s[f[1]].w, s[f[2]].w, s[f[3]].w);
        let n = gs_sim_trace::piece::cross(sub(b, a), sub(c, a));
        let side_o = -dot(a, n);
        let side_d = dot(sub(d, a), n);
        let flat = side_d.abs() <= 1e-18;
        if !flat && side_o * side_d >= 0.0 {
            continue;
        }
        let r = triangle(a, b, c, [f[0], f[1], f[2]]);
        let mut p = [0.0; 3];
        for k in 0..r.2 {
            p = add(p, scale(s[r.0[k]].w, r.1[k]));
        }
        let dd = dot(p, p);
        if best.is_none_or(|x| dd < x.0) {
            best = Some((dd, r));
        }
    }
    match best {
        Some((_, r)) => r,
        None => ([0, 1, 2, 3], [0.25; 4], 4),
    }
}

/// A static bounding-volume tree over the mirror's piece bounds.
#[derive(Debug, Default)]
struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: V3,
    hi: V3,
    /// Leaf: `order[start..start + count]`; inner (`count` 0): children at `start`, `start + 1`.
    start: usize,
    count: usize,
}

const LEAF: usize = 4;

impl Bvh {
    fn build(pieces: &[Piece]) -> Bvh {
        let mut b = Bvh {
            nodes: Vec::new(),
            order: (0..pieces.len()).collect(),
        };
        if !pieces.is_empty() {
            b.nodes.push(Node {
                lo: [0.0; 3],
                hi: [0.0; 3],
                start: 0,
                count: 0,
            });
            b.split(pieces, 0, 0, pieces.len());
        }
        b
    }

    fn split(&mut self, pieces: &[Piece], node: usize, start: usize, end: usize) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for &i in &self.order[start..end] {
            for k in 0..3 {
                lo[k] = lo[k].min(pieces[i].lo[k]);
                hi[k] = hi[k].max(pieces[i].hi[k]);
            }
        }
        self.nodes[node].lo = lo;
        self.nodes[node].hi = hi;
        if end - start <= LEAF {
            self.nodes[node].start = start;
            self.nodes[node].count = end - start;
            return;
        }
        let axis = (0..3)
            .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
            .unwrap_or(0);
        let mid = start + (end - start) / 2;
        let key = |i: &usize| pieces[*i].lo[axis] + pieces[*i].hi[axis];
        self.order[start..end]
            .select_nth_unstable_by(mid - start, |a, b| key(a).total_cmp(&key(b)));
        let left = self.nodes.len();
        let blank = Node {
            lo,
            hi,
            start: 0,
            count: 0,
        };
        self.nodes.push(blank);
        self.nodes.push(blank);
        self.nodes[node].start = left;
        self.nodes[node].count = 0;
        self.split(pieces, left, start, mid);
        self.split(pieces, left + 1, mid, end);
    }

    fn query(&self, lo: V3, hi: V3, mut f: impl FnMut(usize)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            if (0..3).any(|k| n.lo[k] > hi[k] || n.hi[k] < lo[k]) {
                continue;
            }
            if n.count > 0 {
                for &p in &self.order[n.start..n.start + n.count] {
                    f(p);
                }
            } else {
                stack.push(n.start);
                stack.push(n.start + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gs_sim_trace::piece::Xform;

    fn cuboid(at: V3, half: V3) -> Piece {
        let x = Xform {
            origin: at,
            ..Xform::IDENTITY
        };
        Piece::cuboid(&x, half, 1).unwrap()
    }

    fn world(pieces: Vec<Piece>) -> MirrorWorld {
        let mut pieces = pieces;
        for (i, p) in pieces.iter_mut().enumerate() {
            p.shape = i as u32;
        }
        MirrorWorld::new(Arc::new(Mirror::new(pieces)))
    }

    const M: Masks = Masks {
        collision: 1,
        pass_out: 0,
    };

    fn floor() -> MirrorWorld {
        world(vec![cuboid([0.0, -0.5, 0.0], [20.0, 0.5, 20.0])])
    }

    #[test]
    fn gjk_point_box() {
        let b = cuboid([0.0; 3], [1.0; 3]);
        let c = gjk(&[[3.0, 0.0, 0.0]], &b.verts);
        assert!((c.dist - 2.0).abs() < 1e-9, "{c:?}");
        let c = gjk(&[[3.0, 3.0, 0.0], [3.0, -3.0, 0.0]], &b.verts);
        assert!((c.dist - 2.0).abs() < 1e-9, "{c:?}");
        let c = gjk(&[[2.0, 2.0, 2.0]], &b.verts);
        assert!((c.dist - 3f64.sqrt()).abs() < 1e-9, "{c:?}");
        let c = gjk(&[[0.5, 3.0, 0.0], [0.5, -3.0, 0.0]], &b.verts);
        assert_eq!(c.dist, 0.0);
    }

    #[test]
    fn falls_onto_floor() {
        let w = floor();
        let h = w.sweep(
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(0.0, -2.0, 0.0),
            1.8,
            0.4,
            M,
        );
        assert!(h.hit);
        // The capsule's bottom (0.9 below the centre) reaches y = 0 at 1.1 of 2 metres.
        assert!((h.fraction - 0.55).abs() < 2.0 / 256.0, "{h:?}");
        assert!((h.normal.y - 1.0).abs() < 1e-5, "{h:?}");
        assert!(h.point.y.abs() < 1e-3, "{h:?}");
    }

    #[test]
    fn misses_above_and_rests_on_floor() {
        let w = floor();
        let up = w.sweep(
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            1.8,
            0.4,
            M,
        );
        assert!(!up.hit);
        let r = w.rest_contact(Vec3::new(0.0, 0.89, 0.0), 1.8, 0.4, M);
        assert!(r.hit && (r.depth - 0.01).abs() < 1e-4, "{r:?}");
        assert!((r.normal.y - 1.0).abs() < 1e-5);
        assert!(!w.rest_contact(Vec3::new(0.0, 0.95, 0.0), 1.8, 0.4, M).hit);
        assert!(w.overlaps(Vec3::new(0.0, 0.89, 0.0), 1.8, 0.4, M));
        assert!(!w.overlaps(Vec3::new(0.0, 0.91, 0.0), 1.8, 0.4, M));
    }

    #[test]
    fn wall_stops_and_mask_passes() {
        let w = world(vec![cuboid([2.0, 1.0, 0.0], [0.5, 1.0, 2.0])]);
        let h = w.sweep(
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            1.8,
            0.4,
            M,
        );
        // The face at x = 1.5 is met when the centre reaches 1.1.
        assert!(h.hit && (h.fraction - 0.55).abs() < 2.0 / 256.0, "{h:?}");
        assert!((h.normal.x + 1.0).abs() < 1e-5, "{h:?}");
        let other = Masks {
            collision: 2,
            pass_out: 0,
        };
        assert!(
            !w.sweep(
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
                1.8,
                0.4,
                other
            )
            .hit
        );
    }

    #[test]
    fn edge_contact_normal_points_at_capsule() {
        let w = world(vec![cuboid([0.0; 3], [1.0; 3])]);
        // Resting diagonally over the box's top +x edge.
        let r = w.rest_contact(Vec3::new(1.2, 1.0 + 0.5 + 0.25, 0.0), 1.8, 0.4, M);
        assert!(r.hit, "{r:?}");
        assert!(r.normal.x > 0.1 && r.normal.y > 0.1, "{r:?}");
    }

    #[test]
    fn started_inside_is_ignored() {
        let w = world(vec![cuboid([0.0; 3], [1.0; 3])]);
        let h = w.sweep(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            1.8,
            0.4,
            M,
        );
        assert!(!h.hit || h.fraction > 0.0, "{h:?}");
        let r = w.rest_contact(Vec3::new(0.0, 0.0, 0.0), 1.8, 0.4, M);
        assert!(r.hit && r.depth > 1.0, "{r:?}");
    }

    #[test]
    fn triangle_floor() {
        let t =
            Piece::triangle([-10.0, 0.0, -10.0], [10.0, 0.0, -10.0], [0.0, 0.0, 10.0], 1).unwrap();
        let w = world(vec![t]);
        let h = w.sweep(
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(0.0, -2.0, 0.0),
            1.8,
            0.4,
            M,
        );
        assert!(h.hit && (h.fraction - 0.55).abs() < 2.0 / 256.0, "{h:?}");
        assert!(h.normal.y.abs() > 0.999, "{h:?}");
    }

    #[test]
    fn slope_contact_normal() {
        // A 30 degree ramp as a triangle.
        let s = 0.5_f64;
        let c = 3f64.sqrt() / 2.0;
        let t = Piece::triangle(
            [-10.0, -10.0 * s / c, -10.0],
            [10.0, 10.0 * s / c, -10.0],
            [-10.0, -10.0 * s / c, 10.0],
            1,
        )
        .unwrap();
        let w = world(vec![t]);
        let h = w.sweep(
            Vec3::new(-3.0, 3.0, -3.0),
            Vec3::new(0.0, -6.0, 0.0),
            1.8,
            0.4,
            M,
        );
        assert!(h.hit, "{h:?}");
        assert!((h.normal.y.abs() - c as f32).abs() < 1e-3, "{h:?}");
    }

    #[test]
    fn cast_search_matches_engine_steps() {
        // Contact at 0.3: the engine's first step lands at 0.5 (hit), then 0.125 (miss) ...
        let (low, hi) = cast_search(0.3);
        assert!(low < 0.3 && hi >= 0.3 && hi - low < 0.01, "{low} {hi}");
        assert_eq!(cast_search(1.0).1, 1.0);
    }

    #[test]
    fn ray_hits_floor() {
        let w = floor();
        let c = w
            .ray(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -5.0, 0.0), 1)
            .unwrap();
        assert!(c.point.y.abs() < 1e-4 && c.normal.y > 0.999, "{c:?}");
    }

    /// One mover's transform batch entry at `origin`, unrotated.
    fn at(origin: [f32; 3]) -> [f32; 12] {
        let mut f = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        f[9..].copy_from_slice(&origin);
        f
    }

    const LIFT: i64 = 4_000_000_001;
    const DOOR: i64 = 4_000_000_002;

    /// The floor plus a lift (a 2 m square slab 0.2 thick) and a door (a slab across x), both in
    /// body space, posed through the batch.
    fn with_movers() -> MirrorWorld {
        let w = floor();
        w.mirror.set_movers(vec![
            (LIFT, vec![cuboid([0.0; 3], [1.0, 0.1, 1.0])]),
            (DOOR, vec![cuboid([0.0; 3], [0.1, 1.5, 2.0])]),
        ]);
        w
    }

    #[test]
    fn lift_raised_under_a_capsule_carries_it() {
        let w = with_movers();
        let mut batch = [0.0f32; 24];
        batch[..12].copy_from_slice(&at([5.0, 0.5, 0.0]));
        batch[12..].copy_from_slice(&at([-8.0, 1.5, 0.0]));
        assert_eq!(w.mirror.pose_movers(&batch), 2);
        // Resting on the lift's top (y 0.6): the contact names the lift, so the motor rides it.
        let r = w.rest_contact(Vec3::new(5.0, 0.6 + 0.89, 0.0), 1.8, 0.4, M);
        assert!(r.hit && (r.normal.y - 1.0).abs() < 1e-5, "{r:?}");
        assert_eq!(r.collider_id, LIFT);
        // The lift rises 0.3: the capsule that rested on it is now inside it, and a fall onto it
        // lands 0.3 higher.
        batch[..12].copy_from_slice(&at([5.0, 0.8, 0.0]));
        assert_eq!(w.mirror.pose_movers(&batch), 2);
        let r = w.rest_contact(Vec3::new(5.0, 0.6 + 0.89, 0.0), 1.8, 0.4, M);
        assert!(r.hit && r.depth > 0.29 && r.collider_id == LIFT, "{r:?}");
        let h = w.sweep(
            Vec3::new(5.0, 3.0, 0.0),
            Vec3::new(0.0, -3.0, 0.0),
            1.8,
            0.4,
            M,
        );
        assert!(h.hit && h.collider_id == LIFT, "{h:?}");
        assert!((h.point.y - 0.9).abs() < 1e-2, "{h:?}");
        // Beside the lift there is only the floor.
        let r = w.rest_contact(Vec3::new(0.0, 0.89, 0.0), 1.8, 0.4, M);
        assert!(r.hit && r.collider_id == 1, "{r:?}");
    }

    #[test]
    fn door_posed_in_the_path_blocks_and_swung_away_clears() {
        let w = with_movers();
        let mut batch = [0.0f32; 24];
        batch[..12].copy_from_slice(&at([-8.0, 0.5, 0.0]));
        batch[12..].copy_from_slice(&at([2.0, 1.5, 0.0]));
        w.mirror.pose_movers(&batch);
        let walk = |w: &MirrorWorld| {
            w.sweep(
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(4.0, 0.0, 0.0),
                1.8,
                0.4,
                M,
            )
        };
        let h = walk(&w);
        assert!(h.hit && h.collider_id == DOOR, "{h:?}");
        // The capsule's side (0.4 out) meets the door's face at x 1.9 after 1.5 of 4 metres.
        assert!((h.fraction - 1.5 / 4.0).abs() < 2.0 / 256.0, "{h:?}");
        assert!((h.normal.x + 1.0).abs() < 1e-4, "{h:?}");
        let c = w
            .ray(Vec3::new(0.0, 1.0, 0.0), Vec3::new(4.0, 1.0, 0.0), 1)
            .unwrap();
        assert_eq!(c.collider_id, DOOR);
        assert!(w.overlaps(Vec3::new(2.0, 1.0, 0.0), 1.8, 0.4, M));
        // Swung open (slid 5 m aside): the corridor is clear again.
        batch[12..].copy_from_slice(&at([2.0, 1.5, 5.0]));
        w.mirror.pose_movers(&batch);
        assert!(!walk(&w).hit);
        assert!(
            w.ray(Vec3::new(0.0, 1.0, 0.0), Vec3::new(4.0, 1.0, 0.0), 1)
                .is_none()
        );
        // Off the motor's mask a mover is not there.
        batch[12..].copy_from_slice(&at([2.0, 1.5, 0.0]));
        w.mirror.pose_movers(&batch);
        let off = Masks {
            collision: 2,
            pass_out: 0,
        };
        assert!(
            !w.sweep(
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(4.0, 0.0, 0.0),
                1.8,
                0.4,
                off
            )
            .hit
        );
    }
}
