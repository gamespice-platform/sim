//! GameSpice Combat's flat world ray: which of many axis-aligned boxes a ray enters first.
//!
//! The script original is `DotTraceFlat._world_ray` and `_ray_aabb` (dot-combat). Box corners are
//! the engine's `f32`; the slab arithmetic is the script's `f64`, in its order, with Godot's
//! `maxf`/`minf` (`a > b ? a : b`, `a < b ? a : b`), so the chosen box is the script's to the bit.

/// Directions shorter than this on an axis are parallel to that axis's slabs (the script's literal).
pub const PARALLEL: f64 = 0.000001;

/// The slab test: the entry distance of the ray into the box `lo`..`hi`, or -1 for a miss or an
/// entry farther than `limit`. A ray starting inside the box returns 0.
#[inline]
pub fn ray_box(from: [f32; 3], dir: [f32; 3], lo: [f32; 3], hi: [f32; 3], limit: f64) -> f64 {
    let mut t_min = 0.0_f64;
    let mut t_max = limit;
    for axis in 0..3 {
        let d = f64::from(dir[axis]);
        let o = f64::from(from[axis]);
        if d.abs() < PARALLEL {
            if o < f64::from(lo[axis]) || o > f64::from(hi[axis]) {
                return -1.0;
            }
            continue;
        }
        let inverse = 1.0 / d;
        let mut t1 = (f64::from(lo[axis]) - o) * inverse;
        let mut t2 = (f64::from(hi[axis]) - o) * inverse;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = if t_min > t1 { t_min } else { t1 };
        t_max = if t_max < t2 { t_max } else { t2 };
        if t_min > t_max {
            return -1.0;
        }
    }
    t_min
}

/// The box the ray enters first within `limit`: its index and entry distance, or `None`.
/// `corners` holds each box as its low corner then its high corner (`position`, `position + size`).
/// A later box at the same distance wins, as in the script's loop.
pub fn nearest(
    corners: &[[f32; 3]],
    from: [f32; 3],
    dir: [f32; 3],
    limit: f64,
) -> Option<(usize, f64)> {
    nearest_by(
        corners.len() / 2,
        |i| (corners[2 * i], corners[2 * i + 1]),
        from,
        dir,
        limit,
    )
}

/// [`nearest`] over `count` boxes read through `corner(i) -> (low, high)`.
pub fn nearest_by(
    count: usize,
    corner: impl Fn(usize) -> ([f32; 3], [f32; 3]),
    from: [f32; 3],
    dir: [f32; 3],
    limit: f64,
) -> Option<(usize, f64)> {
    let mut nearest = limit;
    let mut best = None;
    for i in 0..count {
        let (lo, hi) = corner(i);
        let t = ray_box(from, dir, lo, hi, nearest);
        if t < 0.0 {
            continue;
        }
        nearest = t;
        best = Some((i, t));
    }
    best
}

/// What a ray found in a [`Bvh`] world: nothing, the ground plane, or a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Found {
    Miss,
    Floor(f64),
    Box(usize, f64),
}

/// A bounding-volume hierarchy over the boxes, so a ray visits the few leaves near its line instead
/// of every box. It answers exactly what [`nearest`] answers: the least entry distance, a later box
/// winning a tie. A node's slab interval contains its children's (rounding is monotonic in the
/// corners), so pruning by it never drops a box the flat scan would take.
pub struct Bvh {
    corners: Vec<[f32; 3]>,
    nodes: Vec<Node>,
    order: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    /// Internal: the left child (the right is `first + 1`). Leaf: the start in `order`.
    first: u32,
    /// 0 for an internal node, else the leaf's box count.
    count: u32,
}

const LEAF: usize = 4;

impl Bvh {
    /// Builds over `corners`, each box its low corner then its high corner.
    pub fn new(corners: &[[f32; 3]]) -> Self {
        let n = corners.len() / 2;
        let mut bvh = Bvh {
            corners: corners[..2 * n].to_vec(),
            nodes: Vec::with_capacity(n / 2 + 1),
            order: (0..n as u32).collect(),
        };
        if n > 0 {
            bvh.nodes.push(Node {
                lo: [0.0; 3],
                hi: [0.0; 3],
                first: 0,
                count: 0,
            });
            bvh.build(0, 0, n);
        }
        bvh
    }

    pub fn len(&self) -> usize {
        self.corners.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.corners.is_empty()
    }

    fn bounds(&self, start: usize, end: usize) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for &b in &self.order[start..end] {
            for a in 0..3 {
                lo[a] = lo[a].min(self.corners[2 * b as usize][a]);
                hi[a] = hi[a].max(self.corners[2 * b as usize + 1][a]);
            }
        }
        (lo, hi)
    }

    fn build(&mut self, node: usize, start: usize, end: usize) {
        let (lo, hi) = self.bounds(start, end);
        self.nodes[node].lo = lo;
        self.nodes[node].hi = hi;
        if end - start <= LEAF {
            self.nodes[node].first = start as u32;
            self.nodes[node].count = (end - start) as u32;
            return;
        }
        let axis = (0..3)
            .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
            .unwrap_or(0);
        let centre =
            |b: u32| self.corners[2 * b as usize][axis] + self.corners[2 * b as usize + 1][axis];
        let mut slice: Vec<u32> = self.order[start..end].to_vec();
        slice.sort_by(|&a, &b| centre(a).total_cmp(&centre(b)));
        self.order[start..end].copy_from_slice(&slice);
        let mid = (start + end) / 2;
        let left = self.nodes.len();
        self.nodes.push(Node {
            lo: [0.0; 3],
            hi: [0.0; 3],
            first: 0,
            count: 0,
        });
        self.nodes.push(Node {
            lo: [0.0; 3],
            hi: [0.0; 3],
            first: 0,
            count: 0,
        });
        self.nodes[node].first = left as u32;
        self.build(left, start, mid);
        self.build(left + 1, mid, end);
    }

    /// The box the ray enters first within `limit`, as [`nearest`] over the same boxes.
    pub fn nearest(&self, from: [f32; 3], dir: [f32; 3], limit: f64) -> Option<(usize, f64)> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best_t = limit;
        let mut best: Option<usize> = None;
        let mut stack = [0u32; 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let node = self.nodes[stack[top] as usize];
            if ray_box(from, dir, node.lo, node.hi, best_t) < 0.0 {
                continue;
            }
            if node.count > 0 {
                let start = node.first as usize;
                for &b in &self.order[start..start + node.count as usize] {
                    let i = b as usize;
                    let t = ray_box(
                        from,
                        dir,
                        self.corners[2 * i],
                        self.corners[2 * i + 1],
                        best_t,
                    );
                    if t < 0.0 {
                        continue;
                    }
                    if t < best_t || (t == best_t && best.is_none_or(|j| i > j)) {
                        best_t = t;
                        best = Some(i);
                    }
                }
                continue;
            }
            // Nearer child last, so it pops first.
            let l = node.first as usize;
            let tl = ray_box(from, dir, self.nodes[l].lo, self.nodes[l].hi, best_t);
            let tr = ray_box(
                from,
                dir,
                self.nodes[l + 1].lo,
                self.nodes[l + 1].hi,
                best_t,
            );
            let (first, second) = if tl >= 0.0 && (tr < 0.0 || tl <= tr) {
                (l, l + 1)
            } else {
                (l + 1, l)
            };
            stack[top] = second as u32;
            stack[top + 1] = first as u32;
            top += 2;
        }
        best.map(|i| (i, best_t))
    }

    /// The whole world ray of the script's `_world_ray`: the ground plane at `floor_y` (none when
    /// it is not above negative infinity) first, then the boxes within what the ground left.
    pub fn world_ray(&self, floor_y: f64, from: [f32; 3], dir: [f32; 3], limit: f64) -> Found {
        let mut found = Found::Miss;
        let mut nearest = limit;
        let dy = f64::from(dir[1]);
        if floor_y > f64::NEG_INFINITY && dy < -0.000001 {
            let t = (floor_y - f64::from(from[1])) / dy;
            if t >= 0.0 && t <= nearest {
                nearest = t;
                found = Found::Floor(t);
            }
        }
        match self.nearest(from, dir, nearest) {
            Some((i, t)) => Found::Box(i, t),
            None => found,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box(at: [f32; 3]) -> [[f32; 3]; 2] {
        [at, [at[0] + 1.0, at[1] + 1.0, at[2] + 1.0]]
    }

    #[test]
    fn hits_the_nearer_of_two_boxes() {
        let mut c = Vec::new();
        c.extend(unit_box([5.0, 0.0, 0.0]));
        c.extend(unit_box([2.0, 0.0, 0.0]));
        let hit = nearest(&c, [0.0, 0.5, 0.5], [1.0, 0.0, 0.0], 100.0);
        assert_eq!(hit, Some((1, 2.0)));
    }

    #[test]
    fn misses_beyond_the_limit_and_beside_the_box() {
        let c = unit_box([5.0, 0.0, 0.0]);
        assert_eq!(nearest(&c, [0.0, 0.5, 0.5], [1.0, 0.0, 0.0], 4.0), None);
        assert_eq!(nearest(&c, [0.0, 2.5, 0.5], [1.0, 0.0, 0.0], 100.0), None);
    }

    #[test]
    fn a_ray_starting_inside_returns_zero() {
        let c = unit_box([0.0, 0.0, 0.0]);
        assert_eq!(
            nearest(&c, [0.5, 0.5, 0.5], [0.0, 0.0, 1.0], 10.0),
            Some((0, 0.0))
        );
    }

    #[test]
    fn a_tie_goes_to_the_later_box() {
        let mut c = Vec::new();
        c.extend(unit_box([3.0, 0.0, 0.0]));
        c.extend(unit_box([3.0, 0.0, 0.0]));
        assert_eq!(
            nearest(&c, [0.0, 0.5, 0.5], [1.0, 0.0, 0.0], 10.0),
            Some((1, 3.0))
        );
    }

    #[test]
    fn slab_arithmetic_is_f64_over_f32_corners() {
        let lo = [0.1_f32, -1.0, -1.0];
        let hi = [0.3_f32, 1.0, 1.0];
        let dir = [0.7_f32, 0.0, 0.0];
        let t = ray_box([0.0; 3], dir, lo, hi, 10.0);
        assert_eq!(t, f64::from(0.1_f32) * (1.0 / f64::from(0.7_f32)));
    }

    fn rng(state: &mut u64) -> f32 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state >> 40) as f32 / (1u64 << 24) as f32
    }

    #[test]
    fn the_bvh_answers_as_the_flat_scan_over_many_worlds() {
        let mut s = 0x9E37_79B9_7F4A_7C15_u64;
        let mut hits = 0;
        let mut total = 0;
        for world in 0..60 {
            let n = [0, 1, 3, 5, 9, 40, 200, 700][world % 8];
            let mut c = Vec::new();
            for k in 0..n {
                let lo = [
                    rng(&mut s) * 40.0 - 20.0,
                    rng(&mut s) * 8.0 - 2.0,
                    rng(&mut s) * 40.0 - 20.0,
                ];
                let sz = [
                    rng(&mut s) * 4.0,
                    rng(&mut s) * 5.0,
                    if k % 9 == 0 { 0.0 } else { rng(&mut s) * 4.0 },
                ];
                c.push(lo);
                c.push([lo[0] + sz[0], lo[1] + sz[1], lo[2] + sz[2]]);
                if k % 11 == 0 {
                    // an identical box: the tie goes to the later one
                    c.push(lo);
                    c.push([lo[0] + sz[0], lo[1] + sz[1], lo[2] + sz[2]]);
                }
            }
            let bvh = Bvh::new(&c);
            for r in 0..300 {
                let from = [
                    rng(&mut s) * 50.0 - 25.0,
                    rng(&mut s) * 8.0 - 1.0,
                    rng(&mut s) * 50.0 - 25.0,
                ];
                let mut dir = [
                    rng(&mut s) * 2.0 - 1.0,
                    rng(&mut s) * 2.0 - 1.0,
                    rng(&mut s) * 2.0 - 1.0,
                ];
                match r % 5 {
                    0 => dir = [if r % 2 == 0 { -1.0 } else { 1.0 }, 0.0, 0.0],
                    1 => dir[1] = 0.0,
                    _ => {}
                }
                let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
                let dir = if len > 0.001 {
                    [dir[0] / len, dir[1] / len, dir[2] / len]
                } else {
                    [0.0, 0.0, -1.0]
                };
                let limit = f64::from(rng(&mut s) * 80.0 + 0.5);
                let a = nearest(&c, from, dir, limit);
                let b = bvh.nearest(from, dir, limit);
                assert_eq!(a, b, "world {world} ray {r}");
                total += 1;
                hits += usize::from(a.is_some());
            }
        }
        assert!(hits > total / 10, "{hits} of {total}");
    }

    #[test]
    fn the_world_ray_puts_a_box_over_the_ground_at_a_tie() {
        let c = unit_box([0.0, 0.0, 0.0]);
        let bvh = Bvh::new(&c);
        // Down onto the box top at y=1 with the ground also at y=1: the box takes it.
        let f = bvh.world_ray(1.0, [0.5, 3.0, 0.5], [0.0, -1.0, 0.0], 10.0);
        assert_eq!(f, Found::Box(0, 2.0));
        let g = bvh.world_ray(0.0, [5.0, 3.0, 5.0], [0.0, -1.0, 0.0], 10.0);
        assert_eq!(g, Found::Floor(3.0));
        assert_eq!(
            bvh.world_ray(f64::NEG_INFINITY, [5.0, 3.0, 5.0], [0.0, -1.0, 0.0], 10.0),
            Found::Miss
        );
    }
}
