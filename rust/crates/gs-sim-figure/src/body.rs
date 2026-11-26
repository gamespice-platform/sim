//! The arm and wall posing after a figure's clip: `GSHumanoidAim.apply_plan` (the view pitch bent
//! into the waist, chest and gun arm), `GSHumanoidIk.two_bone_under` (the support hand on the
//! foregrip) and `GSHumanoidLegs.on_wall` (the feet against a climbed wall), in the script's order
//! and precision like the rest of the crate.

use crate::{
    Pose, UP, basis_rotation, chain_rotation, global_pose, quat_arc, quat_axis_angle, quat_inverse,
    quat_xform, times, two_bone_to,
};
use gs_sim_math::{
    Quat, Vec3, Xform, clampf, determinant, lerp_f32, quat_mul, quat_normalized, quat_slerp,
};

/// `GSHumanoidLegs.WALL_LIFT`: how far a raised foot stands off the wall per metre it is raised.
pub const WALL_LIFT: f64 = 0.6;

const RIGHT: Vec3 = Vec3::new(1.0, 0.0, 0.0);

/// `GSHumanoidAim.apply_plan`. `plan` is the script's plan flat, four per bone: index, parent,
/// share, and 1 for an arm bone; `drop` is the sprint drop an arm bone adds
/// (`-SPRINT_DROP * sprint_blend`). Turns nothing when `to_figure` has no inverse.
pub fn aim<P: Pose>(p: &mut P, plan: &[f64], to_figure: &[Vec3; 3], pitch: f64, drop: f64) {
    // Figure right in skeleton space: the inverse's first column, as `basis.inverse() * RIGHT`.
    let mut axis = Vec3::default();
    if (determinant(to_figure) as f64).abs() > 1e-9 {
        let inv = Xform {
            rows: *to_figure,
            origin: Vec3::default(),
        }
        .affine_inverse();
        axis = inv.basis_xform(RIGHT).normalized();
    }
    let finite = axis.x.is_finite() && axis.y.is_finite() && axis.z.is_finite();
    if !finite || axis.length_squared() < 0.5 {
        return;
    }
    // Parents first, before any bone turns (the script's two loops).
    let (bones, _) = plan.as_chunks::<4>();
    let parents: Vec<Quat> = bones
        .iter()
        .map(|b| chain_rotation(p, b[1] as i32))
        .collect();
    for (b, &pq) in bones.iter().zip(&parents) {
        let bone = b[0] as i32;
        let d = if b[3] != 0.0 { drop } else { 0.0 };
        let turn = quat_axis_angle(axis, (pitch * b[2] + d) as f32);
        let q = quat_mul(quat_mul(quat_mul(quat_inverse(pq), turn), pq), p.rot(bone));
        p.set_rot(bone, quat_normalized(q));
    }
}

/// `GSHumanoidIk.two_bone_under`: turns `bones[0]` and `bones[1]` so `bones[2]` reaches `target`
/// (skeleton space), the elbow toward `pole_dir`, given the upper bone's parent's skeleton-space pose.
pub fn two_bone_under<P: Pose>(
    p: &mut P,
    bones: [i32; 3],
    upper_global_parent: &Xform,
    target: Vec3,
    pole_dir: Vec3,
    weight: f64,
) {
    let upper = upper_global_parent.mul(&p.pose(bones[0]));
    let lower = if p.parent(bones[1]) == bones[0] {
        upper.mul(&p.pose(bones[1]))
    } else {
        global_pose(p, bones[1])
    };
    let wrist = if p.parent(bones[2]) == bones[1] {
        lower.mul(&p.pose(bones[2]))
    } else {
        global_pose(p, bones[2])
    };
    let a = lower.origin - upper.origin;
    let b = wrist.origin - lower.origin;
    let la = a.length() as f64;
    let lb = b.length() as f64;
    let to_target = target - upper.origin;
    if la < 0.0001 || lb < 0.0001 || (to_target.length() as f64) < 0.0001 {
        return;
    }
    let reach = clampf(
        to_target.length() as f64,
        (la - lb).abs() + 0.001,
        la + lb - 0.001,
    );
    let dir = to_target.normalized();
    let along = (la * la - lb * lb + reach * reach) / (2.0 * reach);
    let height = (la * la - along * along).max(0.0).sqrt();
    let side = pole_dir - dir.scale(pole_dir.dot(dir) as f64);
    let side = if (side.length_squared() as f64) > 0.000001 {
        side.normalized()
    } else {
        UP
    };
    let elbow = upper.origin + dir.scale(along) + side.scale(height);
    let hand_goal = upper.origin + dir.scale(reach);
    let q1 = quat_arc(a.normalized(), (elbow - upper.origin).normalized());
    let q2 = quat_arc(
        quat_xform(q1, b).normalized(),
        (hand_goal - elbow).normalized(),
    );
    let upper_rot = quat_normalized(quat_mul(q1, basis_rotation(&upper)));
    let lower_rot = quat_normalized(quat_mul(quat_mul(q2, q1), basis_rotation(&lower)));
    let upper_parent = basis_rotation(upper_global_parent);
    let upper_local = quat_normalized(quat_mul(quat_inverse(upper_parent), upper_rot));
    let lower_local = quat_normalized(quat_mul(quat_inverse(upper_rot), lower_rot));
    let w = clampf(weight, 0.0, 1.0) as f32;
    let r = quat_slerp(p.rot(bones[0]), upper_local, w);
    p.set_rot(bones[0], r);
    let r = quat_slerp(p.rot(bones[1]), lower_local, w);
    p.set_rot(bones[1], r);
}

/// One leg [`on_wall`] poses: its upper, lower and foot bones, the foot's rest origin in skeleton
/// space, and that origin in the lower bone's rest frame.
#[derive(Clone, Debug, Default)]
pub struct WallLeg {
    pub upper: i32,
    pub lower: i32,
    pub foot: i32,
    pub foot_rest: Vec3,
    pub end_local: Vec3,
}

/// A wall as the script's `Plane` in skeleton space: its normal and distance from the origin.
#[derive(Clone, Copy, Debug)]
pub struct Wall {
    pub normal: Vec3,
    pub d: f32,
}

impl Wall {
    pub(crate) fn distance_to(&self, v: Vec3) -> f32 {
        self.normal.dot(v) - self.d
    }

    fn project(&self, v: Vec3) -> Vec3 {
        v - times(self.normal, self.distance_to(v))
    }
}

/// `GSHumanoidLegs._within`: `target` on the wall, pulled in to `span` from `hip`.
fn within(wall: &Wall, hip: Vec3, target: Vec3, span: f64) -> Vec3 {
    let off = wall.distance_to(hip).abs() as f64;
    if (hip.distance_to(target) as f64) <= span || off >= span {
        return target;
    }
    let foot = wall.project(hip);
    let along = target - foot;
    if (along.length() as f64) > 0.0001 {
        foot + along.normalized().scale((span * span - off * off).sqrt())
    } else {
        foot
    }
}

/// `GSHumanoidLegs.on_wall`'s bone work: each ankle onto `wall` (skeleton space), raised feet held
/// off it, blended by `weight`; `up` is the figure's up in skeleton space. The script checks the
/// weight and the wall and finds the legs; this poses them.
pub fn on_wall<P: Pose>(p: &mut P, legs: &[WallLeg; 2], wall: &Wall, up: Vec3, weight: f64) {
    let ankles = legs.each_ref().map(|l| global_pose(p, l.foot).origin);
    let heights: [f64; 2] = std::array::from_fn(|k| (ankles[k] - legs[k].foot_rest).dot(up) as f64);
    let pole = wall.normal + times(up, 0.5);
    for (k, leg) in legs.iter().enumerate() {
        let hip = global_pose(p, leg.upper).origin;
        let knee = global_pose(p, leg.lower);
        let span = ((knee.origin - hip).length() as f64
            + knee.basis_xform(leg.end_local).length() as f64)
            * 0.98;
        let target = within(wall, hip, wall.project(ankles[k]), span)
            + wall.normal.scale(heights[k].max(0.0)).scale(WALL_LIFT);
        let w = weight as f32;
        let a = ankles[k];
        let goal = Vec3::new(
            lerp_f32(a.x, target.x, w),
            lerp_f32(a.y, target.y, w),
            lerp_f32(a.z, target.z, w),
        );
        let reached = two_bone_to(p, leg.upper, leg.lower, leg.end_local, goal, pole, 1.0);
        let parent = p.parent(leg.foot);
        let at = if parent < 0 {
            reached
        } else {
            global_pose(p, parent).affine_inverse().xform(reached)
        };
        p.set_pos(leg.foot, at);
    }
}
