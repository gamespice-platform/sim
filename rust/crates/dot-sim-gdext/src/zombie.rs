//! `DotSimZombieBrains`: Zombie Zone's zombie brains in Rust (`dot_sim_zombie`). The horde pushes its
//! survivor facts and chase rules once a tick ([method set_survivors], [method set_rules]); each common's
//! think then asks [method plan_chase] once for what `ZzCommonBrain._chase_plan_script` reads in five
//! script calls, to the bit. The spacing half of a zombie's steer is [code]DotSimCrowdGrid.spaced_direction[/code].

use dot_sim_math::Vec3;
use dot_sim_zombie::{self as zombie, Chaser, Rules, Survivor, Who};
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
