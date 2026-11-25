//! `GSSimPresentLod`: game-fatal-contact's `FcPlayer.present_lod` judgement for a whole frame of remote bodies.
//!
//! The client hands over every body's position in one `PackedVector3Array` and the camera's frustum, and gets one
//! code per body back (tier, beyond-figure bit, occluder-ray bit; see `gs_sim_present`): one call instead of the
//! per-body vector math and six plane tests in script. Nothing here reads a script object.

use godot::prelude::*;
use gs_sim_math::Vec3 as V3;
use gs_sim_present::{Config, Plane as P, classify};

/// The judgement of one frame, with its buffers kept between frames.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimPresentLod {
    planes: Vec<P>,
    flat: Vec<f32>,
    codes: Vec<i32>,
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
        self.flat.clear();
        for p in positions.as_slice() {
            self.flat.extend_from_slice(&[p.x, p.y, p.z]);
        }
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
