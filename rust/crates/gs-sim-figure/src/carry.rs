//! `GSHumanoidCarryPose.apply`: the gun arm of a figure as others see it, put where the carry
//! stance says, eased between ready, sprint, aim, reload and swap, swaying with the gait, then the
//! hand's fingers turned with it and closed round the grip, and the support hand's point worked
//! out. In the script's order and precision like the rest of the crate.

use crate::{
    IDENTITY_Q, Pose, basis_rotation, chain_pose, quat_axis_angle, quat_inverse, quat_xform,
    smoothstep,
};
use gs_sim_math::{Quat, Vec3, Xform, clampf, lerp_f32, quat_mul, quat_normalized, quat_slerp};
use std::f64::consts::{PI, TAU};

/// `GSHumanoidCarryPose.POLE`: where the gun arm's elbow points, in the figure's frame.
const POLE: Vec3 = Vec3::new(0.5, -1.0, 0.25);
/// `GSHumanoidCarryPose.CHEST_PITCH`.
const CHEST_PITCH: f64 = 0.22;
/// `GSHumanoidCarryPose.BREATH_SECONDS`, `BREATH_SWAY`, `BREATH_TURN`.
const BREATH_SECONDS: f64 = 4.0;
const BREATH_SWAY: f64 = 0.004;
const BREATH_TURN: f64 = 0.35;
/// `GSHumanoidCarryPose.RELOAD_PATH`, `BELT`, `WELL`.
const RELOAD_PATH: f64 = 1.1;
const BELT: Vec3 = Vec3::new(-0.12, -0.26, -0.06);
const WELL: Vec3 = Vec3::new(0.0, -0.10, -0.08);
const RIGHT: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const BACK: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// What `GSHumanoidCarryPose._measure` finds once per body.
#[derive(Clone, Debug, Default)]
pub struct CarryBody {
    /// The bones from the root to the chest.
    pub chest_chain: Vec<i32>,
    /// The gun arm's shoulder bone (the upper arm's parent, a child of the chest).
    pub shoulder: i32,
    /// The gun's frame in the hand bone's, and its inverse (the script's own `affine_inverse`).
    pub hand_frame: Xform,
    pub hand_frame_inv: Xform,
    /// The chest's skeleton-space pose at the held-gun pose.
    pub chest_ref: Xform,
    /// The hand bone's rest rotation, the other finger roots and their rest rotations.
    pub hand_rest: Quat,
    pub fingers: Vec<i32>,
    pub finger_rest: Vec<Quat>,
    /// Every finger segment past the roots and its closed pose.
    pub grip_bones: Vec<i32>,
    pub grip_pose: Vec<Quat>,
}

/// A `GSHumanoidCarry` as the carry reads it, with the stance turns already quaternions
/// (`GSHumanoidCarryPose._turns`: ready, sprint, aim, reload, swap).
#[derive(Clone, Debug, Default)]
pub struct CarryStyle {
    /// Grips: ready, sprint, aim, reload, swap.
    pub grips: [Vec3; 5],
    pub turns: [Quat; 5],
    pub follow: f64,
    pub walk_sway: Vec3,
    pub run_sway: Vec3,
    pub kick: f64,
    pub kick_climb: f64,
    pub bolt: f64,
    pub bolt_back: f64,
    pub bolt_roll: f64,
    pub support: Vec3,
}

/// One frame's state and arguments: the script's fields and `apply`'s.
#[derive(Clone, Copy, Debug, Default)]
pub struct CarryFrame {
    /// The weight drawn (`weight * (1 - under)`), already above the script's 0.001 floor.
    pub w: f64,
    pub aim: f64,
    pub reload: f64,
    pub swap: f64,
    pub sprint: f64,
    /// The stride fraction, negative for none.
    pub phase: f64,
    pub pitch: f64,
    pub size: f64,
    pub speed: f64,
    pub kick: f64,
    pub bolt: f64,
    pub clock: f64,
    pub reload_time: f64,
    /// Whether the other hand takes the gun (`two_handed and explicit and swap < 0.5`).
    pub support: bool,
}

impl CarryFrame {
    /// Number of floats in [`CarryFrame::from_slice`]'s layout.
    pub const LEN: usize = 14;

    /// The frame from the script's packed layout: the fields in declaration order, `support` last
    /// as 0 or 1. `None` when the slice is short.
    pub fn from_slice(v: &[f64]) -> Option<Self> {
        let v: &[f64; Self::LEN] = v.get(..Self::LEN)?.try_into().ok()?;
        Some(Self {
            w: v[0],
            aim: v[1],
            reload: v[2],
            swap: v[3],
            sprint: v[4],
            phase: v[5],
            pitch: v[6],
            size: v[7],
            speed: v[8],
            kick: v[9],
            bolt: v[10],
            clock: v[11],
            reload_time: v[12],
            support: v[13] != 0.0,
        })
    }
}

/// `Basis(Quaternion)`: the engine's `set_quaternion`.
pub(crate) fn basis_from_quat(q: Quat) -> [Vec3; 3] {
    let [x, y, z, w] = q;
    let d = x * x + y * y + z * z + w * w;
    let s = 2.0 / d;
    let (xs, ys, zs) = (x * s, y * s, z * s);
    let (wx, wy, wz) = (w * xs, w * ys, w * zs);
    let (xx, xy, xz) = (x * xs, x * ys, x * zs);
    let (yy, yz, zz) = (y * ys, y * zs, z * zs);
    [
        Vec3::new(1.0 - (yy + zz), xy - wz, xz + wy),
        Vec3::new(xy + wz, 1.0 - (xx + zz), yz - wx),
        Vec3::new(xz - wy, yz + wx, 1.0 - (xx + yy)),
    ]
}

/// `Quaternion.from_euler` (YXZ).
pub(crate) fn quat_from_euler(e: Vec3) -> Quat {
    let (h1, h2, h3) = (e.y * 0.5, e.x * 0.5, e.z * 0.5);
    let (c1, s1) = (h1.cos(), h1.sin());
    let (c2, s2) = (h2.cos(), h2.sin());
    let (c3, s3) = (h3.cos(), h3.sin());
    [
        s1 * c2 * s3 + c1 * s2 * c3,
        s1 * c2 * c3 - c1 * s2 * s3,
        -s1 * s2 * c3 + c1 * c2 * s3,
        s1 * s2 * s3 + c1 * c2 * c3,
    ]
}

/// `Vector3.lerp` with the script's `f64` weight rounded to `f32`.
fn vlerp(a: Vec3, b: Vec3, w: f64) -> Vec3 {
    let w = w as f32;
    Vec3::new(
        lerp_f32(a.x, b.x, w),
        lerp_f32(a.y, b.y, w),
        lerp_f32(a.z, b.z, w),
    )
}

/// `lerpf` in `f64`.
fn lerpf(from: f64, to: f64, w: f64) -> f64 {
    from + (to - from) * w
}

/// `Vector3(x, y, z)` from three script floats.
fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x as f32, y as f32, z as f32)
}

/// `GSHumanoidCarryPose.apply` past its checks and lookups: poses the gun arm (`bones`: upper,
/// lower, hand, each keyed by the clip; `keyed`: whether the clip keys each finger root) and
/// returns where the other hand goes in skeleton space, `None` for nowhere.
pub fn carry<P: Pose>(
    p: &mut P,
    body: &CarryBody,
    s: &CarryStyle,
    bones: [i32; 3],
    keyed: &[u8],
    to_figure: &Xform,
    f: &CarryFrame,
) -> Option<Vec3> {
    let w = f.w;
    let size = f.size;
    // The stance, blended: sprint over ready, the aim over both, reload and swap over all.
    let mut grip = vlerp(s.grips[0], s.grips[1], f.sprint);
    let mut turn = quat_slerp(s.turns[0], s.turns[1], f.sprint as f32);
    let eased = smoothstep(0.0, 1.0, f.aim);
    grip = vlerp(grip, s.grips[2], eased);
    turn = quat_slerp(turn, s.turns[2], eased as f32);
    grip = vlerp(grip, s.grips[3], f.reload);
    turn = quat_slerp(turn, s.turns[3], f.reload as f32);
    grip = vlerp(grip, s.grips[4], f.swap);
    turn = quat_slerp(turn, s.turns[4], f.swap as f32);
    // Ride the chest as the clip moves it.
    let chest_global = chain_pose(p, &body.chest_chain);
    let shoulder_global = chest_global.mul(&p.pose(body.shoulder));
    let chest = to_figure.mul(&chest_global);
    let chest_ref = to_figure.mul(&body.chest_ref);
    let lean = quat_mul(
        basis_rotation(&chest),
        quat_inverse(basis_rotation(&chest_ref)),
    );
    let follow_share = lerpf(s.follow, 0.25, f.aim);
    let follow = quat_slerp(IDENTITY_Q, lean, follow_share as f32);
    grip = chest.origin + quat_xform(follow, grip.scale(size) - chest_ref.origin);
    turn = quat_mul(follow, turn);
    // Gait sway.
    if f.phase >= 0.0 {
        let amount =
            clampf(f.speed / 1.2, 0.0, 1.0) * (1.0 - 0.75 * f.aim) * (1.0 - 0.5 * f.reload);
        let amp = vlerp(
            s.walk_sway,
            s.run_sway,
            clampf((f.speed - 2.0) / 3.0, 0.0, 1.0),
        )
        .scale(amount);
        let a = TAU * f.phase;
        grip = grip + v3(amp.y as f64 * a.sin(), amp.x as f64 * (2.0 * a).sin(), 0.0).scale(size);
        let deg = amp.z as f64 * (PI / 180.0);
        let e = v3(
            0.4 * deg * (2.0 * a).sin(),
            deg * a.sin(),
            -0.6 * deg * a.sin(),
        );
        turn = quat_mul(quat_from_euler(e), turn);
    }
    // A held aim breathes.
    if f.aim > 0.0 {
        let b = TAU * f.clock / BREATH_SECONDS;
        grip = grip + v3((b * 0.5).sin() * 0.5, b.sin(), 0.0).scale(BREATH_SWAY * size * eased);
        let r = BREATH_TURN * (PI / 180.0);
        let e = v3(r * b.sin(), r * 0.6 * (b * 0.5).sin(), 0.0).scale(eased);
        turn = quat_mul(quat_from_euler(e), turn);
    }
    // The view's pitch, about the shoulder.
    let shoulder = to_figure.xform(shoulder_global.mul(&p.pose(bones[0])).origin);
    let share = lerpf(0.5, 1.0, f.aim) * (1.0 - 0.7 * f.sprint * (1.0 - f.aim))
        - CHEST_PITCH * follow_share;
    let tilt = quat_axis_angle(RIGHT, (f.pitch * share) as f32);
    grip = shoulder + quat_xform(tilt, grip - shoulder);
    turn = quat_mul(tilt, turn);
    // The kick.
    if f.kick > 0.0 {
        grip = grip + quat_xform(turn, v3(0.0, 0.0, s.kick * f.kick * size));
        turn = quat_mul(
            turn,
            quat_axis_angle(RIGHT, (s.kick_climb * (PI / 180.0) * f.kick) as f32),
        );
    }
    // The action worked after a shot.
    if f.bolt > 0.0 && s.bolt > 0.0 {
        let t = 1.0 - f.bolt / s.bolt;
        let work = (PI * smoothstep(0.1, 1.0, t)).sin();
        grip = grip + quat_xform(turn, v3(0.0, 0.0, s.bolt_back * work * size));
        turn = quat_mul(
            turn,
            quat_axis_angle(BACK, (s.bolt_roll * (PI / 180.0) * work) as f32),
        );
    }
    // The hand bone that puts the gun's frame there, in skeleton space.
    let to_skel = to_figure.affine_inverse();
    let hand = to_skel
        .mul(&Xform {
            rows: basis_from_quat(turn),
            origin: grip,
        })
        .mul(&body.hand_frame_inv);
    let pole = Xform {
        rows: to_figure.rows,
        origin: Vec3::default(),
    }
    .affine_inverse()
    .basis_xform(POLE);
    crate::two_bone_under(p, bones, &shoulder_global, hand.origin, pole, w);
    let upper_global = shoulder_global.mul(&p.pose(bones[0]));
    let lower_global = upper_global.mul(&p.pose(bones[1]));
    let lower = basis_rotation(&lower_global);
    let wanted = quat_normalized(quat_mul(quat_inverse(lower), basis_rotation(&hand)));
    let was = p.rot(bones[2]);
    let now = quat_slerp(was, wanted, w as f32);
    p.set_rot(bones[2], now);
    turn_fingers(p, body, keyed, was, now);
    for (&bone, &q) in body.grip_bones.iter().zip(&body.grip_pose) {
        let r = quat_slerp(p.rot(bone), q, w as f32);
        p.set_rot(bone, r);
    }
    if !f.support {
        return None;
    }
    let hand_global = lower_global.mul(&p.pose(bones[2]));
    Some(support(body, s, f, &hand_global, &to_skel, chest.origin))
}

/// `GSHumanoidCarryPose._turn_fingers`.
fn turn_fingers<P: Pose>(p: &mut P, body: &CarryBody, keyed: &[u8], was: Quat, now: Quat) {
    if body.fingers.is_empty() {
        return;
    }
    let delta = quat_normalized(quat_mul(now, quat_inverse(was)));
    let base = quat_mul(now, quat_inverse(body.hand_rest));
    for (i, (&bone, &rest)) in body.fingers.iter().zip(&body.finger_rest).enumerate() {
        let q = if keyed.get(i).copied() == Some(1) {
            quat_mul(delta, p.rot(bone))
        } else {
            quat_mul(base, rest)
        };
        p.set_rot(bone, quat_normalized(q));
    }
}

/// `GSHumanoidCarryPose._support`.
fn support(
    body: &CarryBody,
    s: &CarryStyle,
    f: &CarryFrame,
    hand_global: &Xform,
    to_skel: &Xform,
    chest: Vec3,
) -> Vec3 {
    let gun = hand_global.mul(&body.hand_frame);
    let fore = gun.xform(s.support.scale(f.size));
    if f.reload <= 0.0 {
        return fore;
    }
    let well = gun.xform(WELL.scale(f.size));
    let belt = to_skel.xform(chest + BELT.scale(f.size));
    let t = f.reload_time / RELOAD_PATH;
    let path = if t < 0.15 {
        vlerp(fore, well, smoothstep(0.0, 0.15, t))
    } else if t < 0.4 {
        vlerp(well, belt, smoothstep(0.15, 0.4, t))
    } else if t < 0.7 {
        vlerp(belt, well, smoothstep(0.4, 0.7, t))
    } else if t < 0.85 {
        well
    } else if t < 1.0 {
        vlerp(well, fore, smoothstep(0.85, 1.0, t))
    } else {
        fore
    };
    vlerp(fore, path, f.reload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_pose;

    /// A torso, chest, shoulder and a right arm (upper, lower, hand) and one finger root.
    struct Arm {
        parent: Vec<i32>,
        rot: Vec<Quat>,
        pos: Vec<Vec3>,
    }

    impl Pose for Arm {
        fn parent(&self, bone: i32) -> i32 {
            self.parent[bone as usize]
        }
        fn pose(&self, bone: i32) -> Xform {
            Xform {
                rows: basis_from_quat(self.rot[bone as usize]),
                origin: self.pos[bone as usize],
            }
        }
        fn rot(&self, bone: i32) -> Quat {
            self.rot[bone as usize]
        }
        fn set_rot(&mut self, bone: i32, q: Quat) {
            self.rot[bone as usize] = q;
        }
        fn set_pos(&mut self, bone: i32, v: Vec3) {
            self.pos[bone as usize] = v;
        }
    }

    fn arm() -> Arm {
        Arm {
            parent: vec![-1, 0, 1, 2, 3, 4, 3],
            rot: vec![IDENTITY_Q; 7],
            pos: vec![
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.4, 0.0),
                Vec3::new(0.05, 0.05, 0.0),
                Vec3::new(0.12, 0.0, 0.0),
                Vec3::new(0.0, -0.28, 0.0),
                Vec3::new(0.0, -0.26, 0.0),
                Vec3::new(0.0, -0.27, 0.02),
            ],
        }
    }

    fn body(p: &Arm) -> CarryBody {
        let chest_ref = global_pose(p, 1);
        CarryBody {
            chest_chain: vec![0, 1],
            shoulder: 2,
            hand_frame: Xform::IDENTITY,
            hand_frame_inv: Xform::IDENTITY,
            chest_ref,
            hand_rest: IDENTITY_Q,
            fingers: vec![6],
            finger_rest: vec![IDENTITY_Q],
            grip_bones: vec![],
            grip_pose: vec![],
        }
    }

    fn style() -> CarryStyle {
        let g = Vec3::new(0.15, 1.2, -0.3);
        CarryStyle {
            grips: [g; 5],
            turns: [IDENTITY_Q; 5],
            follow: 0.5,
            support: Vec3::new(0.0, 0.0, -0.3),
            ..Default::default()
        }
    }

    #[test]
    fn basis_from_quat_turns_like_the_quaternion() {
        let q = quat_normalized([0.2, -0.4, 0.1, 0.9]);
        let v = Vec3::new(0.3, -1.2, 0.7);
        let b = Xform {
            rows: basis_from_quat(q),
            origin: Vec3::default(),
        };
        assert!((b.basis_xform(v) - quat_xform(q, v)).length() < 1e-6);
    }

    #[test]
    fn from_euler_is_yaw_after_pitch_after_roll() {
        let e = Vec3::new(0.3, -0.5, 0.2);
        let want = quat_mul(
            quat_mul(
                quat_axis_angle(Vec3::new(0.0, 1.0, 0.0), e.y),
                quat_axis_angle(RIGHT, e.x),
            ),
            quat_axis_angle(BACK, e.z),
        );
        let got = quat_from_euler(e);
        for i in 0..4 {
            assert!((got[i] - want[i]).abs() < 1e-6, "{got:?} {want:?}");
        }
    }

    #[test]
    fn carry_puts_the_hand_on_the_grip_and_the_support_on_the_foregrip() {
        let mut p = arm();
        let b = body(&p);
        let s = style();
        let f = CarryFrame {
            w: 1.0,
            size: 1.0,
            phase: -1.0,
            support: true,
            ..Default::default()
        };
        let got = carry(&mut p, &b, &s, [3, 4, 5], &[0], &Xform::IDENTITY, &f);
        let hand = global_pose(&p, 5);
        assert!(
            (hand.origin - s.grips[0]).length() < 1e-4,
            "{:?}",
            hand.origin
        );
        let fore = got.expect("support");
        assert!((fore - hand.xform(s.support)).length() < 1e-6);
        // The unkeyed finger root follows the hand from its rest.
        assert_eq!(p.rot[6], quat_normalized(p.rot[5]));
    }

    #[test]
    fn frame_from_slice_needs_every_field() {
        let mut v = vec![0.0; CarryFrame::LEN];
        assert!(CarryFrame::from_slice(&v[..CarryFrame::LEN - 1]).is_none());
        v[13] = 1.0;
        v[7] = 1.5;
        let f = CarryFrame::from_slice(&v).expect("frame");
        assert!(f.support);
        assert_eq!(f.size, 1.5);
    }
}
