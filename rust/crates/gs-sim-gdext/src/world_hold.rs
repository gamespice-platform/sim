//! `GSSimWorldHold`: fc-weapons `AwWorldModel.advance`'s pose arithmetic (the action keys sampled, the Euler
//! turns, reload dip, sprint carry, wall tilt and the holder transform), bit for bit the script's.
//! Presentation only: it never feeds the simulation.

use godot::prelude::*;
use gs_sim_hold::{HoldIn, Key, euler_yxz, hold, keys_from_rows, sample_field};
use gs_sim_math::{Vec3 as V3, Xform};

#[inline]
fn v3(v: Vector3) -> V3 {
    V3::new(v.x, v.y, v.z)
}

#[inline]
fn gv(v: V3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// One per held world model: it keeps the playing action's keys.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimWorldHold {
    keys: Vec<Key>,
}

#[godot_api]
impl GSSimWorldHold {
    /// The playing action's keys, packed `t, pos.x, pos.y, pos.z, rot.x, rot.y, rot.z` per key.
    #[func]
    fn set_keys(&mut self, rows: PackedFloat64Array) {
        self.keys = keys_from_rows(rows.as_slice());
    }

    /// `AwWeaponPose.sample_field(keys, t, field)` over the keys [method set_keys] gave (field 1 position, 2 rotation).
    #[func]
    fn sample(&self, t: f64, field: i64) -> Vector3 {
        gv(sample_field(&self.keys, t, field == 2))
    }

    /// `Basis.from_euler(euler)` (order YXZ).
    #[func]
    fn euler(&self, euler: Vector3) -> Basis {
        let r = euler_yxz(v3(euler));
        Basis::from_rows(gv(r[0]), gv(r[1]), gv(r[2]))
    }

    /// The holder's transform for the frame; see `gs_sim_hold::HoldIn` for each argument.
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn hold(
        &self,
        acting: bool,
        action: f64,
        looking: bool,
        looked_moved: Vector3,
        looked_degrees: Vector3,
        reload: f64,
        rest: Transform3D,
        sprint: f64,
        sprint_offset: Vector3,
        sprint_degrees: Vector3,
        wall_tilt: f64,
        push: f64,
    ) -> Transform3D {
        let r = rest.basis.rows;
        let h = HoldIn {
            acting,
            action,
            looking,
            looked_moved: v3(looked_moved),
            looked_degrees: v3(looked_degrees),
            reload,
            rest: Xform {
                rows: [v3(r[0]), v3(r[1]), v3(r[2])],
                origin: v3(rest.origin),
            },
            sprint,
            sprint_offset: v3(sprint_offset),
            sprint_degrees: v3(sprint_degrees),
            wall_tilt,
            push,
        };
        let x = hold(&self.keys, &h);
        Transform3D::new(
            Basis::from_rows(gv(x.rows[0]), gv(x.rows[1]), gv(x.rows[2])),
            gv(x.origin),
        )
    }
}
