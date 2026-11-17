//! `DotSimZombieBrains`: Zombie Zone's zombie brains in Rust (`dot_sim_zombie`). The horde pushes its
//! survivor facts and chase rules once a tick ([method set_survivors], [method set_rules]); each common's
//! think then asks [method plan_chase] once for what `ZzCommonBrain._chase_plan_script` reads in five
//! script calls, to the bit. The spacing half of a zombie's steer is [code]DotSimCrowdGrid.spaced_direction[/code];
//! [method steer_step] is the whole steer after the path (spacing, wish, turn, look, progress watch) in one call.

use crate::crowd::DotSimCrowdGrid;
use dot_sim_math::Vec3;
use dot_sim_zombie::steer::{self, SteerIn, Turn, TurnProfile, Watch};
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
        let failed = || PackedFloat64Array::from(&[-1.0]);

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

        let (Some(facing_at_tick), Some(progress_at), Some(progress_since), Some(stuck_seconds)) = (
            member::<Vector3>(&brain, &names.facing_at_tick),
            member::<Vector3>(&brain, &names.progress_at),
            member::<f64>(&brain, &names.progress_since),
            member::<f64>(&brain, &names.stuck_seconds),
        ) else {
            return failed();
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
                facing_at_tick: v3(facing_at_tick),
                direction,
                to: v3(to),
                speed,
                delta,
                now,
                progress_at: v3(progress_at),
                progress_since,
                stuck_seconds,
            },
        );
        self.steered += 1;

        if apply && out.steered {
            let mut brain = brain;
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
            brain.set(&names.cruise_to, &goal.to_variant());
            brain.set(&names.cruise_dest, &to.to_variant());
            if out.watch == Watch::Reset {
                brain.set(&names.progress_at, &gv(here).to_variant());
                brain.set(&names.progress_since, &now.to_variant());
            }
        }

        let (wish, look) = (out.wish.unwrap_or_default(), out.look.unwrap_or_default());
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        PackedFloat64Array::from(&[
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
        ])
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
