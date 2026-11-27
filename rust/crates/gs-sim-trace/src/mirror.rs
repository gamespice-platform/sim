//! A map's static collision as a bounding-volume hierarchy over convex pieces: ray, box sweep and
//! point contents without a physics server. Built once at map load from the scene's shapes (or
//! read back from a baked blob, [`Mirror::to_bytes`]), then read-only and shared across threads.
//! Moving bodies (doors, platforms) are a short dynamic list, re-posed per tick and tested
//! linearly. Answers match Godot Physics within float noise, not to the bit: hit or miss,
//! fraction within 1e-3 and normal within a degree is the gate.

use crate::piece::{Piece, V3, add, scale, sub};
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

/// A map's collision mirror.
#[derive(Debug, Default)]
pub struct Mirror {
    pieces: Vec<Piece>,
    nodes: Vec<Node>,
    order: Vec<usize>,
    dynamic: RwLock<Vec<Piece>>,
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
            dynamic: RwLock::new(Vec::new()),
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

    /// Replaces the dynamic pieces (already posed in world space).
    pub fn set_dynamic(&self, pieces: Vec<Piece>) {
        *self.dynamic.write().unwrap_or_else(|e| e.into_inner()) = pieces;
    }

    /// How many dynamic pieces.
    pub fn dynamic_len(&self) -> usize {
        self.dynamic.read().unwrap_or_else(|e| e.into_inner()).len()
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
        let dynamic = self.dynamic.read().unwrap_or_else(|e| e.into_inner());
        for (k, p) in dynamic.iter().enumerate() {
            if p.layer & mask != 0
                && seg_box(from, inv, sub(p.lo, grow), add(p.hi, grow), best).is_some()
                && let Some(t) = test(self.pieces.len() + k, p, best)
            {
                best = t;
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
        let dynamic = self.dynamic.read().unwrap_or_else(|e| e.into_inner());
        for q in dynamic.iter() {
            if q.layer & mask != 0 && q.contains(p) {
                layers |= q.layer & mask;
            }
        }
        layers
    }

    /// The point a hit lies at along `from` to `to`.
    pub fn at(from: V3, to: V3, fraction: f64) -> V3 {
        add(from, scale(sub(to, from), fraction))
    }
}
