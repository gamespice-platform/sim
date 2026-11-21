//! `GSSimRoad`: a route's centreline samples, mg-dangerous-delivery's `DdRouteBase.nearest_index`,
//! `index_at_distance` and the oncoming car's `_car_blocked`.
//!
//! The route writes its samples once after it is built (`set_samples`) and the yield rule's
//! numbers (`set_yield`), then each active car asks `car_blocked` once a tick with the trucks'
//! route-local positions: one call instead of a nearest-sample search per truck in script (the
//! whole road for every truck outside the car's window). The same answer as the script to the bit
//! (`gs_sim_road`; the game's `headless_native_traffic` and its `traffic_verify` hold the two
//! together).

use gs_sim_math::Vec3;
use gs_sim_road::{Road, Yield};
use godot::prelude::*;

/// One route's centreline and the asks over it.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimRoad {
    road: Road,
    trucks: Vec<Vec3>,
}

#[godot_api]
impl GSSimRoad {
    /// Holds the route's samples: [param points], [param distances] and [param yaws], one each per
    /// sample. Counts that differ are an error and hold nothing (every ask then answers as an empty
    /// road).
    #[func]
    fn set_samples(
        &mut self,
        points: PackedVector3Array,
        distances: PackedFloat32Array,
        yaws: PackedFloat32Array,
    ) -> bool {
        self.road.points.clear();
        self.road.distances.clear();
        self.road.yaws.clear();
        if distances.len() != points.len() || yaws.len() != points.len() {
            godot_error!(
                "GSSimRoad.set_samples: {} points, {} distances, {} yaws",
                points.len(),
                distances.len(),
                yaws.len()
            );
            return false;
        }
        self.road.points = points.as_slice().iter().map(|p| v3(*p)).collect();
        self.road.distances = distances.as_slice().to_vec();
        self.road.yaws = yaws.as_slice().to_vec();
        true
    }

    /// How many samples are held.
    #[func]
    fn count(&self) -> i64 {
        self.road.points.len() as i64
    }

    /// The yield rule: a car stops for a truck between [param behind] and [param ahead] metres
    /// ahead of it, nearer its lane's centre than [param lane].
    #[func]
    fn set_yield(&mut self, behind: f64, ahead: f64, lane: f64) {
        self.road.rule = Yield {
            behind,
            ahead,
            lane,
        };
    }

    /// `index_at_distance(d)`.
    #[func]
    fn index_at_distance(&self, d: f64) -> i64 {
        self.road.index_at_distance(d)
    }

    /// `nearest_index(position, hint)`.
    #[func]
    fn nearest_index(&self, position: Vector3, hint: i64) -> i64 {
        self.road.nearest_index(v3(position), hint)
    }

    /// `_car_blocked`: whether one of [param trucks] (route-local) stands in the lane
    /// [param lateral] metres right of the centreline within the rule ahead of a car at [param d].
    #[func]
    fn car_blocked(&mut self, d: f64, lateral: f64, trucks: PackedVector3Array) -> bool {
        self.trucks.clear();
        self.trucks.extend(trucks.as_slice().iter().map(|p| v3(*p)));
        self.road.car_blocked(d, lateral, &self.trucks)
    }
}

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}
