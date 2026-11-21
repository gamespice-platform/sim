//! Prop Hunt's doubt scan: mg-prop-hunt's `PhBots._doubt` over the map's own pieces, to the bit.
//!
//! The script walks every piece of the map each time a hunter doubts (a dictionary, a transform
//! and a size read per piece, most of them far off or behind): this is that walk over a table
//! of each piece's middle, written once a map. A piece is kept when it is whole, inside the
//! square reach round the eye (`absf` of the `f64` difference of the `f32` components, as the
//! script reads them), its `f32` distance from the eye is within reach and not under
//! [`NEAR`], and its `f32` angle from the hunter's forward, in `f64` degrees, is within half the
//! field of view. The pieces kept come back in the map's order; the line of sight, the cleared
//! list and the oddness stay the script's.

use gs_sim_math::{Vec3, rad_to_deg};

#[cfg(test)]
mod tests;

/// Closer than this (metres) a piece is not doubted, as the script's `0.5`.
pub const NEAR: f64 = 0.5;

/// One of the map's pieces: its middle (its origin lifted half its height, as the script lifts
/// it) and whether it is broken.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Piece {
    /// `frame.origin + Vector3(0.0, size.y * 0.5, 0.0)`.
    pub middle: Vec3,
    /// Broken this round: not doubted.
    pub broken: bool,
}

/// Whether `piece` is near and in view of `eye` looking along `forward`: the script's square
/// reach, distance and field of view tests in its order and precision.
#[inline]
pub fn in_view(piece: &Piece, eye: Vec3, forward: Vec3, fov: f64, reach: f64) -> bool {
    if piece.broken {
        return false;
    }
    let m = piece.middle;
    if (f64::from(m.x) - f64::from(eye.x)).abs() > reach
        || (f64::from(m.z) - f64::from(eye.z)).abs() > reach
    {
        return false;
    }
    let d = f64::from(eye.distance_to(m));
    if d > reach || d < NEAR {
        return false;
    }
    rad_to_deg(f64::from(forward.angle_to(m - eye))) <= fov * 0.5
}

/// The indices of the `pieces` near and in view, in the map's order, appended to `out`.
pub fn scan(pieces: &[Piece], eye: Vec3, forward: Vec3, fov: f64, reach: f64, out: &mut Vec<i32>) {
    for (i, piece) in pieces.iter().enumerate() {
        if in_view(piece, eye, forward, fov, reach) {
            out.push(i as i32);
        }
    }
}
