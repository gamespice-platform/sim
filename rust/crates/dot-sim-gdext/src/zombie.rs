//! `DotSimZombieBrains`: Zombie Zone's zombie brains in Rust (`dot_sim_zombie`). The horde pushes its
//! survivor facts and chase rules once a tick ([method set_survivors], [method set_rules]); each common's
//! think then asks [method plan_chase] once for what `ZzCommonBrain._chase_plan_script` reads in five
//! script calls, to the bit. The spacing half of a zombie's steer is [code]DotSimCrowdGrid.spaced_direction[/code];
//! [method steer_step] is the whole steer after the path (spacing, wish, turn, look, progress watch) in one call,
//! and [method walk] the whole move: the path's next corner, then that steer, with every member handed in.

use crate::crowd::DotSimCrowdGrid;
use dot_sim_math::Vec3;
use dot_sim_zombie::steer::{self, SteerIn, SteerOut, Turn, TurnProfile, Watch};
use dot_sim_zombie::walk::{self, PathGoal, PathState, Repath};
use dot_sim_zombie::{self as zombie, Chaser, Rules, Survivor, Who};
use godot::classes::Node3D;
use godot::prelude::*;

/// `set_survivors` flag bits.
const ALIVE: i32 = 1;
const DOWN: i32 = 2;

/// The tick's survivor table and chase rules, and the plans asked of them.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimZombieBrains {
    ids: Vec<StringName>,
    survivors: Vec<Survivor>,
    rules: Rules,
    empty: StringName,
    planned: i64,
    names: Option<Names>,
    steered: i64,
    /// [method walk]'s path points, reused.
    points: Vec<Vec3>,
}

/// The script members [method DotSimZombieBrains.steer_step] reads and writes, named once.
struct Names {
    facing_at_tick: StringName,
    progress_at: StringName,
    progress_since: StringName,
    stuck_seconds: StringName,
    cruise_to: StringName,
    cruise_dest: StringName,
    peak: StringName,
    over_amt: StringName,
    over_axis: StringName,
    turns: StringName,
    view_factor: StringName,
    view_turn_deg: StringName,
    overshoot: StringName,
    seed: StringName,
    wish: StringName,
}

impl Names {
    fn new() -> Self {
        let n = |s: &str| StringName::from(s);
        Self {
            facing_at_tick: n("_facing_at_tick"),
            progress_at: n("_progress_at"),
            progress_since: n("_progress_since"),
            stuck_seconds: n("stuck_seconds"),
            cruise_to: n("_cruise_to"),
            cruise_dest: n("_cruise_dest"),
            peak: n("_peak_turn"),
            over_amt: n("_over_amt"),
            over_axis: n("_over_axis"),
            turns: n("_turns"),
            view_factor: n("view_factor"),
            view_turn_deg: n("view_turn_deg"),
            overshoot: n("overshoot"),
            seed: n("seed_value"),
            wish: n("wish"),
        }
    }
}

#[inline]
fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[inline]
fn gv(v: Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// A member read as `T`, or `None` when it is missing or another type (the caller falls back to the script).
#[inline]
fn member<T: FromGodot>(o: &Gd<Object>, name: &StringName) -> Option<T> {
    o.get(name).try_to::<T>().ok()
}

impl DotSimZombieBrains {
    fn who(&self, name: &StringName) -> Who {
        if *name == self.empty {
            return Who::Nobody;
        }
        self.ids
            .iter()
            .position(|id| id == name)
            .map_or(Who::Unknown, Who::Is)
    }
}

#[godot_api]
impl DotSimZombieBrains {
    /// The tick's survivors: [param ids], where each stands, and its flags (1 alive, 2 down), as
    /// the horde's facts. Arrays of different sizes are an error and empty the table.
    #[func]
    fn set_survivors(
        &mut self,
        ids: Array<StringName>,
        at: PackedVector3Array,
        flags: PackedInt32Array,
    ) -> bool {
        self.ids.clear();
        self.survivors.clear();
        let (at, flags) = (at.as_slice(), flags.as_slice());
        if ids.len() != at.len() || ids.len() != flags.len() {
            godot_error!(
                "DotSimZombieBrains.set_survivors: {} ids, {} positions, {} flags",
                ids.len(),
                at.len(),
                flags.len()
            );
            return false;
        }
        for (i, id) in ids.iter_shared().enumerate() {
            self.ids.push(id);
            self.survivors.push(Survivor {
                at: Vec3::new(at[i].x, at[i].y, at[i].z),
                alive: flags[i] & ALIVE != 0,
                down: flags[i] & DOWN != 0,
            });
        }
        true
    }

    /// The tick's chase rules: the walking and running chase speeds, whether an Angry Box is on
    /// and the share of rolls that run under it, and whether a downed survivor is struck on the ground.
    #[func]
    fn set_rules(
        &mut self,
        walk_speed: f64,
        run_speed: f64,
        angry: bool,
        runner_fraction: f64,
        down_attack: bool,
    ) {
        self.rules = Rules {
            walk_speed,
            run_speed,
            angry,
            runner_fraction,
            down_attack,
        };
    }

    /// One common's chase facts: [code][code, dropped, at.x, at.y, at.z, speed, gap, reach][/code], with
    /// code 0 the forced survivor, 1 the perceived one, -1 nobody, -2 nobody to be found, and dropped 1
    /// when [param forced] is to be cleared. [param target] is empty when the NPC has no target.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn plan_chase(
        &mut self,
        forced: StringName,
        target: StringName,
        pace: f64,
        roll: f64,
        reach: f64,
        burning: bool,
        crawler: bool,
        here: Vector3,
    ) -> PackedFloat64Array {
        self.planned += 1;
        let z = Chaser {
            forced: self.who(&forced),
            target: self.who(&target),
            pace,
            roll,
            reach,
            burning,
            crawler,
            here: Vec3::new(here.x, here.y, here.z),
        };
        let p = zombie::plan_chase(&self.survivors, &self.rules, &z);
        PackedFloat64Array::from(&[
            f64::from(p.code),
            if p.dropped { 1.0 } else { 0.0 },
            f64::from(p.at.x),
            f64::from(p.at.y),
            f64::from(p.at.z),
            p.speed,
            p.gap,
            p.reach,
        ])
    }

    /// The rest of a zombie's steer once its path gave [param goal] (dot-npc-ai's `steer_with_spacing` after
    /// `_steer_goal`, with Zombie Zone's `steer_toward` and no avoidance node): the spaced heading off
    /// [param grid] for [param id], the body's wish, the eased turn of [param character] from the brain's
    /// `_facing_at_tick`, the `look_at` and the progress watch, in one call. [param to] is the destination,
    /// [param delta] the think's `context.delta` and [param now] the world clock.
    ///
    /// With [param apply] it writes all of it (the brain's cruise and progress, the character's turn
    /// state, the body's wish and transform); without, it only answers, for the script to compare.
    /// Returns [code][code, watch, wished, wish.x, wish.y, wish.z, looked, look.x, look.y, look.z, peak,
    /// over_amt, turns, over_axis.x, over_axis.y, over_axis.z, here.x, here.y, here.z][/code]: code -1
    /// when [param id] is not in the grid or a member could not be read (nothing was touched; the script
    /// steers), 0 when the heading cancelled, 1 steered; watch 0 kept, 1 reset, 2 stuck long enough for
    /// the path's rough corners (the script asks the path and resets the watch).
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn steer_step(
        &mut self,
        grid: Gd<DotSimCrowdGrid>,
        id: i64,
        brain: Gd<Object>,
        character: Gd<Object>,
        body: Gd<Node3D>,
        goal: Vector3,
        to: Vector3,
        spacing: f64,
        speed: f64,
        delta: f64,
        now: f64,
        apply: bool,
    ) -> PackedFloat64Array {
        let names = self.names.get_or_insert_with(Names::new);
        let (Some(facing_at_tick), Some(progress_at), Some(progress_since), Some(stuck_seconds)) = (
            member::<Vector3>(&brain, &names.facing_at_tick),
            member::<Vector3>(&brain, &names.progress_at),
            member::<f64>(&brain, &names.progress_since),
            member::<f64>(&brain, &names.stuck_seconds),
        ) else {
            return PackedFloat64Array::from(&[-1.0]);
        };
        let watch = BrainWatch {
            facing_at_tick,
            progress_at,
            progress_since,
            stuck_seconds,
        };
        let (out, steered, reset, here) = self.steer_core(
            &grid, id, character, body, goal, to, spacing, speed, delta, now, apply, &watch,
        );

        if apply && steered {
            let names = self.names.get_or_insert_with(Names::new);
            let mut brain = brain;
            brain.set(&names.cruise_to, &goal.to_variant());
            brain.set(&names.cruise_dest, &to.to_variant());
            if reset {
                brain.set(&names.progress_at, &here.to_variant());
                brain.set(&names.progress_since, &now.to_variant());
            }
        }

        out
    }

    /// [method steer_step] with the brain's own state handed in rather than read off it, and nothing
    /// written to the brain: [param facing_at_tick], [param progress_at] and [param stuck_seconds]
    /// are its `_facing_at_tick`, `_progress_at` and `stuck_seconds`, and [param waited] is
    /// `now - _progress_since` (the only way the steer reads either clock). With [param apply] the
    /// character's turn state, the body's wish and its look are written as [method steer_step] writes
    /// them; the caller sets the brain's `_cruise_to` to [param goal] and `_cruise_dest` to [param to]
    /// when code is 1, and its `_progress_at` to `here` and `_progress_since` to now when watch is 1.
    /// Six fewer script-member reads and writes a steer than [method steer_step]; the same answer, to
    /// the bit.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn steer_walk(
        &mut self,
        grid: Gd<DotSimCrowdGrid>,
        id: i64,
        character: Gd<Object>,
        body: Gd<Node3D>,
        goal: Vector3,
        to: Vector3,
        spacing: f64,
        speed: f64,
        delta: f64,
        apply: bool,
        facing_at_tick: Vector3,
        progress_at: Vector3,
        waited: f64,
        stuck_seconds: f64,
    ) -> PackedFloat64Array {
        // `now - progress_since` with `now` = waited and `progress_since` = 0 is `waited`, exactly.
        let watch = BrainWatch {
            facing_at_tick,
            progress_at,
            progress_since: 0.0,
            stuck_seconds,
        };
        self.steer_core(
            &grid, id, character, body, goal, to, spacing, speed, delta, waited, apply, &watch,
        )
        .0
    }

    /// A zombie's whole move in one call, every member handed in by value and none read or written
    /// here: dot-npc's `path_toward` on a path not due a repath (its repath test and advance, the
    /// script's `_as_goal`), then [method steer_walk]'s steer for that goal. Under [param apply] only
    /// the body's look is written; the caller writes the rest from the answer.
    ///
    /// [param points] is the path's `points`. [param s]: [code][to.x, to.y, to.z, spacing, speed,
    /// delta, now, facing_at_tick.x, .y, .z, progress_at.x, .y, .z, progress_since, stuck_seconds,
    /// peak, over_amt, over_axis.x, .y, .z, view_factor, view_turn_deg, overshoot, arrive_radius,
    /// computed_at, goal_at_compute.x, .y, .z, repath_drift, repath_interval, repath_drift_ratio,
    /// repath_interval_per_m][/code] (the brain's watch, its character's turn state and tuning, its
    /// path and the spawner's repath tuning; [code]now[/code] the spawner's clock). [param n]:
    /// [code][turns, seed_value, index, partial][/code].
    ///
    /// Returns [code][-2][/code] when the path is due a repath (nothing done: the script moves),
    /// [code][-1][/code] when [param id] is not in the grid or an array is short, else
    /// [method steer_step]'s answer followed by [code][index, goal.x, goal.y, goal.z][/code]: the
    /// path's index and the brain's `_waypoint` (the goal steered for) whatever the steer did.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        grid: Gd<DotSimCrowdGrid>,
        id: i64,
        body: Gd<Node3D>,
        points: PackedVector3Array,
        s: PackedFloat64Array,
        n: PackedInt64Array,
        apply: bool,
    ) -> PackedFloat64Array {
        let (s, n) = (s.as_slice(), n.as_slice());
        if s.len() < WALK_FLOATS || n.len() < WALK_INTS {
            return PackedFloat64Array::from(&[-1.0]);
        }
        let at = |i: usize| Vec3::new(s[i] as f32, s[i + 1] as f32, s[i + 2] as f32);
        let xf = body.get_global_transform();
        let here = v3(xf.origin);
        let to = at(0);

        self.points.clear();
        self.points.extend(points.as_slice().iter().map(|&p| v3(p)));
        let state = PathState {
            index: n[2],
            arrive_radius: s[23],
            computed_at: s[24],
            goal_at_compute: at(25),
            partial: n[3] != 0,
        };
        let repath = Repath {
            drift: s[28],
            interval: s[29],
            drift_ratio: s[30],
            interval_per_m: s[31],
        };
        let (goal, index) = match walk::path_goal(&self.points, &state, &repath, here, to, s[6]) {
            PathGoal::Due => return PackedFloat64Array::from(&[-2.0]),
            PathGoal::Goal { goal, index } => (goal, index),
        };

        let r = &xf.basis.rows;
        let facing = -Vec3::new(r[0].z, r[1].z, r[2].z).normalized();
        let direction = {
            let g = grid.bind();
            let own = g.grid.index_of(id);
            if own < 0 {
                return PackedFloat64Array::from(&[-1.0]);
            }
            match g.grid.spaced_direction(own, here, goal, s[3], facing) {
                Some(d) => d,
                None => return PackedFloat64Array::from(&[-1.0]),
            }
        };

        let mut t = Turn {
            peak: s[15],
            over_amt: s[16],
            over_axis: at(17),
            turns: n[0],
        };
        let p = TurnProfile {
            view_factor: s[20],
            view_turn_deg: s[21],
            overshoot: s[22],
            seed: n[1],
        };
        let out = steer::steer(
            &mut t,
            &p,
            &SteerIn {
                here,
                facing,
                facing_at_tick: at(7),
                direction,
                to,
                speed: s[4],
                delta: s[5],
                now: s[6],
                progress_at: at(10),
                progress_since: s[13],
                stuck_seconds: s[14],
            },
        );
        self.steered += 1;

        if apply
            && out.steered
            && let Some(target) = out.look
        {
            let mut b = body;
            b.look_at(gv(target));
        }

        let a = answer_of(&out, &t, here);
        let mut answer = [0.0; ANSWER + 4];
        answer[..ANSWER].copy_from_slice(&a);
        answer[ANSWER..].copy_from_slice(&[
            index as f64,
            f64::from(goal.x),
            f64::from(goal.y),
            f64::from(goal.z),
        ]);
        PackedFloat64Array::from(&answer)
    }

    /// `DotNpcAiCharacter.turn_view` for [param character], answered without touching it (what
    /// [method steer_step] turns by; for the parity suites). Returns [code][facing.x, facing.y, facing.z,
    /// peak, over_amt, turns, over_axis.x, over_axis.y, over_axis.z][/code], or [code][-1][/code] when
    /// a member could not be read.
    #[func]
    fn turn_view(
        &mut self,
        character: Gd<Object>,
        current: Vector3,
        wanted: Vector3,
        delta: f64,
    ) -> PackedFloat64Array {
        let names = self.names.get_or_insert_with(Names::new);
        let Some((mut t, p)) = turn_of(names, &character) else {
            return PackedFloat64Array::from(&[-1.0]);
        };
        let f = steer::turn_view(&mut t, &p, v3(current), v3(wanted), delta);
        PackedFloat64Array::from(&[
            f64::from(f.x),
            f64::from(f.y),
            f64::from(f.z),
            t.peak,
            t.over_amt,
            t.turns as f64,
            f64::from(t.over_axis.x),
            f64::from(t.over_axis.y),
            f64::from(t.over_axis.z),
        ])
    }

    /// Steers asked of [method steer_step] since this object was made.
    #[func]
    fn steered(&self) -> i64 {
        self.steered
    }

    /// Survivors in the table.
    #[func]
    fn survivors(&self) -> i32 {
        self.survivors.len() as i32
    }

    /// Plans asked since this object was made.
    #[func]
    fn planned(&self) -> i64 {
        self.planned
    }
}

/// What the steer reads off the brain.
struct BrainWatch {
    facing_at_tick: Vector3,
    progress_at: Vector3,
    progress_since: f64,
    stuck_seconds: f64,
}

impl DotSimZombieBrains {
    /// The steer of [method steer_step] and [method steer_walk] past the brain's members: returns the
    /// answer array, whether it steered, whether the watch reset and where the body stood. Writes the
    /// character and body under [param apply]; the brain is the caller's.
    #[allow(clippy::too_many_arguments)]
    fn steer_core(
        &mut self,
        grid: &Gd<DotSimCrowdGrid>,
        id: i64,
        character: Gd<Object>,
        body: Gd<Node3D>,
        goal: Vector3,
        to: Vector3,
        spacing: f64,
        speed: f64,
        delta: f64,
        now: f64,
        apply: bool,
        w: &BrainWatch,
    ) -> (PackedFloat64Array, bool, bool, Vector3) {
        let names = self.names.get_or_insert_with(Names::new);
        let failed = || {
            (
                PackedFloat64Array::from(&[-1.0]),
                false,
                false,
                Vector3::ZERO,
            )
        };

        let xf = body.get_global_transform();
        let here = v3(xf.origin);
        let r = &xf.basis.rows;
        let facing = -Vec3::new(r[0].z, r[1].z, r[2].z).normalized();

        let direction = {
            let g = grid.bind();
            let index = g.grid.index_of(id);
            if index < 0 {
                return failed();
            }
            match g
                .grid
                .spaced_direction(index, here, v3(goal), spacing, facing)
            {
                Some(d) => d,
                None => return failed(),
            }
        };

        let Some((before, p)) = turn_of(names, &character) else {
            return failed();
        };
        let mut t = before;
        let out = steer::steer(
            &mut t,
            &p,
            &SteerIn {
                here,
                facing,
                facing_at_tick: v3(w.facing_at_tick),
                direction,
                to: v3(to),
                speed,
                delta,
                now,
                progress_at: v3(w.progress_at),
                progress_since: w.progress_since,
                stuck_seconds: w.stuck_seconds,
            },
        );
        self.steered += 1;

        if apply && out.steered {
            if let Some(w) = out.wish {
                let mut b = body.clone().upcast::<Object>();
                b.set(&names.wish, &gv(w).to_variant());
            }
            if t != before {
                let mut c = character;
                if t.peak != before.peak {
                    c.set(&names.peak, &t.peak.to_variant());
                }
                if t.over_amt != before.over_amt {
                    c.set(&names.over_amt, &t.over_amt.to_variant());
                }
                if t.over_axis != before.over_axis {
                    c.set(&names.over_axis, &gv(t.over_axis).to_variant());
                }
                if t.turns != before.turns {
                    c.set(&names.turns, &t.turns.to_variant());
                }
            }
            if let Some(target) = out.look {
                let mut b = body.clone();
                b.look_at(gv(target));
            }
        }

        let answer = PackedFloat64Array::from(&answer_of(&out, &t, here));
        (answer, out.steered, out.watch == Watch::Reset, gv(here))
    }
}

/// [method DotSimZombieBrains.walk]'s [param s] and [param n] lengths.
const WALK_FLOATS: usize = 32;
const WALK_INTS: usize = 4;
/// The length of a steer's answer.
const ANSWER: usize = 19;

/// [method DotSimZombieBrains.steer_step]'s answer for a steer: [code][code, watch, wished, wish xyz,
/// looked, look xyz, peak, over_amt, turns, over_axis xyz, here xyz][/code].
fn answer_of(out: &SteerOut, t: &Turn, here: Vec3) -> [f64; ANSWER] {
    let (wish, look) = (out.wish.unwrap_or_default(), out.look.unwrap_or_default());
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    [
        flag(out.steered),
        match out.watch {
            Watch::Keep => 0.0,
            Watch::Reset => 1.0,
            Watch::Stuck => 2.0,
        },
        flag(out.wish.is_some()),
        f64::from(wish.x),
        f64::from(wish.y),
        f64::from(wish.z),
        flag(out.look.is_some()),
        f64::from(look.x),
        f64::from(look.y),
        f64::from(look.z),
        t.peak,
        t.over_amt,
        t.turns as f64,
        f64::from(t.over_axis.x),
        f64::from(t.over_axis.y),
        f64::from(t.over_axis.z),
        f64::from(here.x),
        f64::from(here.y),
        f64::from(here.z),
    ]
}

/// A character's turn state and tuning, as `DotNpcAiCharacter` keeps them; `None` when one is missing.
fn turn_of(names: &Names, character: &Gd<Object>) -> Option<(Turn, TurnProfile)> {
    Some((
        Turn {
            peak: member::<f64>(character, &names.peak)?,
            over_amt: member::<f64>(character, &names.over_amt)?,
            over_axis: v3(member::<Vector3>(character, &names.over_axis)?),
            turns: member::<i64>(character, &names.turns)?,
        },
        TurnProfile {
            view_factor: member::<f64>(character, &names.view_factor)?,
            view_turn_deg: member::<f64>(character, &names.view_turn_deg)?,
            overshoot: member::<f64>(character, &names.overshoot)?,
            seed: member::<i64>(character, &names.seed)?,
        },
    ))
}
