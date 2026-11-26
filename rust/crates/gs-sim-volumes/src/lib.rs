//! A map's static volume boxes (pushes, gravity, conveyors, pits, ladders, water) and, for a tick's
//! players at once, which of them stand clear of every box: a game skips its per-player volume
//! scripts for those. Bit for bit with the script reference (FC's `FcVolumePrefilter.clear_of_script`):
//! f32 math in the engine's order, `AABB.abs()`, then `position - below` and `end + above`, and
//! closed comparisons, so a point on a grown face counts as near.

#[cfg(test)]
mod tests;

/// One axis-aligned box as `AABB.abs()` leaves it: a non-negative size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub position: [f32; 3],
    pub size: [f32; 3],
}

impl Bounds {
    /// From an engine AABB's position and size; a negative size is flipped as `AABB.abs()` does:
    /// `position + size.min(0)`, `size.abs()`.
    pub fn from_position_size(position: [f32; 3], size: [f32; 3]) -> Self {
        let mut p = position;
        let mut s = size;
        for a in 0..3 {
            p[a] = position[a] + size[a].min(0.0);
            s[a] = size[a].abs();
        }
        Self {
            position: p,
            size: s,
        }
    }

    /// Whether `point` lies in this box grown by `below` under its low corner and `above` over its
    /// high one, faces included.
    pub fn near(&self, point: [f32; 3], below: [f32; 3], above: [f32; 3]) -> bool {
        (0..3).all(|a| {
            let lo = self.position[a] - below[a];
            let hi = (self.position[a] + self.size[a]) + above[a];
            point[a] >= lo && point[a] <= hi
        })
    }
}

/// The volume boxes of one map.
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

    /// Whether `point` is near any box (see [`Bounds::near`]).
    pub fn near(&self, point: [f32; 3], below: [f32; 3], above: [f32; 3]) -> bool {
        self.boxes.iter().any(|b| b.near(point, below, above))
    }

    /// One byte per point: 1 when it is finite and near no box, else 0.
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
