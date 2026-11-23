//! `GSSimSenses`: a whole perception pass of gs-npc's `GSNpcPerception` (`GSNpcSenses.rank_many`, then
//! `GSNpcSenses.decide` for each NPC due) in one call, with the same targets, timestamps and counters as
//! the script (`gs_sim_senses`, to the bit) and the same occlusion rays, cast in the same order.
//!
//! The script packs what it already reads per NPC and per candidate, calls [`GSSimSenses::perceive`] and
//! writes the answers back; it keeps every NPC's state, so the script pass and this one see one state.

use godot::classes::{
    CollisionObject3D, Node3D, PhysicsDirectSpaceState3D, PhysicsRayQueryParameters3D,
};
use godot::prelude::*;
use gs_sim_math::Vec3;
use gs_sim_senses::{self as senses, Blocker, Cands, Npc, Outcome, Target, Tuning};

/// Floats per NPC in `perceive`'s `npc_f`.
const NPC_F: usize = 13;
/// Ints per NPC in `perceive`'s `npc_i`.
const NPC_I: usize = 4;
/// Floats per blocker in `perceive`'s `blockers`.
const BLOCKER_F: usize = 5;

/// `target` codes in `perceive`'s answer.
const KEEP: i32 = -1;
const CLEAR: i32 = -2;
/// `flags` bits in `perceive`'s answer.
const SEEN: i32 = 1;
const SINCE: i32 = 2;
const QUIET: i32 = 4;

#[inline]
fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[inline]
fn gv(v: Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// Where one NPC casts from: its space and the RID it excludes, found at its first ray.
enum Caster {
    Unknown,
    /// Not a 3D body or no world yet: the script treats its sight as clear.
    Clear,
    Space(Gd<PhysicsDirectSpaceState3D>, Option<Rid>),
}

/// The perception pass; [method configure] mirrors the `GSNpcSenses` settings.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimSenses {
    tuning: Tuning,
    mask: u32,
    ray: Gd<PhysicsRayQueryParameters3D>,
    /// The RID the ray excludes now (`None`: excludes nothing).
    excluded: Option<Rid>,
    keys: Vec<i64>,
    order: Vec<i32>,
    pos: Vec<Vec3>,
    blockers: Vec<Blocker>,
}

#[godot_api]
impl IRefCounted for GSSimSenses {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            tuning: Tuning::default(),
            mask: 1,
            ray: PhysicsRayQueryParameters3D::new_gd(),
            excluded: None,
            keys: Vec::new(),
            order: Vec::new(),
            pos: Vec::new(),
            blockers: Vec::new(),
        }
    }
}

#[godot_api]
impl GSSimSenses {
    /// `switch_ratio`, `commitment_grace`, `line_of_sight_enabled`, `prune_by_held` and `occlusion_mask`.
    #[func]
    fn configure(
        &mut self,
        switch_ratio: f64,
        grace: f64,
        los_enabled: bool,
        prune_by_held: bool,
        mask: i64,
    ) {
        self.tuning = Tuning {
            switch_ratio,
            grace,
            los_enabled,
            prune_by_held,
        };
        self.mask = mask as u32;
    }

    /// One pass at simulated second [param now] with candidate cap [param cap] (0 or less: all).
    /// Per NPC, `npc_f` holds 13 floats (position xyz, facing xyz, eye height, sight reach, hearing range,
    /// half angle in degrees and in radians, blind until, target seen at), `npc_i` 4 ints (own faction
    /// number, held candidate index, has a target, needs line of sight) and `nodes` its node. Candidates
    /// are `GSNpcSenses.Packed`'s arrays plus `cand_first` (each id as its first index); `blockers` 5 floats
    /// each (centre xyz, radius, until). Answers 2 ints per NPC (target: -1 kept, -2 cleared, else the
    /// candidate index; flags: 1 seen, 2 since, 4 quiet), then the pass's sight checks and rays cast.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn perceive(
        &mut self,
        now: f64,
        cap: i64,
        npc_f: PackedFloat64Array,
        npc_i: PackedInt32Array,
        nodes: VarArray,
        cand_pos: PackedVector3Array,
        cand_loud: PackedFloat64Array,
        cand_fac: PackedInt32Array,
        cand_first: PackedInt32Array,
        blockers: PackedFloat64Array,
    ) -> PackedInt32Array {
        let f = npc_f.as_slice();
        let ii = npc_i.as_slice();
        let n = (f.len() / NPC_F).min(ii.len() / NPC_I).min(nodes.len());

        self.pos.clear();
        self.pos.extend(cand_pos.as_slice().iter().map(|p| v3(*p)));
        self.blockers.clear();
        self.blockers.extend(
            blockers
                .as_slice()
                .as_chunks::<BLOCKER_F>()
                .0
                .iter()
                .map(|b| Blocker {
                    center: Vec3::new(b[0] as f32, b[1] as f32, b[2] as f32),
                    radius: b[3],
                    until: b[4],
                }),
        );
        let pos = std::mem::take(&mut self.pos);
        let c = Cands {
            pos: &pos,
            loud: cand_loud.as_slice(),
            faction: cand_fac.as_slice(),
            first: cand_first.as_slice(),
        };

        let mut out = Vec::with_capacity(n * 2 + 2);
        let mut checks = 0i64;
        let mut casts = 0i64;
        for k in 0..n {
            let a = &f[k * NPC_F..(k + 1) * NPC_F];
            let b = &ii[k * NPC_I..(k + 1) * NPC_I];
            let npc = Npc {
                here: Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32),
                facing: Vec3::new(a[3] as f32, a[4] as f32, a[5] as f32),
                eye: a[6],
                reach: a[7],
                hearing: a[8],
                half_deg: a[9],
                half_rad: a[10],
                blind_until: a[11],
                target_seen_at: a[12],
                own: b[0],
                held: b[1],
                has_target: b[2] != 0,
                require_los: b[3] != 0,
            };
            let mut keys = std::mem::take(&mut self.keys);
            let mut order = std::mem::take(&mut self.order);
            senses::rank(
                npc.here,
                npc.reach,
                npc.hearing,
                npc.own,
                &c,
                cap,
                &mut keys,
                &mut order,
            );
            let mut caster = Caster::Unknown;
            let blockers = std::mem::take(&mut self.blockers);
            let tuning = self.tuning;
            let o: Outcome = senses::decide(
                &npc,
                &c,
                &blockers,
                &order,
                &tuning,
                now,
                &mut checks,
                &mut |from, to| self.clear(&nodes, k, &mut caster, &mut casts, from, to),
            );
            self.blockers = blockers;
            let target = match o.target {
                Target::Keep => KEEP,
                Target::Clear => CLEAR,
                Target::Set(i) => i,
            };
            let mut flags = 0;
            if o.seen {
                flags |= SEEN;
            }
            if o.since {
                flags |= SINCE;
            }
            if order.is_empty() {
                flags |= QUIET;
            }
            out.push(target);
            out.push(flags);
            self.keys = keys;
            self.order = order;
        }
        self.pos = pos;
        out.push(checks as i32);
        out.push(casts as i32);
        PackedInt32Array::from(out.as_slice())
    }
}

impl GSSimSenses {
    /// `GSNpcSenses._has_line_of_sight` for NPC `k` of `nodes`.
    fn clear(
        &mut self,
        nodes: &VarArray,
        k: usize,
        caster: &mut Caster,
        casts: &mut i64,
        from: Vec3,
        to: Vec3,
    ) -> bool {
        if matches!(caster, Caster::Unknown) {
            *caster = match nodes.get(k).and_then(|v| v.try_to::<Gd<Node3D>>().ok()) {
                None => Caster::Clear,
                Some(body) => match body.get_world_3d().and_then(|w| w.get_direct_space_state()) {
                    None => Caster::Clear,
                    Some(space) => {
                        let rid = body
                            .try_cast::<CollisionObject3D>()
                            .ok()
                            .map(|co| co.get_rid());
                        Caster::Space(space, rid)
                    }
                },
            };
        }
        let Caster::Space(space, rid) = caster else {
            return true;
        };
        *casts += 1;
        self.ray.set_from(gv(from));
        self.ray.set_to(gv(to));
        self.ray.set_collision_mask(self.mask);
        if self.excluded != *rid {
            self.excluded = *rid;
            let mut ex: Array<Rid> = Array::new();
            if let Some(r) = rid {
                ex.push(*r);
            }
            self.ray.set_exclude(&ex);
        }
        space.intersect_ray(&self.ray).is_empty()
    }
}
