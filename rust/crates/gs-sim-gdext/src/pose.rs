//! `GSSimPoseBlend`: a replayed figure's pose drawn between two recorded rows.
//!
//! The twin of `GSHumanoidPoseRecord.apply` (gs-player-char): the model's transform interpolated
//! (`Transform3D.interpolate_with`, still the engine's), every bone's rotation the slerp of the two
//! normalized rows' quaternions (`gs_sim_math::quat_slerp`, the engine's order and rounding) and
//! the root bones' positions lerped, written straight into the skeleton. The script's per-bone
//! `Quaternion` construction, `normalized`, `slerp` and array reads go; the two rows are plain
//! float arrays, so no script field is read from Rust.

use godot::classes::{Node3D, Skeleton3D};
use godot::prelude::*;
use gs_sim_math::{lerp_f32, quat_normalized, quat_slerp};

/// Floats before the bone rotations: bone count, the model's basis (9) and origin (3).
const HEAD: usize = 13;

/// Stateless; one instance serves every figure.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimPoseBlend {}

#[godot_api]
impl GSSimPoseBlend {
    /// Draws the pose between rows [param a] and [param b], [param alpha] of the way to [param b],
    /// into [param skeleton] and [param model]; [param roots] are the bones whose position a row
    /// carries. False (and nothing changed) when a row is not this skeleton's.
    #[func]
    fn apply(
        &self,
        skeleton: Option<Gd<Skeleton3D>>,
        model: Option<Gd<Node3D>>,
        a: PackedFloat32Array,
        b: PackedFloat32Array,
        alpha: f64,
        roots: PackedInt32Array,
    ) -> bool {
        let (Some(mut skeleton), Some(mut model)) = (skeleton, model) else {
            return false;
        };
        let (a, b, roots) = (a.as_slice(), b.as_slice(), roots.as_slice());
        let n = skeleton.get_bone_count();
        let Ok(count) = usize::try_from(n) else {
            return false;
        };
        if a.is_empty() || a.len() != b.len() || a[0] as i64 != i64::from(n) {
            return false;
        }
        if a.len() != HEAD + count * 4 + roots.len() * 3 {
            return false;
        }
        let w = alpha as f32;
        model.set_transform(model_transform(a, b, w));
        for i in 0..count {
            let k = HEAD + i * 4;
            let qa = quat_normalized([a[k], a[k + 1], a[k + 2], a[k + 3]]);
            let qb = quat_normalized([b[k], b[k + 1], b[k + 2], b[k + 3]]);
            let q = quat_slerp(qa, qb, w);
            skeleton.set_bone_pose_rotation(i as i32, Quaternion::new(q[0], q[1], q[2], q[3]));
        }
        let mut k = HEAD + count * 4;
        for &bone in roots {
            let p = Vector3::new(
                lerp_f32(a[k], b[k], w),
                lerp_f32(a[k + 1], b[k + 1], w),
                lerp_f32(a[k + 2], b[k + 2], w),
            );
            skeleton.set_bone_pose_position(bone, p);
            k += 3;
        }
        true
    }
}

/// A row's model transform: the basis's three columns' components in order, then the origin.
fn model_transform(a: &[f32], b: &[f32], w: f32) -> Transform3D {
    let row = |r: &[f32]| {
        Transform3D::new(
            Basis::from_cols(
                Vector3::new(r[1], r[2], r[3]),
                Vector3::new(r[4], r[5], r[6]),
                Vector3::new(r[7], r[8], r[9]),
            ),
            Vector3::new(r[10], r[11], r[12]),
        )
    };
    row(a).interpolate_with(&row(b), w)
}
