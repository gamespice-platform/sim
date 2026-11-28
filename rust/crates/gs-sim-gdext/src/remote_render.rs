//! `GSSimRemoteRender`: game-fatal-contact's per-body render stages of `FcPlayer.present` (`r_held`, `r_tag`,
//! `r_anim`) for a whole frame of remote bodies in one call each (see `gs_sim_present::render`).
//!
//! `held` keeps every body's retell gate and shot sequence by instance id and answers which guns are told again
//! and how many shots each fired; `place_tags` writes every name tag's screen-parallel transform itself, skipping a
//! tag that has not moved; `anim` computes the figures' motion numbers from packed rows.

use std::collections::HashMap;

use godot::prelude::*;
use gs_sim_present::render::{
    AnimIn, HeldGate, HeldIn, HeldSlot, Rows, anim_one, tag_basis, zero_approx,
};

/// Values per row of [method GSSimRemoteRender.anim]'s input and output.
const ANIM_IN: usize = 14;
const ANIM_OUT: usize = 7;
/// Calls after which a tag not placed since is forgotten.
const TAG_KEEP: u64 = 256;

#[derive(Clone, Copy)]
struct TagSlot {
    xf: Transform3D,
    eye: Vector3,
    placed: bool,
    used: u64,
}

/// The render stages of one frame, with every body's held gate and tag placement kept between frames.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimRemoteRender {
    held: HeldGate,
    inputs: Vec<HeldIn>,
    tags: HashMap<i64, TagSlot>,
    calls: u64,
    written: Vec<f32>,
}

fn rows_of(b: &Basis) -> Rows {
    let r = |v: Vector3| [v.x, v.y, v.z];
    [r(b.rows[0]), r(b.rows[1]), r(b.rows[2])]
}

fn basis_of(r: &Rows) -> Basis {
    let v = |a: [f32; 3]| Vector3::new(a[0], a[1], a[2]);
    Basis::from_rows(v(r[0]), v(r[1]), v(r[2]))
}

#[godot_api]
impl GSSimRemoteRender {
    /// One frame of `_present_held`'s gate for every body of [param ids] (instance ids). [param rows] holds three
    /// per body: flags (1 remote, 2 the gun is the one last told and equipped), state (switching | reloading << 1
    /// | aiming << 2 | shown << 3) and fire sequence; [param charges] one per body. Returns one code per body,
    /// `fired << 1 | tell`; mismatched lengths are an error and return an empty array.
    #[func]
    fn held(
        &mut self,
        ids: PackedInt64Array,
        rows: PackedInt64Array,
        charges: PackedFloat64Array,
        retell: i64,
        wrap: i64,
    ) -> PackedInt64Array {
        let (ids, rows, charges) = (ids.as_slice(), rows.as_slice(), charges.as_slice());
        if rows.len() != ids.len() * 3 || charges.len() != ids.len() {
            godot_error!(
                "GSSimRemoteRender.held: {} ids, {} rows, {} charges",
                ids.len(),
                rows.len(),
                charges.len()
            );
            return PackedInt64Array::new();
        }
        self.inputs.clear();
        for (r, c) in rows.as_chunks::<3>().0.iter().zip(charges) {
            self.inputs.push(HeldIn {
                remote: r[0] & 1 != 0,
                same: r[0] & 2 != 0,
                state: r[1],
                seq: r[2],
                charge: *c,
            });
        }
        self.held.step(ids, &self.inputs, retell, wrap);
        PackedInt64Array::from(self.held.codes.as_slice())
    }

    /// Body [param id]'s held gate: told, state, charge, seen sequence; empty if it has none.
    #[func]
    fn held_slot(&self, id: i64) -> PackedFloat64Array {
        match self.held.slot(id) {
            Some(s) => PackedFloat64Array::from(
                [s.told as f64, s.state as f64, s.charge, s.seen as f64].as_slice(),
            ),
            None => PackedFloat64Array::new(),
        }
    }

    /// Starts body [param id]'s held gate from what the script told it last: [param told] frames since, the
    /// [param state] and [param charge] told, the fire sequence [param seen].
    #[func]
    fn held_seed(&mut self, id: i64, told: i64, state: i64, charge: f64, seen: i64) {
        self.held.seed(
            id,
            HeldSlot {
                told,
                state,
                charge,
                seen,
            },
        );
    }

    /// Places every tag of [param tags] (`Node3D`s) at the point of [param at] with the scale of [param scales],
    /// parallel to the screen of a camera at [param eye] whose orthonormalized basis is [param basis], as
    /// `_place_health_tag` does. Returns one code per tag: 0 left where it was, 1 transform written, 2 only its
    /// position written (out of the tree, or the eye on the point), -1 not a `Node3D`.
    /// [method written_transforms] then holds the transform written for each code 1.
    #[func]
    fn place_tags(
        &mut self,
        tags: AnyArray,
        at: PackedVector3Array,
        scales: PackedFloat64Array,
        eye: Vector3,
        basis: Basis,
    ) -> PackedInt32Array {
        let (at, scales) = (at.as_slice(), scales.as_slice());
        let n = tags.len();
        if at.len() != n || scales.len() != n {
            godot_error!(
                "GSSimRemoteRender.place_tags: {} tags, {} points, {} scales",
                n,
                at.len(),
                scales.len()
            );
            return PackedInt32Array::new();
        }
        self.calls += 1;
        let now = self.calls;
        let rows = rows_of(&basis);
        self.written.clear();
        let mut codes = Vec::with_capacity(n);
        for (i, v) in tags.iter_shared().enumerate() {
            let Ok(mut tag) = v.try_to::<Gd<Node3D>>() else {
                codes.push(-1);
                continue;
            };
            let p = at[i];
            let id = tag.instance_id().to_i64();
            let slot = self.tags.entry(id).or_insert(TagSlot {
                xf: Transform3D::IDENTITY,
                eye: Vector3::ZERO,
                placed: false,
                used: now,
            });
            slot.used = now;
            if !tag.is_inside_tree() {
                tag.set_global_position(p);
                slot.placed = false;
                codes.push(2);
                continue;
            }
            if slot.placed
                && eye == slot.eye
                && p == slot.xf.origin
                && tag.get_global_transform() == slot.xf
            {
                codes.push(0);
                continue;
            }
            let to_eye = eye - p;
            if zero_approx([to_eye.x, to_eye.y, to_eye.z]) {
                tag.set_global_position(p);
                slot.placed = false;
                codes.push(2);
                continue;
            }
            let xf = Transform3D::new(basis_of(&tag_basis(&rows, scales[i])), p);
            tag.set_global_transform(xf);
            slot.xf = tag.get_global_transform();
            slot.eye = eye;
            slot.placed = true;
            codes.push(1);
            for r in xf.basis.rows {
                self.written.extend_from_slice(&[r.x, r.y, r.z]);
            }
            self.written.extend_from_slice(&[p.x, p.y, p.z]);
        }
        if now.is_multiple_of(TAG_KEEP) {
            self.tags.retain(|_, s| now - s.used < TAG_KEEP);
        }
        PackedInt32Array::from(codes.as_slice())
    }

    /// Twelve floats (basis rows, then origin) per tag the last [method place_tags] wrote a transform to, in order.
    #[func]
    fn written_transforms(&self) -> PackedFloat32Array {
        PackedFloat32Array::from(self.written.as_slice())
    }

    /// `_animate_figure`'s numbers for every body: [param rows] holds fourteen per body (velocity x y z, position
    /// x y z, wall normal x y z, radius, pitch in degrees, previous fire sequence, fire sequence, 1 for a melee
    /// hold). Returns seven per body: speed, aim (radians), wall plane normal x y z and d, 1 for a swing.
    #[func]
    fn anim(&self, rows: PackedFloat64Array, wrap: i64) -> PackedFloat64Array {
        let rows = rows.as_slice();
        if !rows.len().is_multiple_of(ANIM_IN) {
            godot_error!(
                "GSSimRemoteRender.anim: {} values, not rows of {}",
                rows.len(),
                ANIM_IN
            );
            return PackedFloat64Array::new();
        }
        let mut out = Vec::with_capacity(rows.len() / ANIM_IN * ANIM_OUT);
        for r in rows.as_chunks::<ANIM_IN>().0 {
            let f = |k: usize| r[k] as f32;
            let o = anim_one(
                &AnimIn {
                    velocity: [f(0), f(1), f(2)],
                    position: [f(3), f(4), f(5)],
                    wall_normal: [f(6), f(7), f(8)],
                    radius: r[9],
                    pitch: r[10],
                    previous_seq: r[11] as i64,
                    seq: r[12] as i64,
                    melee: r[13] != 0.0,
                },
                wrap,
            );
            out.extend_from_slice(&[
                f64::from(o.speed),
                o.aim,
                f64::from(o.wall[0]),
                f64::from(o.wall[1]),
                f64::from(o.wall[2]),
                f64::from(o.wall[3]),
                f64::from(u8::from(o.swing)),
            ]);
        }
        PackedFloat64Array::from(out.as_slice())
    }

    /// Forgets body [param id]'s held gate (it was freed, or its gun rebuilt).
    #[func]
    fn forget(&mut self, id: i64) {
        self.held.forget(id);
    }

    /// Forgets every body's held gate and every tag's placement.
    #[func]
    fn reset(&mut self) {
        self.held.clear();
        self.tags.clear();
        self.written.clear();
    }
}
