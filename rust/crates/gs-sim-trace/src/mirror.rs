//! A map's static collision as a bounding-volume hierarchy over convex pieces: ray, box sweep and
//! point contents without a physics server. Built once at map load from the scene's shapes (or
//! read back from a baked blob, [`Mirror::to_bytes`]), then read-only and shared across threads.
//! Moving bodies (doors, platforms) are a short dynamic list of movers: each mover's pieces are
//! held in body space, re-posed in place from one batch of transforms per tick
//! ([`Mirror::pose_movers`]) and tested behind the mover's own bounds, so a query pays only for
//! the movers it passes near. Answers match Godot Physics within float noise, not to the bit: hit or miss,
//! fraction within 1e-3 and normal within a degree is the gate.

use crate::piece::{Piece, V3, Xform, add, scale, sub};
use std::sync::RwLock;

const LEAF: usize = 4;

/// The first thing a query met.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// How far along the motion, 0 to 1.
    pub fraction: f64,
    /// The surface normal, facing the ray or box.
    pub normal: V3,
    /// Which piece: static index, or `statics + i` for dynamic piece `i`.
    pub piece: usize,
    /// The box started overlapping (box sweeps only).
    pub start_solid: bool,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: V3,
    hi: V3,
    /// Leaf: first entry in `order`; inner: index of the right child (the left is the next node).
    first: usize,
    /// Pieces in a leaf; 0 for an inner node.
    count: usize,
}

/// One mover's run of dynamic pieces and their posed bounds.
#[derive(Clone, Copy, Debug)]
struct Group {
    /// The caller's id for the mover (the engine's body instance id), 0 for anonymous pieces.
    id: i64,
    first: usize,
    count: usize,
    lo: V3,
    hi: V3,
}

/// Floats per mover transform in [`Mirror::pose_movers`]: basis columns x, y, z, then origin.
pub const XFORM_FLOATS: usize = 12;

#[derive(Debug, Default)]
struct Dynamic {
    /// World-space pieces; hit index `statics + k` for piece `k`.
    posed: Vec<Piece>,
    /// The movers' pieces in body space, parallel to `posed` (empty after [`Mirror::set_dynamic`]).
    local: Vec<Piece>,
    groups: Vec<Group>,
}

fn bounds_of(pieces: &[Piece]) -> (V3, V3) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in pieces {
        for a in 0..3 {
            lo[a] = lo[a].min(p.lo[a]);
            hi[a] = hi[a].max(p.hi[a]);
        }
    }
    (lo, hi)
}

/// A map's collision mirror.
#[derive(Debug, Default)]
pub struct Mirror {
    pieces: Vec<Piece>,
    nodes: Vec<Node>,
    order: Vec<usize>,
    dynamic: RwLock<Dynamic>,
}

/// The parameter of the segment `from + t * delta` where it enters the box, if it does within
/// `[0, limit]`.
fn seg_box(from: V3, inv: V3, lo: V3, hi: V3, limit: f64) -> Option<f64> {
    let (mut t0, mut t1) = (0.0_f64, limit);
    for a in 0..3 {
        if inv[a].is_infinite() {
            if from[a] < lo[a] || from[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (mut n, mut f) = ((lo[a] - from[a]) * inv[a], (hi[a] - from[a]) * inv[a]);
        if n > f {
            std::mem::swap(&mut n, &mut f);
        }
        t0 = t0.max(n);
        t1 = t1.min(f);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

fn inverse(delta: V3) -> V3 {
    delta.map(|d| if d == 0.0 { f64::INFINITY } else { 1.0 / d })
}

impl Mirror {
    /// A mirror over `pieces` (any order), with no dynamic entries.
    pub fn new(pieces: Vec<Piece>) -> Mirror {
        let mut m = Mirror {
            order: (0..pieces.len()).collect(),
            pieces,
            nodes: Vec::new(),
            dynamic: RwLock::new(Dynamic::default()),
        };
        if !m.pieces.is_empty() {
            let n = m.pieces.len();
            m.build(0, n);
        }
        m
    }

    fn build(&mut self, first: usize, count: usize) -> usize {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for &i in &self.order[first..first + count] {
            for a in 0..3 {
                lo[a] = lo[a].min(self.pieces[i].lo[a]);
                hi[a] = hi[a].max(self.pieces[i].hi[a]);
            }
        }
        let at = self.nodes.len();
        self.nodes.push(Node {
            lo,
            hi,
            first,
            count,
        });
        if count <= LEAF {
            return at;
        }
        let ext = sub(hi, lo);
        let axis = (0..3).fold(0, |b, a| if ext[a] > ext[b] { a } else { b });
        let pieces = &self.pieces;
        let centre = |i: usize| pieces[i].lo[axis] + pieces[i].hi[axis];
        let half = count / 2;
        self.order[first..first + count]
            .select_nth_unstable_by(half, |a, b| centre(*a).total_cmp(&centre(*b)));
        self.build(first, half);
        let right = self.build(first + half, count - half);
        self.nodes[at].first = right;
        self.nodes[at].count = 0;
        at
    }

    /// How many static pieces.
    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// The static pieces, in build order.
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    fn read_dynamic(&self) -> std::sync::RwLockReadGuard<'_, Dynamic> {
        self.dynamic.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_dynamic(&self) -> std::sync::RwLockWriteGuard<'_, Dynamic> {
        self.dynamic.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Replaces the dynamic pieces with `pieces`, already posed in world space, as one anonymous
    /// group (collider id 0) that [`Mirror::pose_movers`] does not move. Drops any movers.
    pub fn set_dynamic(&self, pieces: Vec<Piece>) {
        let mut d = self.write_dynamic();
        let (lo, hi) = bounds_of(&pieces);
        d.groups.clear();
        if !pieces.is_empty() {
            d.groups.push(Group {
                id: 0,
                first: 0,
                count: pieces.len(),
                lo,
                hi,
            });
        }
        d.posed = pieces;
        d.local.clear();
    }

    /// Replaces the dynamic pieces with `movers`: each an id (the engine's body instance id, what
    /// [`Mirror::dynamic_collider`] answers) and its pieces in body space, posed at the identity
    /// until the first [`Mirror::pose_movers`].
    pub fn set_movers(&self, movers: Vec<(i64, Vec<Piece>)>) {
        let mut d = self.write_dynamic();
        d.groups.clear();
        d.local.clear();
        for (id, pieces) in movers {
            let (lo, hi) = bounds_of(&pieces);
            let first = d.local.len();
            d.groups.push(Group {
                id,
                first,
                count: pieces.len(),
                lo,
                hi,
            });
            d.local.extend(pieces);
        }
        d.posed = d.local.clone();
    }

    /// Re-poses the movers from one batch of transforms, [`XFORM_FLOATS`] per mover in
    /// [`Mirror::set_movers`] order (basis columns x, y, z, then origin, as Godot's `Transform3D`
    /// reads). In place: no allocation once each posed piece has its size. A short batch poses
    /// the first movers only. How many movers were posed.
    pub fn pose_movers(&self, xforms: &[f32]) -> usize {
        let mut d = self.write_dynamic();
        if d.local.len() != d.posed.len() {
            return 0;
        }
        let n = d.groups.len().min(xforms.len() / XFORM_FLOATS);
        let Dynamic {
            posed,
            local,
            groups,
        } = &mut *d;
        for (g, f) in groups.iter_mut().zip(xforms.as_chunks::<XFORM_FLOATS>().0).take(n) {
            let v = |i: usize| [f64::from(f[i]), f64::from(f[i + 1]), f64::from(f[i + 2])];
            let x = Xform {
                cols: [v(0), v(3), v(6)],
                origin: v(9),
            };
            let run = g.first..g.first + g.count;
            for (src, out) in local[run.clone()].iter().zip(posed[run.clone()].iter_mut()) {
                src.pose_into(&x, out);
            }
            (g.lo, g.hi) = bounds_of(&posed[run]);
        }
        n
    }

    /// The movers' ids, in [`Mirror::set_movers`] order (anonymous pieces are not a mover).
    pub fn mover_ids(&self) -> Vec<i64> {
        let d = self.read_dynamic();
        if d.local.is_empty() {
            return Vec::new();
        }
        d.groups.iter().map(|g| g.id).collect()
    }

    /// How many dynamic pieces.
    pub fn dynamic_len(&self) -> usize {
        self.read_dynamic().posed.len()
    }

    /// The id of the mover dynamic piece `k` belongs to (a hit's `piece - statics`), 0 for an
    /// anonymous piece or none.
    pub fn dynamic_collider(&self, k: usize) -> i64 {
        let d = self.read_dynamic();
        d.groups
            .iter()
            .find(|g| k >= g.first && k < g.first + g.count)
            .map_or(0, |g| g.id)
    }

    /// Every posed dynamic piece on `mask` whose box meets `lo`..`hi`, with its mover's id.
    pub fn dynamic_near(&self, lo: V3, hi: V3, mask: u32, mut f: impl FnMut(&Piece, i64)) {
        let meets = |a: V3, b: V3| (0..3).all(|i| a[i] <= hi[i] && b[i] >= lo[i]);
        let d = self.read_dynamic();
        for g in &d.groups {
            if !meets(g.lo, g.hi) {
                continue;
            }
            for p in &d.posed[g.first..g.first + g.count] {
                if p.layer & mask != 0 && meets(p.lo, p.hi) {
                    f(p, g.id);
                }
            }
        }
    }

    /// Walks the leaves whose box (grown by `grow`) the segment enters before the best hit so far;
    /// `test` answers a piece and returns its fraction when it is the new best.
    fn walk(
        &self,
        from: V3,
        delta: V3,
        grow: V3,
        mask: u32,
        mut test: impl FnMut(usize, &Piece, f64) -> Option<f64>,
    ) {
        let inv = inverse(delta);
        let mut best = 1.0_f64;
        let mut stack = [0usize; 64];
        let mut top = usize::from(!self.nodes.is_empty());
        while top > 0 {
            top -= 1;
            let idx = stack[top];
            let node = self.nodes[idx];
            let (lo, hi) = (sub(node.lo, grow), add(node.hi, grow));
            if seg_box(from, inv, lo, hi, best).is_none() {
                continue;
            }
            if node.count > 0 {
                for &i in &self.order[node.first..node.first + node.count] {
                    let p = &self.pieces[i];
                    if p.layer & mask != 0
                        && let Some(t) = test(i, p, best)
                    {
                        best = t;
                    }
                }
            } else if top + 2 <= stack.len() {
                stack[top] = node.first;
                stack[top + 1] = idx + 1;
                top += 2;
            }
        }
        let d = self.read_dynamic();
        for g in &d.groups {
            if seg_box(from, inv, sub(g.lo, grow), add(g.hi, grow), best).is_none() {
                continue;
            }
            for k in g.first..g.first + g.count {
                let p = &d.posed[k];
                if p.layer & mask != 0
                    && seg_box(from, inv, sub(p.lo, grow), add(p.hi, grow), best).is_some()
                    && let Some(t) = test(self.pieces.len() + k, p, best)
                {
                    best = t;
                }
            }
        }
    }

    /// The ray from `from` to `to` against every piece on `mask`: the nearest hit, if any.
    pub fn ray(&self, from: V3, to: V3, mask: u32) -> Option<Hit> {
        let delta = sub(to, from);
        let mut out = None;
        self.walk(from, delta, [0.0; 3], mask, |i, p, best| {
            let (t, normal) = p.ray(from, delta)?;
            if t >= best && out.is_some() {
                return None;
            }
            out = Some(Hit {
                fraction: t,
                normal,
                piece: i,
                start_solid: false,
            });
            Some(t)
        });
        out
    }

    /// The engine shapes on `mask` the box of `half` extents at `at` overlaps.
    fn inside_shapes(&self, at: V3, half: V3, mask: u32) -> Vec<u32> {
        let mut shapes = Vec::new();
        self.walk(at, [0.0; 3], half, mask, |_, p, _| {
            if !shapes.contains(&p.shape) && p.sweep(at, [0.0; 3], half).is_some() {
                shapes.push(p.shape);
            }
            None
        });
        shapes
    }

    /// The axis-aligned box of `half` extents centred at `from` moved by `motion`: the first
    /// contact, if any. As Godot's `cast_motion`, every shape the box starts inside of is
    /// ignored, so `start_solid` stays false for a box the mirror's shapes all came from.
    pub fn sweep(&self, from: V3, motion: V3, half: V3, mask: u32) -> Option<Hit> {
        let inside = self.inside_shapes(from, half, mask);
        let mut out = None;
        self.walk(from, motion, half, mask, |i, p, best| {
            if inside.contains(&p.shape) {
                return None;
            }
            let (t, normal, start_solid) = p.sweep(from, motion, half)?;
            if t >= best && out.is_some() {
                return None;
            }
            out = Some(Hit {
                fraction: t,
                normal,
                piece: i,
                start_solid,
            });
            Some(t)
        });
        out
    }

    /// The layers of every solid on `mask` that holds `p` (Source's point contents), 0 for none.
    pub fn point_contents(&self, p: V3, mask: u32) -> u32 {
        let mut layers = 0;
        let inside = |lo: V3, hi: V3| (0..3).all(|a| p[a] >= lo[a] - 1e-6 && p[a] <= hi[a] + 1e-6);
        let mut stack = [0usize; 64];
        let mut top = usize::from(!self.nodes.is_empty());
        while top > 0 {
            top -= 1;
            let idx = stack[top];
            let node = self.nodes[idx];
            if !inside(node.lo, node.hi) {
                continue;
            }
            if node.count > 0 {
                for &i in &self.order[node.first..node.first + node.count] {
                    let q = &self.pieces[i];
                    if q.layer & mask != 0 && q.contains(p) {
                        layers |= q.layer & mask;
                    }
                }
            } else if top + 2 <= stack.len() {
                stack[top] = node.first;
                stack[top + 1] = idx + 1;
                top += 2;
            }
        }
        let d = self.read_dynamic();
        for g in &d.groups {
            if !inside(g.lo, g.hi) {
                continue;
            }
            for q in &d.posed[g.first..g.first + g.count] {
                if q.layer & mask != 0 && q.contains(p) {
                    layers |= q.layer & mask;
                }
            }
        }
        layers
    }

    /// The point a hit lies at along `from` to `to`.
    pub fn at(from: V3, to: V3, fraction: f64) -> V3 {
        add(from, scale(sub(to, from), fraction))
    }
}
