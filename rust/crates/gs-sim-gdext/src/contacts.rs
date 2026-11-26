//! `GSSimBotContacts`: a bot world's per-tick contact fields and sight rows, natively
//! (`gs_sim_contacts`). The script packs one record per player; [method fill] writes every
//! per-tick `GSBotContact` field and keeps the rows a native world's `set_rows` takes;
//! [method fill_changed] writes the fields a player keeps from tick to tick only when they change.

use godot::prelude::*;
use gs_sim_contacts::{STABLE, STRIDE, Stable, contact, lift};

/// The contact fields written, in [`GSSimBotContacts::fill`]'s order.
const FIELDS: [&str; 13] = [
    "eye_height",
    "aim_height",
    "alive",
    "team",
    "position",
    "velocity",
    "view_yaw",
    "view_pitch",
    "grounded",
    "sliding",
    "wall_running",
    "air_jump_ready",
    "health",
];

/// The [`FIELDS`] a [`Stable`] holds, in its order.
const KEPT: [usize; STABLE] = [0, 1, 2, 3, 8, 9, 10, 11, 12];

fn vec3(v: [f32; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBotContacts {
    names: Vec<StringName>,
    points: PackedVector3Array,
    eyes: PackedVector3Array,
    positions: PackedVector3Array,
    teams: PackedInt32Array,
    alive: PackedByteArray,
    /// Per contact slot: the object written there last and the kept fields it was given.
    kept: Vec<(InstanceId, Stable)>,
}

#[godot_api]
impl GSSimBotContacts {
    /// The floats in one player's record (head, then its `GSFpsState` columns).
    #[func]
    fn stride(&self) -> i64 {
        STRIDE as i64
    }

    /// Writes each of [param contacts] from its record in [param records] (one [method stride] each,
    /// in order): eye and aim heights, alive, team, position, velocity, view, motion flags and the
    /// health ratio, as the script's contact build does. Keeps the sight rows for [method points],
    /// [method eyes], [method positions], [method teams] and [method alive]. False when the
    /// records do not match the contacts or one is not an object.
    #[func]
    fn fill(
        &mut self,
        contacts: VarArray,
        records: PackedFloat64Array,
        eye: f64,
        aim: f64,
        launch_enabled: bool,
    ) -> bool {
        self.write(&contacts, &records, eye, aim, launch_enabled, false)
    }

    /// [method fill], with the fields a player keeps from tick to tick (eye and aim heights,
    /// alive, team, motion flags, health ratio) written only when they differ, to the bit, from
    /// what this object last wrote to the same contact in the same place; position, velocity and
    /// view are always written. Leaves the same contacts as [method fill] as long as only this
    /// object writes those fields: call [method forget] after anything else did.
    #[func]
    fn fill_changed(
        &mut self,
        contacts: VarArray,
        records: PackedFloat64Array,
        eye: f64,
        aim: f64,
        launch_enabled: bool,
    ) -> bool {
        self.write(&contacts, &records, eye, aim, launch_enabled, true)
    }

    /// Drops what [method fill_changed] remembers writing, so its next call writes every field.
    #[func]
    fn forget(&mut self) {
        self.kept.clear();
    }

    /// The last [method fill]'s aim points (`position + UP * aim`).
    #[func]
    fn points(&self) -> PackedVector3Array {
        self.points.clone()
    }

    /// The last [method fill]'s eye points (`position + UP * eye`).
    #[func]
    fn eyes(&self) -> PackedVector3Array {
        self.eyes.clone()
    }

    /// The last [method fill]'s positions.
    #[func]
    fn positions(&self) -> PackedVector3Array {
        self.positions.clone()
    }

    /// The last [method fill]'s teams (`PackedInt32Array`, as the rows take them).
    #[func]
    fn teams(&self) -> PackedInt32Array {
        self.teams.clone()
    }

    /// The last [method fill]'s alive flags (1 or 0).
    #[func]
    fn alive(&self) -> PackedByteArray {
        self.alive.clone()
    }
}

impl GSSimBotContacts {
    fn write(
        &mut self,
        contacts: &VarArray,
        records: &PackedFloat64Array,
        eye: f64,
        aim: f64,
        launch_enabled: bool,
        changed_only: bool,
    ) -> bool {
        let n = contacts.len();
        let r = records.as_slice();
        if r.len() != n * STRIDE {
            self.kept.clear();
            return false;
        }
        if self.names.is_empty() {
            self.names = FIELDS.iter().map(|f| StringName::from(*f)).collect();
        }
        let (mut points, mut eyes, mut positions) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let (mut teams, mut alive) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let mut kept = Vec::with_capacity(n);
        for (i, rec) in r.as_chunks::<STRIDE>().0.iter().enumerate() {
            let Ok(mut obj) = contacts.at(i).try_to::<Gd<Object>>() else {
                self.kept.clear();
                return false;
            };
            let c = contact(rec, launch_enabled);
            let now = Stable::of(&c, eye, aim);
            let id = obj.instance_id();
            let before = self
                .kept
                .get(i)
                .filter(|(was, _)| changed_only && *was == id)
                .map(|(_, s)| s);
            let mask = now.changed(before);
            let k = &self.names;
            let kept_values = [
                eye.to_variant(),
                aim.to_variant(),
                c.alive.to_variant(),
                c.team.to_variant(),
                c.grounded.to_variant(),
                c.sliding.to_variant(),
                c.wall_running.to_variant(),
                c.air_jump_ready.to_variant(),
                c.health.to_variant(),
            ];
            // FIELDS order: the kept fields before position (0..4), the moving ones, then the rest.
            for (j, &f) in KEPT.iter().enumerate().take(4) {
                if mask & (1 << j) != 0 {
                    obj.set(&k[f], &kept_values[j]);
                }
            }
            obj.set(&k[4], &vec3(c.position).to_variant());
            obj.set(&k[5], &vec3(c.velocity).to_variant());
            obj.set(&k[6], &c.view_yaw.to_variant());
            obj.set(&k[7], &c.view_pitch.to_variant());
            for (j, &f) in KEPT.iter().enumerate().skip(4) {
                if mask & (1 << j) != 0 {
                    obj.set(&k[f], &kept_values[j]);
                }
            }
            kept.push((id, now));
            points.push(vec3(lift(c.position, aim)));
            eyes.push(vec3(lift(c.position, eye)));
            positions.push(vec3(c.position));
            teams.push(c.team as i32);
            alive.push(u8::from(c.alive));
        }
        self.kept = kept;
        self.points = PackedVector3Array::from(points.as_slice());
        self.eyes = PackedVector3Array::from(eyes.as_slice());
        self.positions = PackedVector3Array::from(positions.as_slice());
        self.teams = PackedInt32Array::from(teams.as_slice());
        self.alive = PackedByteArray::from(alive.as_slice());
        true
    }
}
