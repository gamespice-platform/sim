//! A map's PVS: dot-net's `DotNetVisibility` (`cluster_of`, `cluster_sees`, `visible`) over the
//! same arrays, to the bit.
//!
//! The Source engine asks the PVS before it traces a line of sight (`CCSBot::IsVisible`): a
//! cluster pair that no position can see between needs no ray. This is that ask without the
//! script's per-node array reads: the BSP walk (one plane test per node, in `f64` as the script
//! computes it) and one bit of the cluster's row.
//!
//! Conservative as the script: solid, outside the bounds, a broken tree or no data answers
//! visible, so a caller only ever skips a ray the map's world brushes would have blocked.

#[cfg(test)]
mod tests;

/// A map's visibility set, as `DotNetVisibility.native_dict()` hands it over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pvs {
    /// Per plane `nx, ny, nz, d` (metres, Godot axes): in front when `n.p - d >= 0`.
    pub planes: Vec<f32>,
    /// Per node: plane, front child, back child; a child below zero is leaf `-1 - child`.
    pub nodes: Vec<i32>,
    /// The root node.
    pub head: i32,
    /// Per leaf its cluster, `-1` for solid.
    pub leaf_cluster: Vec<i32>,
    /// Per cluster `row_bytes` bytes: bit `c` of byte `c / 8` set when it may see cluster `c`.
    pub rows: Vec<u8>,
    pub row_bytes: i64,
    pub clusters: i64,
    /// Outside `lo..=hi` every point answers visible; `None` for no bound (the script's empty `AABB`).
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

impl Pvs {
    /// `DotNetVisibility.is_ready`: whether this holds a usable set.
    pub fn is_ready(&self) -> bool {
        let node_count = self.nodes.len() / 3;
        self.clusters > 0
            && self.row_bytes == (self.clusters + 7) >> 3
            && self.rows.len() as i64 == self.clusters * self.row_bytes
            && self.nodes.len() == node_count * 3
            && node_count > 0
            && self.head >= 0
            && (self.head as usize) < node_count
            && self.planes.len().is_multiple_of(4)
            && !self.leaf_cluster.is_empty()
    }

    /// `DotNetVisibility.cluster_of`: the cluster at `p`, `-1` for none. A node naming a plane
    /// past the table also answers `-1` (the script would stop on the bad index).
    pub fn cluster_of(&self, p: [f32; 3]) -> i32 {
        if let Some((lo, hi)) = self.bounds
            && (p[0] < lo[0]
                || p[1] < lo[1]
                || p[2] < lo[2]
                || p[0] > hi[0]
                || p[1] > hi[1]
                || p[2] > hi[2])
        {
            return -1;
        }
        let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        let mut node = self.head;
        let mut guard = self.nodes.len() / 3 + 1;
        while node >= 0 && guard > 0 {
            guard -= 1;
            let at = node as usize * 3;
            let Some(&[pl, front, back]) = self
                .nodes
                .get(at..at + 3)
                .and_then(|s| <&[i32; 3]>::try_from(s).ok())
            else {
                return -1;
            };
            let Some(pl) = usize::try_from(pl)
                .ok()
                .and_then(|i| self.planes.get(i * 4..i * 4 + 4))
            else {
                return -1;
            };
            let d = f64::from(pl[0]) * x + f64::from(pl[1]) * y + f64::from(pl[2]) * z
                - f64::from(pl[3]);
            node = if d >= 0.0 { front } else { back };
        }
        if node >= 0 {
            return -1;
        }
        let leaf = (-1 - i64::from(node)) as usize;
        self.leaf_cluster.get(leaf).copied().unwrap_or(-1)
    }

    /// `DotNetVisibility.cluster_sees`: whether cluster `from` may see cluster `to`; either out of
    /// range answers true.
    pub fn cluster_sees(&self, from: i32, to: i32) -> bool {
        if from < 0 || to < 0 || i64::from(from) >= self.clusters || i64::from(to) >= self.clusters
        {
            return true;
        }
        let at = i64::from(from) * self.row_bytes + i64::from(to >> 3);
        self.rows
            .get(at as usize)
            .is_none_or(|b| b & (1u8 << (to & 7)) != 0)
    }

    /// `DotNetVisibility.visible`: whether anything at `from` could see anything at `to`.
    pub fn visible(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        if !self.is_ready() {
            return true;
        }
        self.cluster_sees(self.cluster_of(from), self.cluster_of(to))
    }
}

/// A bound as the script tests it: `bounds.has_volume()` (every side above zero), then
/// `position` and `end` (`position + size` in `f32`).
pub fn bounds_of(position: [f32; 3], size: [f32; 3]) -> Option<([f32; 3], [f32; 3])> {
    if size[0] > 0.0 && size[1] > 0.0 && size[2] > 0.0 {
        Some((
            position,
            [
                position[0] + size[0],
                position[1] + size[1],
                position[2] + size[2],
            ],
        ))
    } else {
        None
    }
}
