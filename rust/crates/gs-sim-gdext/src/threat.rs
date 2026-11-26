//! `GSSimBotThreat`: Zombie Zone's stand-in survivors' look at the crowd (`zz_bots.gd` `_threat` and
//! `_crowd_within`) off the crowd grid, with no script call per zombie. The horde registers each zombie's
//! height and kind once when it spawns ([method register]); [method rank] then answers which crowd indices
//! the bot would rather shoot, [method commons] how many commons stand within reach. The sight rays stay in
//! the script, which tests the ranked ones in order.
//!
//! Positions are the grid's (this tick's rebuild); the script read each zombie's live position.

use crate::crowd::GSSimCrowdGrid;
use godot::prelude::*;
use gs_sim_bots::threat::{self, Cand, Table};
use gs_sim_bots::Vec3 as V3;

fn v3(p: Vector3) -> V3 {
    V3::new(p.x, p.y, p.z)
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBotThreat {
    table: Table,
    near: Vec<i32>,
    cands: Vec<Cand>,
    top: Vec<i32>,
    calm: bool,
}

#[godot_api]
impl GSSimBotThreat {
    /// Flag bit: a plain common.
    #[constant]
    const COMMON: i64 = threat::COMMON as i64;
    /// Flag bit: any kind but the common.
    #[constant]
    const SPECIAL: i64 = threat::SPECIAL as i64;
    /// Flag bit: a sleeper still sitting.
    #[constant]
    const SITTING: i64 = threat::SITTING as i64;

    /// Notes zombie [param id]: its body [param height] in metres and its [param flags].
    #[func]
    fn register(&mut self, id: i64, height: f64, flags: i64) {
        self.table.set(id, height, flags as u8);
    }

    #[func]
    fn forget(&mut self, id: i64) {
        self.table.remove(id);
    }

    /// Sets or clears the sitting flag of [param id] (a sleeper); false when it is not registered.
    #[func]
    fn set_sitting(&mut self, id: i64, on: bool) -> bool {
        self.table.set_sitting(id, on)
    }

    #[func]
    fn clear(&mut self) {
        self.table.clear();
    }

    #[func]
    fn registered(&self) -> i32 {
        self.table.len() as i32
    }

    /// The crowd indices of the (at most ten) zombies a bot at [param feet], eyes at [param eye], would
    /// rather shoot among those within [param radius] of its feet, best first. A special counts twelve
    /// metres closer, a sitting sleeper never. [method calm] then says whether nobody is within
    /// [param calm_radius].
    #[func]
    fn rank(
        &mut self,
        grid: Gd<GSSimCrowdGrid>,
        feet: Vector3,
        eye: Vector3,
        radius: f64,
        calm_radius: f64,
    ) -> PackedInt32Array {
        let g = &grid.bind().grid;
        g.near(v3(feet), radius, &mut self.near);
        self.cands.clear();
        for &i in &self.near {
            // A zombie nobody registered is no threat the script could rank.
            if let Some(info) = self.table.get(g.id_of(i)) {
                self.cands.push(Cand {
                    index: i,
                    at: g.position_of(i),
                    info,
                });
            }
        }
        self.calm = threat::rank(&self.cands, v3(feet), v3(eye), calm_radius, &mut self.top);
        PackedInt32Array::from(self.top.as_slice())
    }

    /// Whether the last [method rank] found no zombie within its calm radius.
    #[func]
    fn calm(&self) -> bool {
        self.calm
    }

    /// How many commons stand within [param reach] of [param point].
    #[func]
    fn commons(&mut self, grid: Gd<GSSimCrowdGrid>, point: Vector3, reach: f64) -> i32 {
        let g = &grid.bind().grid;
        g.near(v3(point), reach, &mut self.near);
        threat::commons(self.near.iter().filter_map(|&i| self.table.get(g.id_of(i))))
    }
}
