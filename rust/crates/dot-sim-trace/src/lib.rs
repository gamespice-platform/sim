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
}
