//! `GSSimZombieBrains`: Zombie Zone's zombie brains in Rust (`gs_sim_zombie`). The horde pushes its
//! survivor facts and chase rules once a tick ([method set_survivors], [method set_rules]); each common's
//! think then asks [method plan_chase] once for what `ZzCommonBrain._chase_plan_script` reads in five
//! script calls, to the bit. The spacing half of a zombie's steer is [code]GSSimCrowdGrid.spaced_direction[/code];
//! [method steer_step] is the whole steer after the path (spacing, wish, turn, look, progress watch) in one call,
//! and [method walk] the whole move: the path's next corner, then that steer, with every member handed in.
//! [method chase] is the plan and the chase's choice in one; [method nearest] and [method barricade_in_way] answer
//! the think's two other questions of the horde off the same tick's tables.

use crate::crowd::GSSimCrowdGrid;
use godot::classes::Node3D;
use godot::prelude::*;
use gs_sim_math::Vec3;
use gs_sim_zombie::decide::{self, Barricade, Barricades};
use gs_sim_zombie::steer::{self, SteerIn, SteerOut, Turn, TurnProfile, Watch};
use gs_sim_zombie::walk::{self, PathGoal, PathState, Repath};
use gs_sim_zombie::{self as zombie, Chaser, Rules, Survivor, Who};
use std::collections::HashMap;

/// `set_survivors` flag bits.
const ALIVE: i32 = 1;
const DOWN: i32 = 2;

/// [method GSSimZombieBrains.chase] flag bits.
const BURNING: i32 = 1;
const CRAWLER: i32 = 2;
const SWINGING: i32 = 4;

/// The tick's survivor table and chase rules, and the plans asked of them.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimZombieBrains {
    ids: Vec<StringName>,
    survivors: Vec<Survivor>,
    rules: Rules,
    empty: StringName,
    planned: i64,
    names: Option<Names>,
    steered: i64,
    /// [method walk]'s path points, reused.
    points: Vec<Vec3>,
    /// The intact barricades ([method set_barricades]) and their ids.
    barricades: Barricades,
    barricade_ids: Vec<GString>,
    /// The per-zombie store ([method store_put]): each brain's turn and progress watch, kept here
    /// between thinks so [method walk_stored], [method steer_stored] and [method face_stored] read
    /// and write them without crossing for them. Keyed by the brain's instance id.
    store: HashMap<i64, Stored>,
    /// The store's size at which the next put sweeps out freed brains (doubles with the store; no cap).
    sweep_at: usize,
    /// The spawner's repath tuning ([method set_repath]), which [method walk_stored] paths by.
    repath: Option<Repath>,
}

/// One zombie's state held in [member GSSimZombieBrains.store]: its character's turn state and
/// tuning, and its brain's progress watch.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stored {
    turn: Turn,
    profile: TurnProfile,
    progress_at: Vec3,
    progress_since: f64,
    stuck_seconds: f64,
}

/// [method GSSimZombieBrains.store_put]'s [param s] and [method GSSimZombieBrains.walk_stored]'s lengths.
const STORE_FLOATS: usize = 13;
const WALK_STORED_FLOATS: usize = 15;
const WALK_STORED_INTS: usize = 2;
/// The store's first sweep size.
const SWEEP_FIRST: usize = 64;

/// The script members [method GSSimZombieBrains.steer_step] reads and writes, named once.
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

impl GSSimZombieBrains {
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
impl GSSimZombieBrains {
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
                "GSSimZombieBrains.set_survivors: {} ids, {} positions, {} flags",
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

    /// One common's whole chase choice (`ZzCommonBrain._chase`): [method plan_chase]'s eight values, then what it does
    /// as if it attacks (0 nothing, 1 halt and swing, 2 run at it, 3 drift at a third of the speed) and 1 when it must
    /// first be granted an attack slot (refused, it holds [param ring] instead). [param flags]: 1 burning, 2 crawler,
    /// 4 mid-swing.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn chase(
        &mut self,
        forced: StringName,
        target: StringName,
        pace: f64,
        roll: f64,
        reach: f64,
        ring: f64,
        flags: i32,
        here: Vector3,
    ) -> PackedFloat64Array {
        self.planned += 1;
        let z = Chaser {
            forced: self.who(&forced),
            target: self.who(&target),
            pace,
            roll,
            reach,
            burning: flags & BURNING != 0,
            crawler: flags & CRAWLER != 0,
            here: v3(here),
        };
        let d = decide::decide_chase(
            &self.survivors,
            &self.rules,
            &z,
            ring,
            flags & SWINGING != 0,
        );
        let p = d.plan;
        PackedFloat64Array::from(&[
            f64::from(p.code),
            if p.dropped { 1.0 } else { 0.0 },
            f64::from(p.at.x),
            f64::from(p.at.y),
            f64::from(p.at.z),
            p.speed,
            p.gap,
            p.reach,
            f64::from(d.act),
            if d.ask { 1.0 } else { 0.0 },
        ])
    }

    /// The flat metres from [param at] to the nearest living survivor of the tick, INF with none
    /// (`ZzHorde.nearest_survivor_distance`).
    #[func]
    fn nearest(&self, at: Vector3) -> f64 {
        decide::nearest(&self.survivors, v3(at))
    }

    /// The intact barricades: [param ids] in the horde's order, each box's position and size, the grid's
    /// [param cell] in metres and the bash [param reach]. Arrays of different sizes or a cell not above zero
    /// are an error and empty the table.
    #[func]
    fn set_barricades(
        &mut self,
        ids: PackedStringArray,
        position: PackedVector3Array,
        size: PackedVector3Array,
        cell: f64,
        reach: f64,
    ) -> bool {
        self.barricades = Barricades::default();
        self.barricade_ids.clear();
        let (ids, position, size) = (ids.as_slice(), position.as_slice(), size.as_slice());
        if ids.len() != position.len() || ids.len() != size.len() {
            godot_error!(
                "GSSimZombieBrains.set_barricades: {} ids, {} positions, {} sizes",
                ids.len(),
                position.len(),
                size.len()
            );
            return false;
        }
        let boxes = position
            .iter()
            .zip(size)
            .map(|(p, s)| Barricade {
                position: v3(*p),
                size: v3(*s),
            })
            .collect();
        let Some(table) = Barricades::new(boxes, cell, reach) else {
            godot_error!("GSSimZombieBrains.set_barricades: cell {cell} is not above zero");
            return false;
        };
        self.barricades = table;
        self.barricade_ids = ids.to_vec();
        true
    }

    /// The id of the barricade a zombie at [param at] wishing [param wish] is pressed against, or "" (`ZzHorde.barricade_in_way`).
    #[func]
    fn barricade_in_way(&self, at: Vector3, wish: Vector3) -> GString {
        self.barricades
            .in_way(v3(at), v3(wish))
            .and_then(|i| self.barricade_ids.get(i).cloned())
            .unwrap_or_default()
    }

    /// How many barricades [method set_barricades] holds.
    #[func]
    fn barricades(&self) -> i32 {
        i32::try_from(self.barricades.len()).unwrap_or(i32::MAX)
    }

    /// The rest of a zombie's steer once its path gave [param goal] (gs-npc-ai's `steer_with_spacing` after
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
        grid: Gd<GSSimCrowdGrid>,
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
        grid: Gd<GSSimCrowdGrid>,
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
    /// here: gs-npc's `path_toward` on a path not due a repath (its repath test and advance, the
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
        grid: Gd<GSSimCrowdGrid>,
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

    /// `GSNpcAiCharacter.turn_view` for [param character], answered without touching it (what
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

    /// Puts one zombie in the store under [param key] (its brain's instance id), replacing what was
    /// there: [param s] is [code][peak, over_amt, over_axis.x, .y, .z, view_factor, view_turn_deg,
    /// overshoot, progress_at.x, .y, .z, progress_since, stuck_seconds][/code] (its character's turn
    /// state and tuning, its brain's watch), with [param turns] and [param seed] the character's
    /// `_turns` and `seed_value`. While stored, the store is the truth for those and the script's
    /// members are not read; [method store_take] hands them back. A put that grows the store past its
    /// sweep size first drops the entries of freed brains. False (nothing stored) when [param s] is short.
    #[func]
    fn store_put(&mut self, key: i64, s: PackedFloat64Array, turns: i64, seed: i64) -> bool {
        let s = s.as_slice();
        if s.len() < STORE_FLOATS {
            godot_error!(
                "GSSimZombieBrains.store_put: {} floats, {} wanted",
                s.len(),
                STORE_FLOATS
            );
            return false;
        }
        let at = |i: usize| Vec3::new(s[i] as f32, s[i + 1] as f32, s[i + 2] as f32);
        let entry = Stored {
            turn: Turn {
                peak: s[0],
                over_amt: s[1],
                over_axis: at(2),
                turns,
            },
            profile: TurnProfile {
                view_factor: s[5],
                view_turn_deg: s[6],
                overshoot: s[7],
                seed,
            },
            progress_at: at(8),
            progress_since: s[11],
            stuck_seconds: s[12],
        };
        if !self.store.contains_key(&key) && self.store.len() >= self.sweep_at.max(SWEEP_FIRST) {
            self.store.retain(|&k, _| {
                InstanceId::try_from_i64(k)
                    .is_some_and(|id| Gd::<Object>::try_from_instance_id(id).is_ok())
            });
            self.sweep_at = (self.store.len() * 2).max(SWEEP_FIRST);
        }
        self.store.insert(key, entry);
        true
    }

    /// What the store holds for [param key], as the script's members take it back: [code][peak,
    /// over_amt, over_axis.x, .y, .z, turns, progress_at.x, .y, .z, progress_since][/code], or empty
    /// when nothing is stored. With [param drop] the entry leaves the store.
    #[func]
    fn store_take(&mut self, key: i64, drop: bool) -> PackedFloat64Array {
        let e = if drop {
            self.store.remove(&key)
        } else {
            self.store.get(&key).copied()
        };
        let Some(e) = e else {
            return PackedFloat64Array::new();
        };
        let t = &e.turn;
        PackedFloat64Array::from(&[
            t.peak,
            t.over_amt,
            f64::from(t.over_axis.x),
            f64::from(t.over_axis.y),
            f64::from(t.over_axis.z),
            t.turns as f64,
            f64::from(e.progress_at.x),
            f64::from(e.progress_at.y),
            f64::from(e.progress_at.z),
            e.progress_since,
        ])
    }

    /// Zombies in the store (freed brains included until the next sweep).
    #[func]
    fn stored(&self) -> i64 {
        self.store.len() as i64
    }

    /// The spawner's repath tuning, which [method walk_stored] paths by (the horde's feed sets it each tick).
    #[func]
    fn set_repath(&mut self, drift: f64, interval: f64, drift_ratio: f64, interval_per_m: f64) {
        self.repath = Some(Repath {
            drift,
            interval,
            drift_ratio,
            interval_per_m,
        });
    }

    /// [method walk] with the turn and watch read from the store under [param key], not handed in.
    /// [param s]: [code][to.x, to.y, to.z, spacing, speed, delta, now, facing_at_tick.x, .y, .z,
    /// arrive_radius, computed_at, goal_at_compute.x, .y, .z][/code]; [param n]: [code][index,
    /// partial][/code]. Under [param apply] the stored turn is updated, the watch restarted from
    /// `here` at now on a reset or a stuck path (the caller puts the path rough on the latter), and
    /// the body's look written; the caller writes the path's index, the waypoint, the wish and the cruise.
    /// Without it nothing is changed. The answer is [method walk]'s; [code][-3][/code] when nothing is
    /// stored for [param key] or no repath tuning was set.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn walk_stored(
        &mut self,
        grid: Gd<GSSimCrowdGrid>,
        id: i64,
        key: i64,
        body: Gd<Node3D>,
        points: PackedVector3Array,
        s: PackedFloat64Array,
        n: PackedInt64Array,
        apply: bool,
    ) -> PackedFloat64Array {
        let (s, n) = (s.as_slice(), n.as_slice());
        if s.len() < WALK_STORED_FLOATS || n.len() < WALK_STORED_INTS {
            return PackedFloat64Array::from(&[-1.0]);
        }
        let (Some(e), Some(repath)) = (self.store.get(&key).copied(), self.repath) else {
            return PackedFloat64Array::from(&[-3.0]);
        };
        let at = |i: usize| Vec3::new(s[i] as f32, s[i + 1] as f32, s[i + 2] as f32);
        let (here, facing) = pose(&body);
        let to = at(0);

        self.points.clear();
        self.points.extend(points.as_slice().iter().map(|&p| v3(p)));
        let state = PathState {
            index: n[0],
            arrive_radius: s[10],
            computed_at: s[11],
            goal_at_compute: at(12),
            partial: n[1] != 0,
        };
        let (goal, index) = match walk::path_goal(&self.points, &state, &repath, here, to, s[6]) {
            PathGoal::Due => return PackedFloat64Array::from(&[-2.0]),
            PathGoal::Goal { goal, index } => (goal, index),
        };
        let Some(direction) = spaced(&grid, id, here, goal, s[3], facing) else {
            return PackedFloat64Array::from(&[-1.0]);
        };

        let mut t = e.turn;
        let out = steer::steer(
            &mut t,
            &e.profile,
            &SteerIn {
                here,
                facing,
                facing_at_tick: at(7),
                direction,
                to,
                speed: s[4],
                delta: s[5],
                now: s[6],
                progress_at: e.progress_at,
                progress_since: e.progress_since,
                stuck_seconds: e.stuck_seconds,
            },
        );
        self.steered += 1;

        if apply && out.steered {
            self.keep(key, &e, t, out.watch != Watch::Keep, here, s[6]);
            if let Some(target) = out.look {
                let mut b = body;
                b.look_at(gv(target));
            }
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

    /// [method steer_walk] with the turn and watch read from the store under [param key]. Under
    /// [param apply] the body's wish and look are written and the stored turn updated, and the watch
    /// restarted from `here` at [param now] on a reset, or on a stuck path when [param rough] (the
    /// brain's path can go rough; the caller does that); the caller writes the cruise. Without it
    /// nothing is changed. The answer is [method steer_step]'s; [code][-1][/code] when [param id] is
    /// not in the grid, [code][-3][/code] when nothing is stored for [param key].
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn steer_stored(
        &mut self,
        grid: Gd<GSSimCrowdGrid>,
        id: i64,
        key: i64,
        body: Gd<Node3D>,
        goal: Vector3,
        to: Vector3,
        spacing: f64,
        speed: f64,
        delta: f64,
        now: f64,
        facing_at_tick: Vector3,
        rough: bool,
        apply: bool,
    ) -> PackedFloat64Array {
        let Some(e) = self.store.get(&key).copied() else {
            return PackedFloat64Array::from(&[-3.0]);
        };
        let (here, facing) = pose(&body);
        let Some(direction) = spaced(&grid, id, here, v3(goal), spacing, facing) else {
            return PackedFloat64Array::from(&[-1.0]);
        };
        let mut t = e.turn;
        let out = steer::steer(
            &mut t,
            &e.profile,
            &SteerIn {
                here,
                facing,
                facing_at_tick: v3(facing_at_tick),
                direction,
                to: v3(to),
                speed,
                delta,
                now,
                progress_at: e.progress_at,
                progress_since: e.progress_since,
                stuck_seconds: e.stuck_seconds,
            },
        );
        self.steered += 1;

        if apply && out.steered {
            let restart = out.watch == Watch::Reset || (rough && out.watch == Watch::Stuck);
            self.keep(key, &e, t, restart, here, now);
            if let Some(w) = out.wish {
                let names = self.names.get_or_insert_with(Names::new);
                let mut b = body.clone().upcast::<Object>();
                b.set(&names.wish, &gv(w).to_variant());
            }
            if let Some(target) = out.look {
                let mut b = body;
                b.look_at(gv(target));
            }
        }
        PackedFloat64Array::from(&answer_of(&out, &t, here))
    }

    /// gs-npc-ai's `face(direction)` on a living body whose character is stored under [param key]:
    /// the turn from [param facing_at_tick] (or where the body faces, when unset) and the look. Under
    /// [param apply] the stored turn is updated and the body turned. Returns [code][looked, look.x,
    /// .y, .z, peak, over_amt, turns, over_axis.x, .y, .z][/code], or [code][-3][/code] when nothing
    /// is stored for [param key].
    #[func]
    fn face_stored(
        &mut self,
        key: i64,
        body: Gd<Node3D>,
        direction: Vector3,
        facing_at_tick: Vector3,
        delta: f64,
        apply: bool,
    ) -> PackedFloat64Array {
        let Some(e) = self.store.get_mut(&key) else {
            return PackedFloat64Array::from(&[-3.0]);
        };
        let (here, facing) = pose(&body);
        let mut t = e.turn;
        let look = steer::face(
            &mut t,
            &e.profile,
            here,
            facing,
            v3(facing_at_tick),
            v3(direction),
            delta,
        );
        if apply {
            e.turn = t;
            if let Some(target) = look {
                let mut b = body;
                b.look_at(gv(target));
            }
        }
        let l = look.unwrap_or_default();
        PackedFloat64Array::from(&[
            if look.is_some() { 1.0 } else { 0.0 },
            f64::from(l.x),
            f64::from(l.y),
            f64::from(l.z),
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

impl GSSimZombieBrains {
    /// A steer's writes to the stored entry under [param key] (read as [param e]): the turn, and the
    /// watch restarted from [param here] at [param now] when [param restart].
    fn keep(&mut self, key: i64, e: &Stored, t: Turn, restart: bool, here: Vec3, now: f64) {
        let mut kept = *e;
        kept.turn = t;
        if restart {
            kept.progress_at = here;
            kept.progress_since = now;
        }
        self.store.insert(key, kept);
    }
}

/// Where [param body] stands and faces (`npc.position()`, `npc.facing()`).
fn pose(body: &Gd<Node3D>) -> (Vec3, Vec3) {
    let xf = body.get_global_transform();
    let r = &xf.basis.rows;
    (
        v3(xf.origin),
        -Vec3::new(r[0].z, r[1].z, r[2].z).normalized(),
    )
}

/// The crowd grid's spaced heading for zombie [param id], or `None` when it is not in the grid.
fn spaced(
    grid: &Gd<GSSimCrowdGrid>,
    id: i64,
    here: Vec3,
    goal: Vec3,
    spacing: f64,
    facing: Vec3,
) -> Option<Vec3> {
    let g = grid.bind();
    let own = g.grid.index_of(id);
    if own < 0 {
        return None;
    }
    g.grid.spaced_direction(own, here, goal, spacing, facing)
}

/// What the steer reads off the brain.
struct BrainWatch {
    facing_at_tick: Vector3,
    progress_at: Vector3,
    progress_since: f64,
    stuck_seconds: f64,
}

impl GSSimZombieBrains {
    /// The steer of [method steer_step] and [method steer_walk] past the brain's members: returns the
    /// answer array, whether it steered, whether the watch reset and where the body stood. Writes the
    /// character and body under [param apply]; the brain is the caller's.
    #[allow(clippy::too_many_arguments)]
    fn steer_core(
        &mut self,
        grid: &Gd<GSSimCrowdGrid>,
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

/// [method GSSimZombieBrains.walk]'s [param s] and [param n] lengths.
const WALK_FLOATS: usize = 32;
const WALK_INTS: usize = 4;
/// The length of a steer's answer.
const ANSWER: usize = 19;

/// [method GSSimZombieBrains.steer_step]'s answer for a steer: [code][code, watch, wished, wish xyz,
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

/// A character's turn state and tuning, as `GSNpcAiCharacter` keeps them; `None` when one is missing.
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
