//! `GSSimWorldHold`: fc-weapons `AwWorldModel.advance`'s pose arithmetic (the action keys sampled, the Euler
//! turns, reload dip, sprint carry, wall tilt and the holder transform), bit for bit the script's.
//! Presentation only: it never feeds the simulation.

use godot::prelude::*;
use gs_sim_hold::{
    HoldIn, Key, Reload, euler_yxz, hold, keys_from_rows, sample_field, support_point,
};
use gs_sim_math::{Vec3 as V3, Xform};

#[inline]
fn v3(v: Vector3) -> V3 {
    V3::new(v.x, v.y, v.z)
}

#[inline]
fn gv(v: V3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

#[inline]
fn xf(t: Transform3D) -> Xform {
    let r = t.basis.rows;
    Xform {
        rows: [v3(r[0]), v3(r[1]), v3(r[2])],
        origin: v3(t.origin),
    }
}

#[inline]
fn gt(x: Xform) -> Transform3D {
    Transform3D::new(
        Basis::from_rows(gv(x.rows[0]), gv(x.rows[1]), gv(x.rows[2])),
        gv(x.origin),
    )
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
        let h = HoldIn {
            acting,
            action,
            looking,
            looked_moved: v3(looked_moved),
            looked_degrees: v3(looked_degrees),
            reload,
            rest: xf(rest),
            sprint,
            sprint_offset: v3(sprint_offset),
            sprint_degrees: v3(sprint_degrees),
            wall_tilt,
            push,
        };
        gt(hold(&self.keys, &h))
    }
}

/// Floats per gun in [method GSSimWorldHoldBatch.step]: slot, flags, action, looked moved xyz, looked
/// degrees xyz, reload dip share, sprint, wall tilt, push, reload blend, reload time, reload period.
const ROW: usize = 16;
/// Row flag: an action is playing (`HoldIn::acting`).
const ACTING: u32 = 1;
/// Row flag: the inspect turn shows (`HoldIn::looking`).
const LOOKING: u32 = 2;
/// Row flag: set the holder's transform this frame.
const POSE: u32 = 4;
/// Row flag: tell the person where the support hand goes, from the holder before it is posed.
const SUPPORT: u32 = 8;

/// What a slot keeps between frames: the nodes it writes, the action keys and the art's numbers.
struct Slot {
    holder: InstanceId,
    person: Option<InstanceId>,
    keys: Vec<Key>,
    rest: Xform,
    sprint_offset: V3,
    sprint_degrees: V3,
    support_local: V3,
}

/// Every held world model's hand in one call a frame: fc-weapons' `aw_world_ticks.gd` gathers one row
/// per gun ([constant ROW] floats) and [method step] runs, gun by gun, what `AwWorldModel` does once its
/// script has settled the frame: the support point sent to the person (`_follow_person`,
/// `_support_world`) and the holder's transform (`GSSimWorldHold.hold`). The rest transform and sprint
/// carry are kept per slot ([method set_art]) instead of being read off the art every frame.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimWorldHoldBatch {
    slots: Vec<Option<Slot>>,
    support_keys: Vec<Key>,
    grip: StringName,
}

#[godot_api]
impl IRefCounted for GSSimWorldHoldBatch {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            slots: Vec::new(),
            support_keys: Vec::new(),
            grip: StringName::from("set_support_grip"),
        }
    }
}

#[godot_api]
impl GSSimWorldHoldBatch {
    /// A slot for a gun whose pose goes to [param holder]; a closed slot is reused.
    #[func]
    fn open(&mut self, holder: Gd<Node3D>) -> i64 {
        let slot = Slot {
            holder: holder.instance_id(),
            person: None,
            keys: Vec::new(),
            rest: Xform::IDENTITY,
            sprint_offset: V3::default(),
            sprint_degrees: V3::default(),
            support_local: V3::default(),
        };
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            self.slots[i] = Some(slot);
            return i as i64;
        }
        self.slots.push(Some(slot));
        (self.slots.len() - 1) as i64
    }

    /// Frees [param slot] for the next [method open].
    #[func]
    fn close(&mut self, slot: i64) {
        if let Some(s) = self.slots.get_mut(slot as usize) {
            *s = None;
        }
    }

    /// How many slots are open. For a suite.
    #[func]
    fn open_count(&self) -> i64 {
        self.slots.iter().filter(|s| s.is_some()).count() as i64
    }

    /// The object told `set_support_grip(point)` for [param slot]; null for none.
    #[func]
    fn set_person(&mut self, slot: i64, person: Option<Gd<Object>>) {
        if let Some(Some(s)) = self.slots.get_mut(slot as usize) {
            s.person = person.map(|p| p.instance_id());
        }
    }

    /// The playing action's keys for [param slot], as `GSSimWorldHold.set_keys` takes them.
    #[func]
    fn set_keys(&mut self, slot: i64, rows: PackedFloat64Array) {
        if let Some(Some(s)) = self.slots.get_mut(slot as usize) {
            s.keys = keys_from_rows(rows.as_slice());
        }
    }

    /// The art's numbers for [param slot]: the rest transform (`AwWorldModel._rest`), the full sprint
    /// carry's offset and degrees, and the support hand's point in the holder's space.
    #[func]
    fn set_art(
        &mut self,
        slot: i64,
        rest: Transform3D,
        sprint_offset: Vector3,
        sprint_degrees: Vector3,
        support_local: Vector3,
    ) {
        if let Some(Some(s)) = self.slots.get_mut(slot as usize) {
            s.rest = xf(rest);
            s.sprint_offset = v3(sprint_offset);
            s.sprint_degrees = v3(sprint_degrees);
            s.support_local = v3(support_local);
        }
    }

    /// `AwWorldModel.RELOAD_SUPPORT_KEYS`, packed as [method set_keys] takes them.
    #[func]
    fn set_support_keys(&mut self, rows: PackedFloat64Array) {
        self.support_keys = keys_from_rows(rows.as_slice());
    }

    /// One frame for every row in [param rows], in order. A row whose slot is closed or whose holder
    /// is gone is skipped.
    #[func]
    fn step(&self, rows: PackedFloat64Array) {
        for r in rows.as_slice().as_chunks::<ROW>().0 {
            let Some(Some(s)) = self.slots.get(r[0] as usize) else {
                continue;
            };
            let Ok(mut holder) = Gd::<Node3D>::try_from_instance_id(s.holder) else {
                continue;
            };
            let flags = r[1] as u32;
            if flags & SUPPORT != 0
                && let Some(mut person) = s
                    .person
                    .and_then(|id| Gd::<Object>::try_from_instance_id(id).ok())
            {
                let reload = Reload {
                    blend: r[13],
                    time: r[14],
                    period: r[15],
                };
                let at = support_point(
                    &xf(holder.get_global_transform()),
                    s.support_local,
                    reload,
                    &self.support_keys,
                );
                person.call(&self.grip, &[gv(at).to_variant()]);
            }
            if flags & POSE != 0 {
                let h = HoldIn {
                    acting: flags & ACTING != 0,
                    action: r[2],
                    looking: flags & LOOKING != 0,
                    looked_moved: V3::new(r[3] as f32, r[4] as f32, r[5] as f32),
                    looked_degrees: V3::new(r[6] as f32, r[7] as f32, r[8] as f32),
                    reload: r[9],
                    rest: s.rest,
                    sprint: r[10],
                    sprint_offset: s.sprint_offset,
                    sprint_degrees: s.sprint_degrees,
                    wall_tilt: r[11],
                    push: r[12],
                };
                holder.set_transform(gt(hold(&s.keys, &h)));
            }
        }
    }
}
