//! `GSSimPresentLod`: game-fatal-contact's `FcPlayer.present_lod` judgement for a whole frame of remote bodies.
//!
//! The client hands over every body's position in one `PackedVector3Array` and the camera's frustum, and gets one
//! code per body back (tier, beyond-figure bit, occluder-ray bit; see `gs_sim_present`): one call instead of the
//! per-body vector math and six plane tests in script. Nothing here reads a script object.
//!
//! `schedule` then thins the crowd by those codes and keeps every body's thinning between frames
//! (`FcPlayer.present_lod_coded` for all bodies): the script gets back only the rows to present and the held-gun
//! and figure-range bits that changed.

use godot::prelude::*;
use gs_sim_math::Vec3 as V3;
use gs_sim_present::{Config, OCCLUSION_CANDIDATE, Plane as P, Scheduler, Thinning, classify};

/// The judgement of one frame, with its buffers kept between frames.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimPresentLod {
    planes: Vec<P>,
    flat: Vec<f32>,
    codes: Vec<i32>,
    sched: Scheduler,
    hidden: Vec<i32>,
    ids: Vec<i64>,
}

fn v3(v: Vector3) -> V3 {
    V3::new(v.x, v.y, v.z)
}

#[godot_api]
impl GSSimPresentLod {
    /// One code per body of [param positions] (global, where the script reads `global_position`), judged from
    /// [param eye] looking along [param forward]. [param settings], in this order: near2, cone, radius, farthest2,
    /// far2, anim_far2, anim_near2, figure2 (0: no figure limit), frustum_on (0 or 1), occlusion (0 or 1).
    /// [param frustum] is `Camera3D.get_frustum()`, or empty for the cone. Fewer than ten settings is an error and
    /// returns an empty array.
    #[func]
    fn classify(
        &mut self,
        positions: PackedVector3Array,
        eye: Vector3,
        forward: Vector3,
        frustum: Array<Plane>,
        settings: PackedFloat64Array,
    ) -> PackedInt32Array {
        self.flat.clear();
        for p in positions.as_slice() {
            self.flat.extend_from_slice(&[p.x, p.y, p.z]);
        }
        self.judge(eye, forward, &frustum, &settings)
    }

    /// [method classify] with the gather done here: each of [param bodies] (`Node3D`s) gives its `global_position`,
    /// and its instance id goes to [method gathered_ids], 0 for a body whose `camera` property is set (the viewer's
    /// own, never thinned), as the script's loop read them. A slot that is not a `Node3D` stands at the origin with id 0.
    #[func]
    fn classify_nodes(
        &mut self,
        bodies: AnyArray,
        eye: Vector3,
        forward: Vector3,
        frustum: Array<Plane>,
        settings: PackedFloat64Array,
    ) -> PackedInt32Array {
        self.flat.clear();
        self.ids.clear();
        let camera = StringName::from("camera");
        for v in bodies.iter_shared() {
            match v.try_to::<Gd<Node3D>>() {
                Ok(body) => {
                    let p = body.get_global_position();
                    self.flat.extend_from_slice(&[p.x, p.y, p.z]);
                    let own = !body.get(&camera).is_nil();
                    self.ids
                        .push(if own { 0 } else { body.instance_id().to_i64() });
                }
                Err(_) => {
                    self.flat.extend_from_slice(&[0.0, 0.0, 0.0]);
                    self.ids.push(0);
                }
            }
        }
        self.judge(eye, forward, &frustum, &settings)
    }

    /// One id per body of the last [method classify_nodes]: its instance id, 0 for the viewer's own.
    #[func]
    fn gathered_ids(&self) -> PackedInt64Array {
        PackedInt64Array::from(self.ids.as_slice())
    }

    /// Indices of the bodies the last [method classify] marked for the occluder ray.
    #[func]
    fn occlusion_candidates(&self) -> PackedInt32Array {
        self.codes
            .iter()
            .enumerate()
            .filter(|(_, c)| *c & OCCLUSION_CANDIDATE != 0)
            .map(|(i, _)| i as i32)
            .collect()
    }

    /// One frame of thinning over the codes of the last [method classify]: [param ids] one per body (instance id,
    /// 0 for a body never thinned), [param hidden] the ascending indices the occluder ray hid, [param delta] the
    /// frame's game time. [param settings], in this order: off_every, off_dt, farthest_dt, far_dt, anim_off_dt,
    /// anim_far_dt, anim_dt, anim_gate (0 or 1). Returns rows of (index, delta, anim gate or NaN) for the bodies
    /// to present; [method held_changes] and [method beyond_changes] then hold the bits that changed.
    #[func]
    fn schedule(
        &mut self,
        ids: PackedInt64Array,
        hidden: PackedInt32Array,
        delta: f64,
        settings: PackedFloat64Array,
    ) -> PackedFloat64Array {
        let s = settings.as_slice();
        if s.len() < 8 {
            godot_error!("GSSimPresentLod.schedule: {} settings, need 8", s.len());
            return PackedFloat64Array::new();
        }
        let t = Thinning {
            off_every: s[0] as i32,
            off_dt: s[1],
            farthest_dt: s[2],
            far_dt: s[3],
            anim_off_dt: s[4],
            anim_far_dt: s[5],
            anim_dt: s[6],
            anim_gate: s[7] != 0.0,
        };
        self.hidden.clear();
        self.hidden.extend_from_slice(hidden.as_slice());
        self.hidden.sort_unstable();
        self.sched
            .step(&t, ids.as_slice(), &self.codes, &self.hidden, delta);
        PackedFloat64Array::from(self.sched.presents.as_slice())
    }

    /// `index << 1 | thin` for each body whose held gun changed thinning in the last [method schedule].
    #[func]
    fn held_changes(&self) -> PackedInt32Array {
        PackedInt32Array::from(self.sched.held.as_slice())
    }

    /// `index << 1 | beyond` for each body whose figure-range bit changed in the last [method schedule].
    #[func]
    fn beyond_changes(&self) -> PackedInt32Array {
        PackedInt32Array::from(self.sched.beyond.as_slice())
    }

    /// The thinning kept for body [param id]: frames, delta, thin, beyond; empty if it was not in the last crowd.
    #[func]
    fn slot_state(&self, id: i64) -> PackedFloat64Array {
        match self.sched.slot(id) {
            Some(s) => PackedFloat64Array::from(
                [
                    f64::from(s.frames),
                    s.delta,
                    f64::from(u8::from(s.thin)),
                    f64::from(u8::from(s.beyond)),
                ]
                .as_slice(),
            ),
            None => PackedFloat64Array::new(),
        }
    }

    /// Hands out both bits of body [param id] again on the next [method schedule] (it rebuilt its figure or gun).
    #[func]
    fn refresh(&mut self, id: i64) {
        self.sched.refresh(id);
    }

    /// Forgets every body's thinning.
    #[func]
    fn reset(&mut self) {
        self.sched.clear();
    }
}

impl GSSimPresentLod {
    /// The shared half of [method classify] and [method classify_nodes], on positions already in `flat`.
    fn judge(
        &mut self,
        eye: Vector3,
        forward: Vector3,
        frustum: &Array<Plane>,
        settings: &PackedFloat64Array,
    ) -> PackedInt32Array {
        let s = settings.as_slice();
        if s.len() < 10 {
            godot_error!("GSSimPresentLod.classify: {} settings, need 10", s.len());
            return PackedInt32Array::new();
        }
        let cfg = Config {
            near2: s[0],
            cone: s[1],
            radius: s[2],
            farthest2: s[3],
            far2: s[4],
            anim_far2: s[5],
            anim_near2: s[6],
            figure2: s[7],
            frustum_on: s[8] != 0.0,
            occlusion: s[9] != 0.0,
        };
        self.planes.clear();
        self.planes.extend(frustum.iter_shared().map(|p| P {
            normal: V3::new(p.normal.x, p.normal.y, p.normal.z),
            d: p.d,
        }));
        classify(
            &cfg,
            &self.flat,
            v3(eye),
            v3(forward),
            &self.planes,
            &mut self.codes,
        );
        PackedInt32Array::from(self.codes.as_slice())
    }
}
