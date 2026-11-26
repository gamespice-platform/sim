//! A map's static volume boxes (pushes, gravity, conveyors, pits, ladders, water) and, for a tick's
//! players at once, which of them stand clear of every box: a game skips its per-player volume
//! scripts for those. The answer is a prefilter, never the volumes' own rules, so it only has to
//! be conservative: a box counts as touched when the query box overlaps or merely meets it, in
//! f64 from the script's f32 numbers, so float rounding can only ever say "near", never "clear".

#[cfg(test)]
mod tests;

/// One axis-aligned box as its two corners, `lo <= hi` on every axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

impl Bounds {
    /// From an engine AABB's position and size, as f32; a negative size is flipped (`AABB.abs()`).
    pub fn from_position_size(position: [f32; 3], size: [f32; 3]) -> Self {
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for a in 0..3 {
            let p = f64::from(position[a]);
            let e = p + f64::from(size[a]);
            lo[a] = p.min(e);
            hi[a] = p.max(e);
        }
        Self { lo, hi }
    }

    /// Whether the two boxes overlap or meet (closed on every face).
    pub fn touches(&self, other: &Bounds) -> bool {
        (0..3).all(|a| self.lo[a] <= other.hi[a] && other.lo[a] <= self.hi[a])
    }
}

/// The volumes of one map, set once when it loads.
#[derive(Clone, Debug, Default)]
pub struct VolumeSet {
    boxes: Vec<Bounds>,
}

impl VolumeSet {
    pub fn clear(&mut self) {
        self.boxes.clear();
    }

    pub fn add(&mut self, b: Bounds) {
        self.boxes.push(b);
    }

    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// Whether the query box `point + below .. point + above` touches any volume.
    pub fn near(&self, point: [f32; 3], below: [f32; 3], above: [f32; 3]) -> bool {
        let mut q = Bounds {
            lo: [0.0; 3],
            hi: [0.0; 3],
        };
        for a in 0..3 {
            let p = f64::from(point[a]);
            let x = p + f64::from(below[a]);
            let y = p + f64::from(above[a]);
            q.lo[a] = x.min(y);
            q.hi[a] = x.max(y);
        }
        self.boxes.iter().any(|b| b.touches(&q))
    }

    /// [`VolumeSet::near`] for every point: 1 where it stands clear of every volume, 0 where not
    /// (and 0 for a NaN coordinate, which compares false and so must not be trusted as clear).
    pub fn clear_of(&self, points: &[[f32; 3]], below: [f32; 3], above: [f32; 3]) -> Vec<u8> {
        points
            .iter()
            .map(|p| {
                let finite = p.iter().all(|c| c.is_finite());
                u8::from(finite && !self.near(*p, below, above))
            })
            .collect()
    }
}
