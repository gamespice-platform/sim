//! A drawn figure's leg and limb posing after its clip, as gs-player-char's `GSHumanoidLegs`
//! (`restore`, `apply`, `on_floor`, `hold_feet`, `step_off`, `resettle`), `GSHumanoidIk`
//! (`two_bone_to`, `global_pose`, `chain_pose`, `chain_rotation`) and `GSHumanoidTurnLimit`
//! (`step`, `restore`) do it, in the same order and precision: vector, transform and quaternion
//! maths in `f32` as the engine's, script scalars in `f64` as GDScript's, rounded to `f32` where
//! the script hands them to a vector.
//!
//! Drawing only: nothing a game simulates reads these bones. The skeleton is behind [`Pose`], so
//! the engine binding reads and writes the real one and the tests a plain list of bones.

use gs_sim_math::{
    Quat, Vec3, Xform, clampf, quat_mul, quat_normalized, quat_slerp, rotation_quaternion,
};

/// A skeleton's local poses as they are now.
pub trait Pose {
    /// The bone's parent, or -1.
    fn parent(&self, bone: i32) -> i32;
    /// The bone's local pose (`get_bone_pose`).
    fn pose(&self, bone: i32) -> Xform;
    /// The bone's local rotation (`get_bone_pose_rotation`).
    fn rot(&self, bone: i32) -> Quat;
    /// Sets the bone's local rotation (`set_bone_pose_rotation`).
    fn set_rot(&mut self, bone: i32, q: Quat);
    /// Sets the bone's local position (`set_bone_pose_position`).
    fn set_pos(&mut self, bone: i32, v: Vec3);
}

mod body;
mod carry;
pub use body::{WALL_LIFT, Wall, WallLeg, aim, on_wall, two_bone_under};
pub use carry::{CarryBody, CarryFrame, CarryStyle, carry};

pub(crate) const IDENTITY_Q: Quat = [0.0, 0.0, 0.0, 1.0];
pub(crate) const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const FORWARD: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// `CMP_EPSILON`.
const CMP_EPSILON: f32 = 0.00001;

/// `Quaternion.inverse()`: the conjugate.
pub fn quat_inverse(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

/// `Quaternion * Vector3`.
pub fn quat_xform(q: Quat, v: Vec3) -> Vec3 {
    let u = Vec3::new(q[0], q[1], q[2]);
    let uv = u.cross(v);
    let t = times(uv, q[3]) + u.cross(uv);
    v + times(t, 2.0)
}

/// `Quaternion(axis, angle)`.
pub fn quat_axis_angle(axis: Vec3, angle: f32) -> Quat {
    let d = axis.length();
    if d == 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let s = (angle * 0.5).sin() / d;
    [axis.x * s, axis.y * s, axis.z * s, (angle * 0.5).cos()]
}

/// `Quaternion(v0, v1)`: the shortest arc from one direction to the other.
pub fn quat_arc(v0: Vec3, v1: Vec3) -> Quat {
    let n0 = v0.normalized();
    let n1 = v1.normalized();
    let d = n0.dot(n1);
    // The engine's `ALMOST_ONE` for single precision.
    if d.abs() > 0.999_999_75 {
        if d >= 0.0 {
            return IDENTITY_Q;
        }
        // `Vector3.get_any_perpendicular()`: across RIGHT, or UP when x is not the smallest.
        let (ax, ay, az) = (n0.x.abs(), n0.y.abs(), n0.z.abs());
        let pick = if ax <= ay && ax <= az {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            UP
        };
        let axis = n0.cross(pick).normalized();
        return [axis.x, axis.y, axis.z, 0.0];
    }
    let c = n0.cross(n1);
    let s = ((1.0 + d) * 2.0).sqrt();
    let rs = 1.0 / s;
    [c.x * rs, c.y * rs, c.z * rs, s * 0.5]
}

/// `Quaternion.angle_to()`.
pub fn quat_angle_to(a: Quat, b: Quat) -> f32 {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    (d * d * 2.0 - 1.0).clamp(-1.0, 1.0).acos()
}

pub(crate) fn times(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// `Vector2(x, y)` from two script floats, then its length (both in `f32`).
fn flat_length(x: f64, y: f64) -> f32 {
    let (x, y) = (x as f32, y as f32);
    (x * x + y * y).sqrt()
}

/// `Vector3.limit_length()`.
fn limit_length(v: Vec3, len: f32) -> Vec3 {
    let l = v.length();
    if l > 0.0 && len < l {
        times(v.div_scalar(l as f64), len)
    } else {
        v
    }
}

/// `Vector3.move_toward()`.
fn move_toward(from: Vec3, to: Vec3, delta: f32) -> Vec3 {
    let vd = to - from;
    let len = vd.length();
    if len <= delta || len < CMP_EPSILON {
        to
    } else {
        from + times(vd.div_scalar(len as f64), delta)
    }
}

/// `smoothstep(from, to, x)` in `f64`.
fn smoothstep(from: f64, to: f64, x: f64) -> f64 {
    let s = ((x - from) / (to - from)).clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// `GSHumanoidIk.global_pose`: the bone's skeleton-space transform from the local poses now.
pub fn global_pose<P: Pose>(p: &P, bone: i32) -> Xform {
    // The same products in the same order as the script's walk: identity, then root first.
    if bone < 0 {
        return Xform::IDENTITY;
    }
    global_pose(p, p.parent(bone)).mul(&p.pose(bone))
}

/// `GSHumanoidIk.chain_to`: the bones from the root down to `bone`.
pub fn chain_to(parents: impl Fn(i32) -> i32, bone: i32) -> Vec<i32> {
    let mut chain = Vec::new();
    let mut b = bone;
    while b >= 0 {
        chain.push(b);
        b = parents(b);
    }
    chain.reverse();
    chain
}

/// `GSHumanoidIk.chain_pose`.
pub fn chain_pose<P: Pose>(p: &P, chain: &[i32]) -> Xform {
    chain
        .iter()
        .fold(Xform::IDENTITY, |out, &b| out.mul(&p.pose(b)))
}

/// `GSHumanoidIk.chain_rotation`: the bone's skeleton-space rotation from the local rotations now.
pub fn chain_rotation<P: Pose>(p: &P, bone: i32) -> Quat {
    let mut q = IDENTITY_Q;
    let mut b = bone;
    while b >= 0 {
        q = quat_mul(p.rot(b), q);
        b = p.parent(b);
    }
    q
}

pub(crate) fn basis_rotation(x: &Xform) -> Quat {
    rotation_quaternion(&x.rows)
}

/// `GSHumanoidIk.two_bone_to`: turns `upper` and `lower` so the point `end_local` (in `lower`'s
/// frame) reaches `target` (skeleton space), the knee toward `pole_dir`. Returns where it reached.
pub fn two_bone_to<P: Pose>(
    p: &mut P,
    upper: i32,
    lower: i32,
    end_local: Vec3,
    target: Vec3,
    pole_dir: Vec3,
    weight: f64,
) -> Vec3 {
    let top = global_pose(p, upper);
    let mid = global_pose(p, lower);
    let a = mid.origin - top.origin;
    let b = mid.xform(end_local) - mid.origin;
    let la = a.length() as f64;
    let lb = b.length() as f64;
    let to_target = target - top.origin;
    if la < 0.0001 || lb < 0.0001 || (to_target.length() as f64) < 0.0001 {
        return mid.xform(end_local);
    }
    let reach = clampf(
        to_target.length() as f64,
        (la - lb).abs() + 0.001,
        la + lb - 0.001,
    );
    let dir = to_target.normalized();
    let along = (la * la - lb * lb + reach * reach) / (2.0 * reach);
    let side = pole_dir - dir.scale(pole_dir.dot(dir) as f64);
    let side = if (side.length_squared() as f64) > 0.000001 {
        side.normalized()
    } else {
        UP
    };
    let knee =
        top.origin + dir.scale(along) + side.scale((la * la - along * along).max(0.0).sqrt());
    let q1 = quat_arc(a.normalized(), (knee - top.origin).normalized());
    let q2 = quat_arc(
        quat_xform(q1, b).normalized(),
        (top.origin + dir.scale(reach) - knee).normalized(),
    );
    let upper_rot = quat_normalized(quat_mul(q1, basis_rotation(&top)));
    let lower_rot = quat_normalized(quat_mul(quat_mul(q2, q1), basis_rotation(&mid)));
    let parent = basis_rotation(&global_pose(p, p.parent(upper)));
    let w = clampf(weight, 0.0, 1.0) as f32;
    let up_local = quat_normalized(quat_mul(quat_inverse(parent), upper_rot));
    let low_local = quat_normalized(quat_mul(quat_inverse(upper_rot), lower_rot));
    let r = quat_slerp(p.rot(upper), up_local, w);
    p.set_rot(upper, r);
    let r = quat_slerp(p.rot(lower), low_local, w);
    p.set_rot(lower, r);
    global_pose(p, lower).xform(end_local)
}

/// One leg [`Legs`] poses: the upper and lower bones, the foot's end in the lower bone's frame
/// (the rig's foot bones hang off the root), and the root-to-upper and root-to-lower chains.
#[derive(Clone, Debug, Default)]
pub struct FloorLeg {
    pub upper: i32,
    pub lower: i32,
    pub end_local: Vec3,
    pub chain_upper: Vec<i32>,
    pub chain_lower: Vec<i32>,
}

/// One figure's legs, as `GSHumanoidLegs` (the parts that pose bones).
#[derive(Clone, Debug)]
pub struct Legs {
    /// Up to two legs, left then right; set by the caller per skeleton.
    pub floor_legs: Vec<FloorLeg>,
    saved: Vec<(i32, Quat)>,
    holds: [Option<Vec3>; 2],
    held: [Vec3; 2],
    letting: [f64; 2],
    was: [Vec3; 2],
    off: [f64; 2],
    off_from: [Vec3; 2],
    stepped: [bool; 2],
    armed: bool,
}

impl Default for Legs {
    fn default() -> Self {
        let inf = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        Self {
            floor_legs: Vec::new(),
            saved: Vec::new(),
            holds: [None, None],
            held: [Vec3::default(); 2],
            letting: [0.0; 2],
            was: [inf; 2],
            off: [-1.0; 2],
            off_from: [Vec3::default(); 2],
            stepped: [false; 2],
            armed: false,
        }
    }
}

/// The legs a figure has (`GSHumanoidLegs.LEGS`).
pub const LEGS: usize = 2;

impl Legs {
    fn save(&mut self, bone: i32, q: Quat) {
        match self.saved.iter_mut().find(|(b, _)| *b == bone) {
            Some(s) => s.1 = q,
            None => self.saved.push((bone, q)),
        }
    }

    fn save_once(&mut self, bone: i32, q: Quat) {
        if !self.saved.iter().any(|(b, _)| *b == bone) {
            self.saved.push((bone, q));
        }
    }

    /// Puts back what the last frame's posing wrote.
    pub fn restore<P: Pose>(&mut self, p: &mut P) {
        for &(b, q) in &self.saved {
            p.set_rot(b, q);
        }
        self.saved.clear();
    }

    /// Turns `root` by `twist` about `up` (skeleton space) and `hips` back by as much.
    pub fn apply<P: Pose>(&mut self, p: &mut P, root: i32, hips: i32, up: Vec3, twist: f64) {
        let r = p.rot(root);
        self.save(root, r);
        let h = p.rot(hips);
        self.save(hips, h);
        let turn = quat_axis_angle(up.normalized(), twist as f32);
        let rp = chain_rotation(p, p.parent(root));
        let q = quat_mul(quat_mul(quat_mul(quat_inverse(rp), turn), rp), r);
        p.set_rot(root, quat_normalized(q));
        let hp = chain_rotation(p, p.parent(hips));
        let q = quat_mul(
            quat_mul(quat_mul(quat_inverse(hp), quat_inverse(turn)), hp),
            h,
        );
        p.set_rot(hips, quat_normalized(q));
    }

    /// Lifts each ankle the pose puts under its floor height back up to it.
    pub fn on_floor<P: Pose>(
        &mut self,
        p: &mut P,
        to_figure: &Xform,
        floor_y: &[f32],
        raised: f64,
    ) {
        if floor_y.len() < LEGS {
            return;
        }
        let to_skel = to_figure.affine_inverse();
        for (k, &floor) in floor_y.iter().enumerate().take(self.floor_legs.len()) {
            let leg = self.floor_legs[k].clone();
            let knee = chain_pose(p, &leg.chain_lower);
            let ankle = knee.xform(leg.end_local);
            let at = to_figure.xform(ankle);
            let lift = floor as f64 + raised - at.y as f64;
            if lift <= 0.0 {
                continue;
            }
            for bone in [leg.upper, leg.lower] {
                let q = p.rot(bone);
                self.save_once(bone, q);
            }
            let mut pole =
                knee.origin - (chain_pose(p, &leg.chain_upper).origin + ankle).scale(0.5);
            if (pole.length_squared() as f64) < 1e-8 {
                pole = to_skel.basis_xform(FORWARD);
            }
            let target = to_skel.xform(at + UP.scale(lift));
            two_bone_to(p, leg.upper, leg.lower, leg.end_local, target, pole, 1.0);
        }
    }

    /// Holds each planted ankle at the world point it came down on (`GSHumanoidLegs.hold_feet`).
    #[allow(clippy::too_many_arguments)]
    pub fn hold_feet<P: Pose>(
        &mut self,
        p: &mut P,
        to_world: &Xform,
        to_figure: &Xform,
        floor_y: &[f32],
        height: f64,
        settle: f64,
        reach: f64,
        release: f64,
        dt: f64,
    ) {
        if floor_y.len() < LEGS || dt <= 0.0 {
            return;
        }
        self.off = [-1.0; 2];
        self.stepped = [false; 2];
        self.armed = true;
        let to_skel = to_world.affine_inverse();
        let n = self.floor_legs.len();
        let mut knees = [Xform::IDENTITY; 2];
        let mut lows = [0f32; 2];
        for k in 0..n {
            knees[k] = chain_pose(p, &self.floor_legs[k].chain_lower);
            let y = to_figure
                .xform(knees[k].xform(self.floor_legs[k].end_local))
                .y;
            lows[k] = (y as f64 - floor_y[k] as f64) as f32;
        }
        for k in 0..n {
            let leg = self.floor_legs[k].clone();
            let knee = knees[k];
            let ankle_s = knee.xform(leg.end_local);
            let ankle = to_world.xform(ankle_s);
            let was = self.was[k];
            let flat = if finite(was) {
                flat_length(ankle.x as f64 - was.x as f64, ankle.z as f64 - was.z as f64) as f64
                    / dt
            } else {
                f64::INFINITY
            };
            self.was[k] = ankle;
            let other = if n > 1 { 1 - k } else { k };
            let support = other != k && lows[k] < lows[other] && self.holds[other].is_none();
            let planted =
                lows[k] as f64 <= height && (flat <= settle || self.holds[k].is_some() || support);
            let shift = if planted {
                let held = match self.holds[k] {
                    Some(h) => h,
                    None => {
                        let h = ankle + self.held[k];
                        self.holds[k] = Some(h);
                        h
                    }
                };
                let mut s = Vec3::new(
                    (held.x as f64 - ankle.x as f64) as f32,
                    0.0,
                    (held.z as f64 - ankle.z as f64) as f32,
                );
                if reach > 0.0 && (s.length() as f64) > reach {
                    s = limit_length(s, reach as f32);
                    self.holds[k] = Some(Vec3::new(
                        (ankle.x as f64 + s.x as f64) as f32,
                        held.y,
                        (ankle.z as f64 + s.z as f64) as f32,
                    ));
                }
                self.letting[k] = if release > 0.0 {
                    s.length() as f64 / release
                } else {
                    f64::INFINITY
                };
                s
            } else {
                self.holds[k] = None;
                move_toward(self.held[k], Vec3::default(), (self.letting[k] * dt) as f32)
            };
            self.held[k] = shift;
            if (shift.length_squared() as f64) < 1e-10 {
                continue;
            }
            self.pin(p, &leg, &knee, ankle_s, to_world, &to_skel, shift);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pin<P: Pose>(
        &mut self,
        p: &mut P,
        leg: &FloorLeg,
        knee: &Xform,
        ankle_s: Vec3,
        to_world: &Xform,
        to_skel: &Xform,
        shift: Vec3,
    ) {
        for bone in [leg.upper, leg.lower] {
            let q = p.rot(bone);
            self.save_once(bone, q);
        }
        let ankle = to_world.xform(ankle_s);
        let hip_s = chain_pose(p, &leg.chain_upper).origin;
        let mut pole = knee.origin - (hip_s + ankle_s).scale(0.5);
        if (pole.length_squared() as f64) < 1e-8 {
            pole = to_skel.basis_xform(FORWARD);
        }
        let knee_w = to_world.xform(knee.origin);
        let hip_w = to_world.xform(hip_s);
        let span = knee_w.distance_to(hip_w) as f64 + knee_w.distance_to(ankle) as f64;
        let target = within_leg(hip_w, ankle + shift, span);
        two_bone_to(
            p,
            leg.upper,
            leg.lower,
            leg.end_local,
            to_skel.xform(target),
            pole,
            1.0,
        );
    }

    /// Whether any foot is held or still stepping off its hold.
    pub fn holding(&self) -> bool {
        self.armed
    }

    /// Makes [`Legs::step_off`] follow through without a stride before it.
    pub fn arm(&mut self) {
        self.off = [-1.0; 2];
        self.stepped = [false; 2];
        self.armed = true;
    }

    /// The stop after a stride: each foot stays where it came down, then steps onto the clip's pose.
    #[allow(clippy::too_many_arguments)]
    pub fn step_off<P: Pose>(
        &mut self,
        p: &mut P,
        to_world: &Xform,
        to_figure: &Xform,
        floor_y: &[f32],
        height: f64,
        seconds: f64,
        lift: f64,
        dt: f64,
    ) {
        if dt <= 0.0 || floor_y.len() < LEGS {
            return;
        }
        let to_skel = to_world.affine_inverse();
        let mut pending = false;
        #[allow(clippy::needless_range_loop)]
        for k in 0..self.floor_legs.len() {
            if self.stepped[k] {
                continue;
            }
            let leg = self.floor_legs[k].clone();
            let knee = chain_pose(p, &leg.chain_lower);
            let ankle_s = knee.xform(leg.end_local);
            let ankle = to_world.xform(ankle_s);
            if self.off[k] < 0.0 {
                if let Some(h) = self.holds[k] {
                    self.off_from[k] = h;
                } else if to_figure.xform(ankle_s).y as f64 - floor_y[k] as f64 <= height {
                    self.off_from[k] = ankle;
                } else {
                    pending = true;
                    continue;
                }
                self.off[k] = 0.0;
                self.holds[k] = None;
                self.held[k] = Vec3::default();
            }
            self.off[k] = (self.off[k] + dt / seconds.max(0.001)).min(1.0);
            let t = self.off[k];
            if t >= 1.0 {
                self.off[k] = -1.0;
                self.stepped[k] = true;
                continue;
            }
            pending = true;
            let from = self.off_from[k];
            let along = smoothstep(0.0, 1.0, t);
            let shift = Vec3::new(
                ((from.x as f64 - ankle.x as f64) * (1.0 - along)) as f32,
                (lift * (std::f64::consts::PI * t).sin()) as f32,
                ((from.z as f64 - ankle.z as f64) * (1.0 - along)) as f32,
            );
            self.pin(p, &leg, &knee, ankle_s, to_world, &to_skel, shift);
        }
        if !pending {
            self.armed = false;
        }
    }

    /// Moves each hold to where its ankle is drawn now, across the ground.
    pub fn resettle<P: Pose>(&mut self, p: &P, to_world: &Xform) {
        for k in 0..self.floor_legs.len().min(LEGS) {
            let Some(held) = self.holds[k] else { continue };
            let leg = &self.floor_legs[k];
            let ankle = to_world.xform(chain_pose(p, &leg.chain_lower).xform(leg.end_local));
            self.holds[k] = Some(Vec3::new(ankle.x, held.y, ankle.z));
        }
    }

    /// Lets go of every hold at once.
    pub fn let_go(&mut self) {
        self.holds = [None, None];
        self.held = [Vec3::default(); 2];
        let inf = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        self.was = [inf; 2];
        self.off = [-1.0; 2];
        self.armed = false;
    }
}

/// `GSHumanoidLegs._within_leg`: `target` raised as far as the leg needs to reach it from `hip`.
fn within_leg(hip: Vec3, target: Vec3, span: f64) -> Vec3 {
    let most = span * 0.995;
    let fx = (target.x as f64 - hip.x as f64) as f32;
    let fz = (target.z as f64 - hip.z as f64) as f32;
    let flat = (fx * fx + fz * fz) as f64;
    if most <= 0.0 || flat >= most * most || ((target - hip).length_squared() as f64) <= most * most
    {
        return target;
    }
    let y = (target.y as f64).max(hip.y as f64 - (most * most - flat).sqrt());
    Vec3::new(target.x, y as f32, target.z)
}

/// A limb's turn rate cap, as `GSHumanoidTurnLimit` (bones resolved by the caller).
#[derive(Clone, Debug, Default)]
pub struct TurnLimit {
    /// The bones to turn, and per bone whether the arm rate governs it.
    pub bones: Vec<i32>,
    pub arm: Vec<bool>,
    drawn: Vec<Quat>,
    saved: Vec<(i32, Quat)>,
}

impl TurnLimit {
    /// New bones: forgets the last drawn pose.
    pub fn set_bones(&mut self, bones: Vec<i32>, arm: Vec<bool>) {
        self.bones = bones;
        self.arm = arm;
        self.drawn.clear();
    }

    /// Forgets the last drawn pose (`rate <= 0`, no skeleton).
    pub fn forget(&mut self) {
        self.drawn.clear();
    }

    /// Turns each bone no further than `rate * dt` (`arm_rate * dt` for an arm) from its last drawn rotation.
    pub fn step<P: Pose>(&mut self, p: &mut P, rate: f64, dt: f64, arm_rate: f64) {
        if rate <= 0.0 {
            self.drawn.clear();
            return;
        }
        let first = self.drawn.len() != self.bones.len();
        if first {
            self.drawn.resize(self.bones.len(), IDENTITY_Q);
        }
        let most = rate * dt.max(0.0);
        let arm_most = if arm_rate < 0.0 {
            most
        } else {
            arm_rate * dt.max(0.0)
        };
        for n in 0..self.bones.len() {
            let bone = self.bones[n];
            let mut now = p.rot(bone);
            if !first {
                let last = self.drawn[n];
                let angle = quat_angle_to(last, now) as f64;
                let cap = if self.arm[n] { arm_most } else { most };
                if cap > 0.0 && angle > cap {
                    match self.saved.iter_mut().find(|(b, _)| *b == bone) {
                        Some(s) => s.1 = now,
                        None => self.saved.push((bone, now)),
                    }
                    now = quat_slerp(last, now, (cap / angle) as f32);
                    p.set_rot(bone, now);
                }
            }
            self.drawn[n] = now;
        }
    }

    /// Puts back what [`TurnLimit::step`] turned.
    pub fn restore<P: Pose>(&mut self, p: Option<&mut P>) {
        if let Some(p) = p {
            for &(b, q) in &self.saved {
                p.set_rot(b, q);
            }
        }
        self.saved.clear();
    }

    /// Whether the last step held a bone back.
    pub fn turned(&self) -> bool {
        !self.saved.is_empty()
    }

    /// Forgets the last drawn pose and what was held back.
    pub fn clear(&mut self) {
        self.drawn.clear();
        self.saved.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chain of bones: root, upper leg, lower leg, each with a local pose (rotation and offset).
    struct Bones {
        parent: Vec<i32>,
        rot: Vec<Quat>,
        pos: Vec<Vec3>,
    }

    fn basis(q: Quat) -> [Vec3; 3] {
        let [x, y, z, w] = q;
        let (xx, yy, zz) = (x * x * 2.0, y * y * 2.0, z * z * 2.0);
        let (xy, xz, yz) = (x * y * 2.0, x * z * 2.0, y * z * 2.0);
        let (wx, wy, wz) = (w * x * 2.0, w * y * 2.0, w * z * 2.0);
        [
            Vec3::new(1.0 - (yy + zz), xy - wz, xz + wy),
            Vec3::new(xy + wz, 1.0 - (xx + zz), yz - wx),
            Vec3::new(xz - wy, yz + wx, 1.0 - (xx + yy)),
        ]
    }

    impl Pose for Bones {
        fn parent(&self, bone: i32) -> i32 {
            self.parent[bone as usize]
        }
        fn pose(&self, bone: i32) -> Xform {
            Xform {
                rows: basis(self.rot[bone as usize]),
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

    fn leg() -> Bones {
        Bones {
            parent: vec![-1, 0, 1],
            rot: vec![IDENTITY_Q; 3],
            pos: vec![
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.1, 0.0, 0.0),
                Vec3::new(0.0, -0.45, 0.0),
            ],
        }
    }

    fn floor_leg() -> FloorLeg {
        FloorLeg {
            upper: 1,
            lower: 2,
            end_local: Vec3::new(0.0, -0.45, 0.0),
            chain_upper: vec![0, 1],
            chain_lower: vec![0, 1, 2],
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn arc_turns_one_direction_onto_the_other() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert!(close(quat_xform(quat_arc(a, b), a), b));
        assert_eq!(quat_arc(a, a), IDENTITY_Q);
        let back = quat_arc(a, -a);
        assert!(close(quat_xform(back, a), -a));
    }

    #[test]
    fn two_bone_reaches_a_target_within_the_leg() {
        let mut p = leg();
        let target = Vec3::new(0.1, 0.3, 0.3);
        let reached = two_bone_to(
            &mut p,
            1,
            2,
            Vec3::new(0.0, -0.45, 0.0),
            target,
            Vec3::new(0.0, 0.0, 1.0),
            1.0,
        );
        assert!(close(reached, target), "{reached:?}");
        let again = global_pose(&p, 2).xform(Vec3::new(0.0, -0.45, 0.0));
        assert!(close(again, target));
    }

    #[test]
    fn on_floor_lifts_a_sunk_ankle_and_restore_puts_it_back() {
        let mut p = leg();
        let mut legs = Legs {
            floor_legs: vec![floor_leg()],
            ..Legs::default()
        };
        // The ankle is at y 0.1; the floor at 0.2.
        legs.on_floor(&mut p, &Xform::IDENTITY, &[0.2, 0.2], 0.0);
        let ankle = global_pose(&p, 2).xform(Vec3::new(0.0, -0.45, 0.0));
        assert!((ankle.y - 0.2).abs() < 1e-4, "{ankle:?}");
        legs.restore(&mut p);
        assert_eq!(p.rot, vec![IDENTITY_Q; 3]);
    }

    #[test]
    fn apply_turns_the_root_and_the_hips_back() {
        let mut p = leg();
        let mut legs = Legs::default();
        legs.apply(&mut p, 0, 1, UP, 0.5);
        let world_hips = chain_rotation(&p, 1);
        assert!(quat_angle_to(world_hips, IDENTITY_Q) < 1e-5);
        assert!((quat_angle_to(p.rot(0), IDENTITY_Q) - 0.5).abs() < 1e-5);
        legs.restore(&mut p);
        assert_eq!(p.rot, vec![IDENTITY_Q; 3]);
    }

    #[test]
    fn a_held_foot_stays_put_while_the_body_moves_on() {
        let mut p = leg();
        let mut legs = Legs {
            floor_legs: vec![floor_leg()],
            ..Legs::default()
        };
        let mut world = Xform::IDENTITY;
        let dt = 1.0 / 60.0;
        // The first step only sees where the ankle is; the second, standing still, plants it.
        for _ in 0..2 {
            legs.hold_feet(
                &mut p,
                &world,
                &Xform::IDENTITY,
                &[0.1, 0.1],
                0.05,
                1.0,
                0.5,
                0.2,
                dt,
            );
            legs.restore(&mut p);
        }
        assert!(legs.holding());
        world.origin = Vec3::new(0.0, 0.0, 0.05);
        legs.hold_feet(
            &mut p,
            &world,
            &Xform::IDENTITY,
            &[0.1, 0.1],
            0.05,
            1.0,
            0.5,
            0.2,
            dt,
        );
        let ankle = world.xform(global_pose(&p, 2).xform(Vec3::new(0.0, -0.45, 0.0)));
        // Short of the hold by no more than the straightened leg cannot reach (the clip has it 0.05 on).
        assert!(ankle.z.abs() < 0.01, "{ankle:?}");
        legs.let_go();
        assert!(!legs.holding());
    }

    #[test]
    fn turn_limit_caps_a_fast_turn_and_restores_it() {
        let mut p = leg();
        let mut limit = TurnLimit::default();
        limit.set_bones(vec![1, 2], vec![false, true]);
        limit.step(&mut p, 1.0, 0.1, -1.0);
        let fast = quat_axis_angle(Vec3::new(1.0, 0.0, 0.0), 1.0);
        p.set_rot(1, fast);
        limit.step(&mut p, 1.0, 0.1, -1.0);
        assert!(limit.turned());
        assert!((quat_angle_to(p.rot(1), IDENTITY_Q) - 0.1).abs() < 1e-4);
        limit.restore(Some(&mut p));
        assert_eq!(p.rot(1), fast);
        assert!(!limit.turned());
    }

    /// A root with two legs (upper, lower, foot each), standing at the origin facing +z.
    fn two_legs() -> Bones {
        let leg = |x: f32| {
            [
                Vec3::new(x, 0.0, 0.0),
                Vec3::new(0.0, -0.45, 0.0),
                Vec3::new(0.0, -0.45, 0.0),
            ]
        };
        let mut pos = vec![Vec3::new(0.0, 1.0, 0.0)];
        pos.extend(leg(0.1));
        pos.extend(leg(-0.1));
        Bones {
            parent: vec![-1, 0, 1, 2, 0, 4, 5],
            rot: vec![IDENTITY_Q; 7],
            pos,
        }
    }

    fn wall_leg(upper: i32, x: f32) -> WallLeg {
        WallLeg {
            upper,
            lower: upper + 1,
            foot: upper + 2,
            foot_rest: Vec3::new(x, 0.1, 0.0),
            end_local: Vec3::new(0.0, -0.45, 0.0),
        }
    }

    #[test]
    fn aim_turns_each_bone_its_share_about_figure_right() {
        let mut p = leg();
        let plan = [1.0, 0.0, 0.5, 0.0, 2.0, 1.0, 0.25, 1.0];
        let id = [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        aim(&mut p, &plan, &id, 0.8, -0.2);
        let half = |a: f32| [(a * 0.5).sin(), 0.0, 0.0, (a * 0.5).cos()];
        let near = |q: Quat, r: Quat| q.iter().zip(r).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(near(p.rot(1), half(0.4)), "{:?}", p.rot(1));
        assert!(near(p.rot(2), half(0.0)), "{:?}", p.rot(2));
        assert_eq!(p.rot(0), IDENTITY_Q);
    }

    #[test]
    fn aim_keeps_the_clip_pose_when_the_figure_basis_collapses() {
        let mut p = leg();
        aim(
            &mut p,
            &[1.0, 0.0, 1.0, 0.0],
            &[Vec3::default(); 3],
            0.8,
            0.0,
        );
        assert_eq!(p.rot(1), IDENTITY_Q);
    }

    #[test]
    fn two_bone_under_puts_the_end_on_a_reachable_target() {
        let mut p = leg();
        p.pos.push(Vec3::new(0.0, -0.45, 0.0));
        p.parent.push(2);
        p.rot.push(IDENTITY_Q);
        let target = Vec3::new(0.4, 0.6, 0.3);
        let under = global_pose(&p, 0);
        two_bone_under(
            &mut p,
            [1, 2, 3],
            &under,
            target,
            Vec3::new(0.0, 0.0, 1.0),
            1.0,
        );
        let end = global_pose(&p, 3).origin;
        assert!((end - target).length() < 1e-4, "{end:?}");
    }

    #[test]
    fn on_wall_sets_unraised_feet_on_the_wall_within_reach() {
        let mut p = two_legs();
        let legs = [wall_leg(1, 0.1), wall_leg(4, -0.1)];
        let wall = Wall {
            normal: Vec3::new(0.0, 0.0, -1.0),
            d: -0.2,
        };
        on_wall(&mut p, &legs, &wall, UP, 1.0);
        for leg in &legs {
            let foot = global_pose(&p, leg.foot).origin;
            let hip = global_pose(&p, leg.upper).origin;
            assert!(wall.distance_to(foot).abs() < 1e-4, "{foot:?}");
            assert!((foot - hip).length() <= 0.882 + 1e-4, "{foot:?}");
        }
    }
}
