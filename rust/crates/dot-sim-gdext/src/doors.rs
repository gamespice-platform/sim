//! `DotSimDoorReach`: game-murder-mystery's `MmDoors.door_for` over a table of posed panels.
//!
//! The game writes a door's panel each time it poses it (`set_door`: the transform, its
//! `affine_inverse()` from the engine, the box size), then asks `door_for` as often as it likes:
//! one call instead of the script's per-door dictionary reads, inverse and clamps. The same
//! index as the script to the bit (`dot_sim_doors`; the game's `headless_native_doors` and its
//! `reach_verify` hold the two together).

use dot_sim_doors::{Panel, door_for};
use dot_sim_math::{Vec3, Xform};
use godot::prelude::*;

/// A map's door panels where they were last posed, and the reach ask over them.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimDoorReach {
    panels: Vec<Panel>,
}

#[godot_api]
impl DotSimDoorReach {
    /// Holds [param count] doors, the new ones unposed (identity, zero size); fails loudly below zero.
    #[func]
    fn resize(&mut self, count: i32) {
        match usize::try_from(count) {
            Ok(n) => self.panels.resize(n, Panel::default()),
            Err(_) => {
                godot_error!("DotSimDoorReach.resize: count {count} is negative");
            }
        }
    }

    /// How many doors are held.
    #[func]
    fn count(&self) -> i32 {
        self.panels.len() as i32
    }

    /// Door [param index]'s panel as posed: its transform, that transform's `affine_inverse()`
    /// and its box size. An index outside the table is an error and changes nothing.
    #[func]
    fn set_door(&mut self, index: i32, frame: Transform3D, inverse: Transform3D, size: Vector3) {
        let Some(panel) = usize::try_from(index)
            .ok()
            .and_then(|i| self.panels.get_mut(i))
        else {
            godot_error!(
                "DotSimDoorReach.set_door: index {index} outside {} doors",
                self.panels.len()
            );
            return;
        };
        *panel = Panel {
            frame: xf(&frame),
            inverse: xf(&inverse),
            size: v3(size),
        };
    }

    /// `MmDoors.door_for`: the nearest door within [param reach] of [param eyes] and in front of
    /// [param aim] (or close enough to touch), `-1` for none.
    #[func]
    fn door_for(&self, eyes: Vector3, aim: Vector3, reach: f64) -> i64 {
        door_for(&self.panels, v3(eyes), v3(aim), reach)
    }
}

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn xf(t: &Transform3D) -> Xform {
    Xform {
        rows: [
            v3(t.basis.rows[0]),
            v3(t.basis.rows[1]),
            v3(t.basis.rows[2]),
        ],
        origin: v3(t.origin),
    }
}
