//! `GSSimPropDoubt`: mg-prop-hunt's `PhBots._doubt` walk over the map's own pieces.
//!
//! The game writes the map's pieces once a map (`set_pieces`: each piece's middle as the script
//! lifts it) and their broken flags as they change (`set_broken`), then asks `scan` each time a
//! hunter doubts: one call instead of the script's per-piece dictionary, transform and size
//! reads. The pieces kept are the script's to the bit and in its order (`dot_sim_doubt`; the
//! game's `headless_native_doubt` and its `doubt_verify` hold the two together).

use dot_sim_doubt::{Piece, scan};
use dot_sim_math::Vec3;
use godot::prelude::*;

/// A map's pieces (middle and broken flag) and the doubt scan over them.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimPropDoubt {
    pieces: Vec<Piece>,
    kept: Vec<i32>,
}

#[godot_api]
impl GSSimPropDoubt {
    /// Holds the map's pieces, one per middle in [param middles], all whole.
    #[func]
    fn set_pieces(&mut self, middles: PackedVector3Array) {
        self.pieces = middles
            .as_slice()
            .iter()
            .map(|m| Piece {
                middle: v3(*m),
                broken: false,
            })
            .collect();
    }

    /// How many pieces are held.
    #[func]
    fn count(&self) -> i32 {
        self.pieces.len() as i32
    }

    /// Piece [param index] broken or whole. An index outside the table is an error and changes
    /// nothing.
    #[func]
    fn set_broken(&mut self, index: i32, broken: bool) {
        let Some(piece) = usize::try_from(index)
            .ok()
            .and_then(|i| self.pieces.get_mut(i))
        else {
            godot_error!(
                "GSSimPropDoubt.set_broken: index {index} outside {} pieces",
                self.pieces.len()
            );
            return;
        };
        piece.broken = broken;
    }

    /// The indices of the whole pieces within [param reach] of [param eye] (not nearer than half
    /// a metre) and within half [param fov] degrees of [param forward], in the map's order.
    #[func]
    fn scan(&mut self, eye: Vector3, forward: Vector3, fov: f64, reach: f64) -> PackedInt32Array {
        self.kept.clear();
        scan(
            &self.pieces,
            v3(eye),
            v3(forward),
            fov,
            reach,
            &mut self.kept,
        );
        PackedInt32Array::from(self.kept.as_slice())
    }
}

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}
