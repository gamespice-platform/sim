//! `GSSimSpawnPick`: gs-spawn's scatter weights and draw and a game's site occupancy, natively
//! (`gs_sim_spawn`). Stateless; the script keeps the rng, so the draw stays the script's stream.

use godot::prelude::*;
use gs_sim_spawn::Scatter;

fn pts(a: &PackedVector3Array) -> Vec<[f32; 3]> {
    a.as_slice().iter().map(|v| [v.x, v.y, v.z]).collect()
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimSpawnPick {}

#[godot_api]
impl GSSimSpawnPick {
    /// `GSSpawnScoring.pick_scatter`'s weights for [param sites] (their own weights in
    /// [param weights]), damped near [param recent] picks and [param enemies]; one more value at
    /// the end is the sum. All zero falls back to the sites' own weights, as the script does.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn scatter_weights(
        &self,
        sites: PackedVector3Array,
        weights: PackedFloat64Array,
        recent: PackedVector3Array,
        enemies: PackedVector3Array,
        min_gap: f64,
        radius: f64,
        enemy_radius: f64,
    ) -> PackedFloat64Array {
        let rules = Scatter {
            min_gap,
            radius,
            enemy_radius,
        };
        let out = gs_sim_spawn::scatter_weights(
            &rules,
            &pts(&sites),
            weights.as_slice(),
            &pts(&recent),
            &pts(&enemies),
        );
        PackedFloat64Array::from(out.as_slice())
    }

    /// The site index [param roll] (`rng.randf() * sum`) lands on in [method scatter_weights]'s
    /// output, or -1 for no sites.
    #[func]
    fn draw(&self, weights: PackedFloat64Array, roll: f64) -> i64 {
        if weights.len() < 2 {
            return -1;
        }
        gs_sim_spawn::draw(weights.as_slice(), roll) as i64
    }

    /// How many of [param players] stand within [param radius] of each of [param sites].
    #[func]
    fn occupancy(
        &self,
        sites: PackedVector3Array,
        players: PackedVector3Array,
        radius: f64,
    ) -> PackedInt32Array {
        let out = gs_sim_spawn::occupancy(&pts(&sites), &pts(&players), radius);
        PackedInt32Array::from(out.as_slice())
    }
}
