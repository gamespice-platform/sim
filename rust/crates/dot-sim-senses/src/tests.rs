use super::*;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn npc_at(here: Vec3) -> Npc {
    Npc {
        here,
        facing: v(0.0, 0.0, -1.0),
        eye: 1.5,
        reach: 30.0,
        hearing: 12.0,
        half_deg: 60.0,
        half_rad: 60f64.to_radians(),
        blind_until: -1.0,
        target_seen_at: 0.0,
        own: 1,
        held: -1,
        has_target: false,
        require_los: true,
    }
}

struct World {
    pos: Vec<Vec3>,
    loud: Vec<f64>,
    faction: Vec<i32>,
    first: Vec<i32>,
}

impl World {
    fn new(pos: &[Vec3]) -> Self {
        Self {
            pos: pos.to_vec(),
            loud: vec![0.0; pos.len()],
            faction: vec![0; pos.len()],
            first: (0..pos.len() as i32).collect(),
        }
    }

    fn cands(&self) -> Cands<'_> {
        Cands {
            pos: &self.pos,
            loud: &self.loud,
            faction: &self.faction,
            first: &self.first,
        }
    }
}

fn ranked(npc: &Npc, w: &World, cap: i64) -> Vec<i32> {
    let (mut keys, mut out) = (Vec::new(), Vec::new());
    rank(
        npc.here,
        npc.reach,
        npc.hearing,
        npc.own,
        &w.cands(),
        cap,
        &mut keys,
        &mut out,
    );
    out
}

fn run(npc: &Npc, w: &World, now: f64, los: &mut impl FnMut(Vec3, Vec3) -> bool) -> (Outcome, i64) {
    let order = ranked(npc, w, 0);
    let mut checks = 0;
    let o = decide(
        npc,
        &w.cands(),
        &[],
        &order,
        &Tuning::default(),
        now,
        &mut checks,
        los,
    );
    (o, checks)
}

#[test]
fn rank_is_nearest_first_and_capped() {
    let mut w = World::new(&[
        v(0.0, 0.0, -9.0),
        v(0.0, 0.0, -3.0),
        v(0.0, 0.0, -50.0),
        v(0.0, 0.0, -6.0),
    ]);
    let npc = npc_at(Vec3::default());
    assert_eq!(ranked(&npc, &w, 0), vec![1, 3, 0]);
    assert_eq!(ranked(&npc, &w, 2), vec![1, 3]);
    // Own faction and missing candidates are never ranked; loudness extends reach to the hearing range at most.
    w.faction[1] = 1;
    w.faction[3] = -2;
    w.loud[2] = 100.0;
    assert_eq!(ranked(&npc, &w, 0), vec![0]);
}

#[test]
fn nearest_visible_is_acquired_and_walls_are_asked_nearest_first() {
    let w = World::new(&[v(0.0, 0.0, -9.0), v(0.0, 0.0, -3.0)]);
    let npc = npc_at(Vec3::default());
    let mut asked = Vec::new();
    let (o, checks) = run(&npc, &w, 1.0, &mut |_f, to| {
        asked.push(to);
        to.z < -5.0
    });
    assert_eq!(
        o,
        Outcome {
            target: Target::Set(0),
            seen: true,
            since: true
        }
    );
    assert_eq!(checks, 2);
    assert_eq!(asked, vec![v(0.0, 0.0, -3.0), v(0.0, 0.0, -9.0)]);
}

#[test]
fn hearing_skips_the_ray_and_the_cone() {
    let mut w = World::new(&[v(0.0, 0.0, 4.0)]);
    w.loud[0] = 8.0;
    let npc = npc_at(Vec3::default());
    let (o, _) = run(&npc, &w, 1.0, &mut |_, _| {
        panic!("no ray for a heard candidate")
    });
    assert_eq!(o.target, Target::Set(0));
}

#[test]
fn a_held_target_is_kept_until_a_rival_is_much_nearer() {
    let w = World::new(&[v(0.0, 0.0, -10.0), v(0.0, 0.0, -7.0), v(0.0, 0.0, -5.0)]);
    let mut npc = npc_at(Vec3::default());
    npc.held = 0;
    npc.has_target = true;
    // 7 m is not under 0.6 of 10 m: kept, and the 7 m rival is never tested.
    let mut rays = 0;
    let (o, _) = run(&npc, &w, 2.0, &mut |_, to| {
        rays += 1;
        to.z != -5.0
    });
    assert_eq!(
        o,
        Outcome {
            target: Target::Keep,
            seen: true,
            since: false
        }
    );
    assert_eq!(rays, 2);
    let (o, _) = run(&npc, &w, 2.0, &mut |_, _| true);
    assert_eq!(
        o,
        Outcome {
            target: Target::Set(2),
            seen: true,
            since: true
        }
    );
}

#[test]
fn a_lost_target_is_chased_for_the_grace_then_dropped() {
    let w = World::new(&[v(0.0, 0.0, -10.0)]);
    let mut npc = npc_at(Vec3::default());
    npc.held = 0;
    npc.has_target = true;
    npc.target_seen_at = 5.0;
    assert_eq!(run(&npc, &w, 7.9, &mut |_, _| false).0, Outcome::KEEP);
    assert_eq!(
        run(&npc, &w, 8.0, &mut |_, _| false).0.target,
        Target::Clear
    );
    npc.blind_until = 9.0;
    assert_eq!(
        run(&npc, &w, 6.0, &mut |_, _| panic!("blind")).0,
        Outcome::KEEP
    );
}

#[test]
fn smoke_blocks_sight_until_it_expires() {
    let w = World::new(&[v(0.0, 0.0, -10.0)]);
    let npc = npc_at(Vec3::default());
    let smoke = [Blocker {
        center: v(0.0, 1.0, -5.0),
        radius: 1.0,
        until: 3.0,
    }];
    let order = ranked(&npc, &w, 0);
    let mut checks = 0;
    let t = Tuning::default();
    let o = decide(
        &npc,
        &w.cands(),
        &smoke,
        &order,
        &t,
        2.0,
        &mut checks,
        &mut |_, _| true,
    );
    assert_eq!(o.target, Target::Keep);
    let o = decide(
        &npc,
        &w.cands(),
        &smoke,
        &order,
        &t,
        3.0,
        &mut checks,
        &mut |_, _| true,
    );
    assert_eq!(o.target, Target::Set(0));
}
