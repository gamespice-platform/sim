//! `GSSimBotContacts`: a bot world's per-tick contact fields and sight rows, natively
//! (`gs_sim_contacts`). The script packs one record per player; [method fill] writes every
//! per-tick `GSBotContact` field and keeps the rows a native world's `set_rows` takes;
//! [method fill_changed] writes the fields a player keeps from tick to tick only when they change;
//! [method fill_table] writes no object at all, only packed columns for gs-bot's `GSBotContactTable`.
//! [method fill_arms] writes the weapon in hand (`weapon_class`, `magazine`) from interned classes.

use godot::prelude::*;
use gs_sim_contacts::{
    Arms, ROW_FLOATS, STABLE, STRIDE, Stable, contact, lift, row_flags, row_floats,
};

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
    velocities: PackedVector3Array,
    floats: PackedFloat64Array,
    flags: PackedInt32Array,
    /// Per contact slot: the object written there last and the kept fields it was given.
    kept: Vec<(InstanceId, Stable)>,
    /// The style classes [method intern_class] gave out, by index.
    classes: Vec<StringName>,
    /// `weapon_class` and `magazine`, named once.
    arm_names: Option<(StringName, StringName, StringName)>,
    /// Per contact slot: the object [method fill_arms] wrote there last and what it wrote.
    arms_kept: Vec<(InstanceId, (i32, u64))>,
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

    /// The columns of gs-bot's `GSBotContactTable` from [param records], with no object written:
    /// the sight rows as [method fill] keeps them, and [method velocities], [method floats]
    /// ([method row_floats] per row: eye and aim heights, view yaw and pitch, health ratio) and
    /// [method flags] (alive 1, grounded 2, sliding 4, wall running 8, air jump ready 16). The same
    /// values [method fill] writes, to the bit. False when the records are not whole.
    #[func]
    fn fill_table(
        &mut self,
        records: PackedFloat64Array,
        eye: f64,
        aim: f64,
        launch_enabled: bool,
    ) -> bool {
        let r = records.as_slice();
        if !r.len().is_multiple_of(STRIDE) {
            return false;
        }
        let n = r.len() / STRIDE;
        self.kept.clear();
        let mut points = Vec::with_capacity(n);
        let mut eyes = Vec::with_capacity(n);
        let mut positions = Vec::with_capacity(n);
        let mut velocities = Vec::with_capacity(n);
        let mut teams = Vec::with_capacity(n);
        let mut alive = Vec::with_capacity(n);
        let mut floats = Vec::with_capacity(n * ROW_FLOATS);
        let mut flags = Vec::with_capacity(n);
        for rec in r.as_chunks::<STRIDE>().0 {
            let c = contact(rec, launch_enabled);
            points.push(vec3(lift(c.position, aim)));
            eyes.push(vec3(lift(c.position, eye)));
            positions.push(vec3(c.position));
            velocities.push(vec3(c.velocity));
            teams.push(c.team as i32);
            alive.push(u8::from(c.alive));
            floats.extend_from_slice(&row_floats(&c, eye, aim));
            flags.push(row_flags(&c));
        }
        self.points = PackedVector3Array::from(points.as_slice());
        self.eyes = PackedVector3Array::from(eyes.as_slice());
        self.positions = PackedVector3Array::from(positions.as_slice());
        self.velocities = PackedVector3Array::from(velocities.as_slice());
        self.teams = PackedInt32Array::from(teams.as_slice());
        self.alive = PackedByteArray::from(alive.as_slice());
        self.floats = PackedFloat64Array::from(floats.as_slice());
        self.flags = PackedInt32Array::from(flags.as_slice());
        true
    }

    /// Floats per [method floats] row.
    #[func]
    fn row_floats(&self) -> i64 {
        ROW_FLOATS as i64
    }

    /// The last [method fill_table]'s velocities.
    #[func]
    fn velocities(&self) -> PackedVector3Array {
        self.velocities.clone()
    }

    /// The last [method fill_table]'s floats, [method row_floats] per row.
    #[func]
    fn floats(&self) -> PackedFloat64Array {
        self.floats.clone()
    }

    /// The last [method fill_table]'s flag bits per row.
    #[func]
    fn flags(&self) -> PackedInt32Array {
        self.flags.clone()
    }

    /// The index of style class [param name] for [method fill_arms], the same for the same name.
    #[func]
    fn intern_class(&mut self, name: StringName) -> i64 {
        if let Some(i) = self.classes.iter().position(|c| *c == name) {
            return i as i64;
        }
        self.classes.push(name);
        (self.classes.len() - 1) as i64
    }

    /// Writes each of [param contacts]' weapon in hand, as the script's `_weapon_of` does:
    /// `weapon_class` is the [method intern_class] name at [param classes] (negative: `&""`) and
    /// `magazine` is `rounds / size` as f64 from [param rounds] (two ints per contact: the slot's
    /// rounds, the definition's magazine size), -1 with no weapon or no magazine. With
    /// [param changed_only], a contact is written only when it differs from what this object last
    /// wrote to the same object in the same place; pass false after anything else wrote them.
    /// False when the arrays do not match the contacts, a class was never interned or a contact
    /// is not an object.
    #[func]
    fn fill_arms(
        &mut self,
        contacts: VarArray,
        classes: PackedInt32Array,
        rounds: PackedInt64Array,
        changed_only: bool,
    ) -> bool {
        let n = contacts.len();
        let (cl, ro) = (classes.as_slice(), rounds.as_slice());
        if cl.len() != n
            || ro.len() != n * 2
            || cl.iter().any(|&c| c as i64 >= self.classes.len() as i64)
        {
            self.arms_kept.clear();
            return false;
        }
        let (klass, mag, none) = self
            .arm_names
            .get_or_insert_with(|| {
                (
                    StringName::from("weapon_class"),
                    StringName::from("magazine"),
                    StringName::default(),
                )
            })
            .clone();
        let mut kept = Vec::with_capacity(n);
        for (i, &class) in cl.iter().enumerate() {
            let Ok(mut obj) = contacts.at(i).try_to::<Gd<Object>>() else {
                self.arms_kept.clear();
                return false;
            };
            let arms = Arms {
                class,
                rounds: ro[i * 2],
                size: ro[i * 2 + 1],
            };
            let now = arms.written();
            let id = obj.instance_id();
            let same = changed_only && self.arms_kept.get(i) == Some(&(id, now));
            if !same {
                let name = if now.0 < 0 {
                    &none
                } else {
                    &self.classes[now.0 as usize]
                };
                obj.set(&klass, &name.to_variant());
                obj.set(&mag, &arms.magazine().to_variant());
            }
            kept.push((id, now));
        }
        self.arms_kept = kept;
        true
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
