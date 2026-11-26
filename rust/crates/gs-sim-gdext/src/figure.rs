//! `GSSimLegs` and `GSSimTurnLimit`: a drawn figure's leg posing and limb turn cap after its clip.
//!
//! The twins of gs-player-char's `GSHumanoidLegs` (`restore`, `apply`, `on_floor`, `hold_feet`,
//! `step_off`, `resettle`) and `GSHumanoidTurnLimit` (`step`, `restore`); the kernels are
//! `gs_sim_figure`'s. The script keeps its own fields (the twist, what it last applied) and hands
//! everything in as arguments; this side keeps only what the script kept between calls about the
//! bones (the holds, the saved rotations, the last drawn pose). Drawing only.

use godot::classes::Skeleton3D;
use godot::prelude::*;
use gs_sim_figure::{
    CarryBody, CarryFrame, CarryStyle, FloorLeg, Legs, Pose, TurnLimit, Wall, WallLeg,
};
use gs_sim_math::{Quat, Vec3, Xform};

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Quaternions packed four floats each (x, y, z, w), as the script hands them in.
fn quats(v: &PackedFloat32Array) -> Vec<Quat> {
    v.as_slice().as_chunks::<4>().0.to_vec()
}

fn xf(t: &Transform3D) -> Xform {
    Xform {
        rows: [
            v3(t.basis.rows[0]),
            v3(t.basis.rows[1]),
            v3(t.basis.rows[2]),
        ],
        origin: v3(t.origin),
    }
}

/// Which skeleton a cache was worked out on: its instance id and bone count, as the script keys it.
#[derive(Default, PartialEq, Clone, Copy)]
struct Seen(i64, i32);

impl Seen {
    fn of(skeleton: &Gd<Skeleton3D>) -> Self {
        Self(skeleton.instance_id().to_i64(), skeleton.get_bone_count())
    }
}

/// One skeleton seen through [`Pose`] for one call: parents from the per-skeleton cache, local
/// poses and rotations read once and kept until the call writes that bone (a write is read back
/// from the engine, so a pose is always the engine's own composition).
struct Skel<'a> {
    gd: &'a mut Gd<Skeleton3D>,
    parents: &'a [i32],
    poses: &'a mut Vec<Option<Xform>>,
    rots: &'a mut Vec<Option<Quat>>,
}

impl<'a> Skel<'a> {
    fn new(
        gd: &'a mut Gd<Skeleton3D>,
        parents: &'a [i32],
        poses: &'a mut Vec<Option<Xform>>,
        rots: &'a mut Vec<Option<Quat>>,
    ) -> Self {
        poses.clear();
        poses.resize(parents.len(), None);
        rots.clear();
        rots.resize(parents.len(), None);
        Self {
            gd,
            parents,
            poses,
            rots,
        }
    }
}

impl Pose for Skel<'_> {
    fn parent(&self, bone: i32) -> i32 {
        self.parents.get(bone as usize).copied().unwrap_or(-1)
    }

    fn pose(&self, bone: i32) -> Xform {
        // Interior caching would need `&mut self`; the poses read per call are few, so read
        // through the cache when it has the bone and from the engine otherwise.
        match self.poses.get(bone as usize).copied().flatten() {
            Some(x) => x,
            None => xf(&self.gd.get_bone_pose(bone)),
        }
    }

    fn rot(&self, bone: i32) -> Quat {
        match self.rots.get(bone as usize).copied().flatten() {
            Some(q) => q,
            None => {
                let q = self.gd.get_bone_pose_rotation(bone);
                [q.x, q.y, q.z, q.w]
            }
        }
    }

    fn set_rot(&mut self, bone: i32, q: Quat) {
        self.gd
            .set_bone_pose_rotation(bone, Quaternion::new(q[0], q[1], q[2], q[3]));
        if let Some(slot) = self.rots.get_mut(bone as usize) {
            *slot = Some(q);
        }
        if let Some(slot) = self.poses.get_mut(bone as usize) {
            *slot = None;
        }
    }

    fn set_pos(&mut self, bone: i32, v: Vec3) {
        self.gd
            .set_bone_pose_position(bone, Vector3::new(v.x, v.y, v.z));
        if let Some(slot) = self.poses.get_mut(bone as usize) {
            *slot = None;
        }
    }
}

impl Skel<'_> {
    /// Reads every bone's local pose into the cache once (a call that walks chains many times).
    fn warm(&mut self) {
        for b in 0..self.parents.len() {
            self.poses[b] = Some(xf(&self.gd.get_bone_pose(b as i32)));
        }
    }
}

fn parents_of(skeleton: &Gd<Skeleton3D>) -> Vec<i32> {
    (0..skeleton.get_bone_count())
        .map(|b| skeleton.get_bone_parent(b))
        .collect()
}

/// The legs' bones on a skeleton, as `GSHumanoidLegs._floor_bones`: up to the first leg with a bone missing.
fn floor_bones(skeleton: &Gd<Skeleton3D>, parents: &[i32]) -> Vec<FloorLeg> {
    const LEGS: [[&str; 3]; 2] = [
        ["UpperLeg.L", "LowerLeg.L", "Foot.L"],
        ["UpperLeg.R", "LowerLeg.R", "Foot.R"],
    ];
    let mut out = Vec::new();
    let parent = |b: i32| parents.get(b as usize).copied().unwrap_or(-1);
    for names in LEGS {
        let upper = skeleton.find_bone(names[0]);
        let lower = skeleton.find_bone(names[1]);
        let foot = skeleton.find_bone(names[2]);
        if upper < 0 || lower < 0 || foot < 0 {
            break;
        }
        let end_local = xf(&skeleton.get_bone_global_rest(lower))
            .affine_inverse()
            .xform(v3(skeleton.get_bone_global_rest(foot).origin));
        out.push(FloorLeg {
            upper,
            lower,
            end_local,
            chain_upper: gs_sim_figure::chain_to(parent, upper),
            chain_lower: gs_sim_figure::chain_to(parent, lower),
        });
    }
    out
}

/// The legs `GSHumanoidLegs.on_wall` poses, `None` when a bone is missing (the script then returns).
fn wall_bones(skeleton: &Gd<Skeleton3D>) -> Option<[WallLeg; 2]> {
    let leg = |names: [&str; 3]| {
        let [upper, lower, foot] = names.map(|n| skeleton.find_bone(n));
        if upper < 0 || lower < 0 || foot < 0 {
            return None;
        }
        let foot_rest = v3(skeleton.get_bone_global_rest(foot).origin);
        let end_local = xf(&skeleton.get_bone_global_rest(lower))
            .affine_inverse()
            .xform(foot_rest);
        Some(WallLeg {
            upper,
            lower,
            foot,
            foot_rest,
            end_local,
        })
    };
    Some([
        leg(["UpperLeg.L", "LowerLeg.L", "Foot.L"])?,
        leg(["UpperLeg.R", "LowerLeg.R", "Foot.R"])?,
    ])
}

/// One figure's `GSHumanoidLegs` bone work. The script calls it in the same places with the same
/// arguments; null skeletons and the zero twist stay on the script's side.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimLegs {
    legs: Legs,
    floor_seen: Seen,
    turn_seen: Seen,
    root: i32,
    hips: i32,
    parents: Vec<i32>,
    parents_seen: Seen,
    poses: Vec<Option<Xform>>,
    rots: Vec<Option<Quat>>,
    wall_legs: Option<[WallLeg; 2]>,
    wall_seen: Seen,
}

impl GSSimLegs {
    fn parents_for(&mut self, skeleton: &Gd<Skeleton3D>) {
        let seen = Seen::of(skeleton);
        if self.parents_seen != seen {
            self.parents_seen = seen;
            self.parents = parents_of(skeleton);
        }
    }

    /// `GSHumanoidLegs._legs_for`.
    fn legs_for(&mut self, skeleton: &Gd<Skeleton3D>) {
        self.parents_for(skeleton);
        let seen = Seen::of(skeleton);
        if self.floor_seen != seen {
            self.floor_seen = seen;
            self.legs.floor_legs = floor_bones(skeleton, &self.parents);
        }
    }
}

#[godot_api]
impl GSSimLegs {
    /// `GSHumanoidLegs.restore`.
    #[func]
    fn restore(&mut self, skeleton: Option<Gd<Skeleton3D>>) {
        let Some(mut skeleton) = skeleton else { return };
        self.parents_for(&skeleton);
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        self.legs.restore(&mut s);
    }

    /// `GSHumanoidLegs.apply` past its null and zero-twist checks: the root turned by [param twist]
    /// about [param up] (skeleton space), the hips back by as much.
    #[func]
    fn apply(&mut self, skeleton: Option<Gd<Skeleton3D>>, up: Vector3, twist: f64) {
        let Some(mut skeleton) = skeleton else { return };
        let seen = Seen::of(&skeleton);
        if self.turn_seen != seen {
            self.turn_seen = seen;
            self.root = skeleton.find_bone("Root");
            self.hips = skeleton.find_bone("Hips");
        }
        if self.root < 0 || self.hips < 0 {
            return;
        }
        self.parents_for(&skeleton);
        let (root, hips) = (self.root, self.hips);
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        self.legs.apply(&mut s, root, hips, v3(up), twist);
    }

    /// `GSHumanoidLegs.on_floor`.
    #[func]
    fn on_floor(
        &mut self,
        skeleton: Option<Gd<Skeleton3D>>,
        to_figure: Transform3D,
        floor_y: PackedFloat32Array,
        raised: f64,
    ) {
        let Some(mut skeleton) = skeleton else { return };
        if floor_y.len() < gs_sim_figure::LEGS {
            return;
        }
        self.legs_for(&skeleton);
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        s.warm();
        self.legs
            .on_floor(&mut s, &xf(&to_figure), floor_y.as_slice(), raised);
    }

    /// `GSHumanoidLegs.on_wall` past its weight and wall checks: [param plane] and [param up] are
    /// in skeleton space, [param weight] already clamped.
    #[func]
    fn on_wall(
        &mut self,
        skeleton: Option<Gd<Skeleton3D>>,
        plane: Plane,
        up: Vector3,
        weight: f64,
    ) {
        let Some(mut skeleton) = skeleton else { return };
        self.parents_for(&skeleton);
        let seen = Seen::of(&skeleton);
        if self.wall_seen != seen {
            self.wall_seen = seen;
            self.wall_legs = wall_bones(&skeleton);
        }
        let Some(legs) = self.wall_legs.clone() else {
            return;
        };
        let wall = Wall {
            normal: v3(plane.normal),
            d: plane.d,
        };
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        s.warm();
        gs_sim_figure::on_wall(&mut s, &legs, &wall, v3(up), weight);
    }

    /// `GSHumanoidLegs.hold_feet`.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn hold_feet(
        &mut self,
        skeleton: Option<Gd<Skeleton3D>>,
        to_world: Transform3D,
        to_figure: Transform3D,
        floor_y: PackedFloat32Array,
        height: f64,
        settle: f64,
        reach: f64,
        release: f64,
        dt: f64,
    ) {
        let Some(mut skeleton) = skeleton else { return };
        if floor_y.len() < gs_sim_figure::LEGS || dt <= 0.0 {
            return;
        }
        self.legs_for(&skeleton);
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        s.warm();
        self.legs.hold_feet(
            &mut s,
            &xf(&to_world),
            &xf(&to_figure),
            floor_y.as_slice(),
            height,
            settle,
            reach,
            release,
            dt,
        );
    }

    /// `GSHumanoidLegs.step_off`.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn step_off(
        &mut self,
        skeleton: Option<Gd<Skeleton3D>>,
        to_world: Transform3D,
        to_figure: Transform3D,
        floor_y: PackedFloat32Array,
        height: f64,
        seconds: f64,
        lift: f64,
        dt: f64,
    ) {
        let Some(mut skeleton) = skeleton else { return };
        if dt <= 0.0 || floor_y.len() < gs_sim_figure::LEGS {
            return;
        }
        self.legs_for(&skeleton);
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        s.warm();
        self.legs.step_off(
            &mut s,
            &xf(&to_world),
            &xf(&to_figure),
            floor_y.as_slice(),
            height,
            seconds,
            lift,
            dt,
        );
    }

    /// `GSHumanoidLegs.resettle`: on the legs the last call worked out (it does not look again).
    #[func]
    fn resettle(&mut self, skeleton: Option<Gd<Skeleton3D>>, to_world: Transform3D) {
        let Some(mut skeleton) = skeleton else { return };
        if self.legs.floor_legs.is_empty() {
            return;
        }
        self.parents_for(&skeleton);
        let s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        self.legs.resettle(&s, &xf(&to_world));
    }

    #[func]
    fn holding(&self) -> bool {
        self.legs.holding()
    }

    #[func]
    fn arm(&mut self) {
        self.legs.arm();
    }

    #[func]
    fn let_go(&mut self) {
        self.legs.let_go();
    }
}

/// One `GSHumanoidTurnLimit`: [method setup] with its bone names, then the same calls.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimTurnLimit {
    limit: TurnLimit,
    names: Vec<GString>,
    arm_names: Vec<GString>,
    seen: Seen,
    parents: Vec<i32>,
    poses: Vec<Option<Xform>>,
    rots: Vec<Option<Quat>>,
}

#[godot_api]
impl GSSimTurnLimit {
    /// The bones to turn, and those of them the arm rate governs.
    #[func]
    fn setup(&mut self, names: PackedStringArray, arm_names: PackedStringArray) {
        self.names = names.as_slice().to_vec();
        self.arm_names = arm_names.as_slice().to_vec();
        self.seen = Seen::default();
        self.limit = TurnLimit::default();
    }

    /// `GSHumanoidTurnLimit.step`.
    #[func]
    fn step(&mut self, skeleton: Option<Gd<Skeleton3D>>, rate: f64, dt: f64, arm_rate: f64) {
        let Some(mut skeleton) = skeleton else {
            self.limit.forget();
            return;
        };
        if rate <= 0.0 {
            self.limit.forget();
            return;
        }
        let seen = Seen::of(&skeleton);
        if self.seen != seen {
            self.seen = seen;
            self.parents = parents_of(&skeleton);
            let mut bones = Vec::new();
            let mut arm = Vec::new();
            for name in &self.names {
                let bone = skeleton.find_bone(name);
                if bone >= 0 {
                    bones.push(bone);
                    arm.push(self.arm_names.contains(name));
                }
            }
            self.limit.set_bones(bones, arm);
        }
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        self.limit.step(&mut s, rate, dt, arm_rate);
    }

    /// `GSHumanoidTurnLimit.restore`.
    #[func]
    fn restore(&mut self, skeleton: Option<Gd<Skeleton3D>>) {
        match skeleton {
            Some(mut skeleton) => {
                let mut s = Skel::new(
                    &mut skeleton,
                    &self.parents,
                    &mut self.poses,
                    &mut self.rots,
                );
                self.limit.restore(Some(&mut s));
            }
            None => self.limit.restore(None::<&mut Skel>),
        }
    }

    #[func]
    fn turned(&self) -> bool {
        self.limit.turned()
    }

    #[func]
    fn clear(&mut self) {
        self.limit.clear();
    }
}

/// One figure's arm posing after its clip: `GSHumanoidAim.apply_plan`, the gun arm's
/// `GSHumanoidCarryPose.apply` and the support hand's `GSHumanoidIk.two_bone_under`. The script
/// keeps the plans, chains and its carry state; this side caches the skeleton's parents and what
/// the carry measured on the body and baked from the style.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimFigurePose {
    parents: Vec<i32>,
    parents_seen: Seen,
    poses: Vec<Option<Xform>>,
    rots: Vec<Option<Quat>>,
    carry_body: Option<CarryBody>,
    carry_style: Option<CarryStyle>,
}

impl GSSimFigurePose {
    fn parents_for(&mut self, skeleton: &Gd<Skeleton3D>) {
        let seen = Seen::of(skeleton);
        if self.parents_seen != seen {
            self.parents_seen = seen;
            self.parents = parents_of(skeleton);
        }
    }
}

#[godot_api]
impl GSSimFigurePose {
    /// `GSHumanoidAim.apply_plan` with the plan flat (index, parent, share, 1 for an arm bone) and
    /// the sprint drop worked out: [param drop] is `-SPRINT_DROP * sprint_blend`.
    #[func]
    fn aim(
        &mut self,
        skeleton: Option<Gd<Skeleton3D>>,
        plan: PackedFloat64Array,
        to_figure: Transform3D,
        pitch: f64,
        drop: f64,
    ) {
        let Some(mut skeleton) = skeleton else { return };
        self.parents_for(&skeleton);
        let rows = xf(&to_figure).rows;
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        gs_sim_figure::aim(&mut s, plan.as_slice(), &rows, pitch, drop);
    }

    /// `GSHumanoidIk.two_bone_under(skeleton, bones, chain_pose(skeleton, chain), target, pole, weight)`.
    #[func]
    fn support(
        &mut self,
        skeleton: Option<Gd<Skeleton3D>>,
        bones: PackedInt32Array,
        chain: PackedInt32Array,
        target: Vector3,
        pole: Vector3,
        weight: f64,
    ) {
        let Some(mut skeleton) = skeleton else { return };
        let &[a, b, c] = bones.as_slice() else { return };
        self.parents_for(&skeleton);
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        let under = gs_sim_figure::chain_pose(&s, chain.as_slice());
        gs_sim_figure::two_bone_under(&mut s, [a, b, c], &under, v3(target), v3(pole), weight);
    }

    /// What `GSHumanoidCarryPose._measure` found on a body, quaternions packed four floats each.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn carry_body(
        &mut self,
        chest_chain: PackedInt32Array,
        shoulder: i32,
        hand_frame: Transform3D,
        hand_frame_inv: Transform3D,
        chest_ref: Transform3D,
        hand_rest: Quaternion,
        fingers: PackedInt32Array,
        finger_rest: PackedFloat32Array,
        grip_bones: PackedInt32Array,
        grip_pose: PackedFloat32Array,
    ) {
        self.carry_body = Some(CarryBody {
            chest_chain: chest_chain.to_vec(),
            shoulder,
            hand_frame: xf(&hand_frame),
            hand_frame_inv: xf(&hand_frame_inv),
            chest_ref: xf(&chest_ref),
            hand_rest: [hand_rest.x, hand_rest.y, hand_rest.z, hand_rest.w],
            fingers: fingers.to_vec(),
            finger_rest: quats(&finger_rest),
            grip_bones: grip_bones.to_vec(),
            grip_pose: quats(&grip_pose),
        });
    }

    /// A `GSHumanoidCarry` as the carry reads it: the five grips and baked turns (ready, sprint,
    /// aim, reload, swap), `values` = follow, kick, kick_climb, bolt, bolt_back, bolt_roll.
    #[func]
    fn carry_style(
        &mut self,
        grips: PackedVector3Array,
        turns: PackedFloat32Array,
        values: PackedFloat64Array,
        walk_sway: Vector3,
        run_sway: Vector3,
        support: Vector3,
    ) {
        let turns = quats(&turns);
        let (Ok(g), Ok(t), &[follow, kick, kick_climb, bolt, bolt_back, bolt_roll]) = (
            <[Vector3; 5]>::try_from(grips.as_slice()),
            <[Quat; 5]>::try_from(turns.as_slice()),
            values.as_slice(),
        ) else {
            self.carry_style = None;
            return;
        };
        self.carry_style = Some(CarryStyle {
            grips: g.map(v3),
            turns: t,
            follow,
            walk_sway: v3(walk_sway),
            run_sway: v3(run_sway),
            kick,
            kick_climb,
            bolt,
            bolt_back,
            bolt_roll,
            support: v3(support),
        });
    }

    /// `GSHumanoidCarryPose.apply` past its checks: poses the gun arm (`bones`: upper, lower,
    /// hand) and returns the support grip, `Vector3.INF` for none. `frame` is the
    /// [`CarryFrame`] layout; `keyed` whether the clip keys each finger root.
    #[func]
    fn carry(
        &mut self,
        skeleton: Option<Gd<Skeleton3D>>,
        bones: PackedInt32Array,
        keyed: PackedByteArray,
        to_figure: Transform3D,
        frame: PackedFloat64Array,
    ) -> Vector3 {
        let none = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let (Some(mut skeleton), Some(body), Some(style)) = (
            skeleton,
            self.carry_body.as_ref(),
            self.carry_style.as_ref(),
        ) else {
            return none;
        };
        let (&[a, b, c], Some(f)) = (bones.as_slice(), CarryFrame::from_slice(frame.as_slice()))
        else {
            return none;
        };
        let seen = Seen::of(&skeleton);
        if self.parents_seen != seen {
            self.parents_seen = seen;
            self.parents = parents_of(&skeleton);
        }
        let mut s = Skel::new(
            &mut skeleton,
            &self.parents,
            &mut self.poses,
            &mut self.rots,
        );
        match gs_sim_figure::carry(
            &mut s,
            body,
            style,
            [a, b, c],
            keyed.as_slice(),
            &xf(&to_figure),
            &f,
        ) {
            Some(v) => Vector3::new(v.x, v.y, v.z),
            None => none,
        }
    }
}
