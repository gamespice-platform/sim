//! A road's centreline samples, to the bit: mg-dangerous-delivery's `DdRouteBase.nearest_index`,
//! `index_at_distance` and the oncoming car's `_car_blocked`.
//!
//! The route writes its samples once after it is built (`points`, `distances`, `yaws` as the
//! engine holds them: `f32`), then every active car asks whether a truck stands in its lane ahead
//! of it. The script asks the nearest sample to each truck in a window round the car's own sample
//! and, when the truck is outside that window (every truck on another part of the road), the whole
//! road: one car and seven far trucks are seven full scans a tick. Every step here is the script's
//! arithmetic: script floats in `f64`, vectors in `f32`, `sin`/`cos` in `f64` rounded into the
//! vector, the packed array's `bsearch` in `f32`.

use gs_sim_math::{Vec3, clampf};

#[cfg(test)]
mod tests;

/// Samples searched either side of a hint before the whole road, as the script's window.
pub const HINT_WINDOW: i64 = 30;

/// The yield rule's numbers: a car stops for a truck less than `ahead` metres ahead of it (and not
/// more than `behind` behind it), nearer its lane's centre than `lane`. The script's
/// `TRAFFIC_YIELD`, `TRAFFIC_LANE` and its literal `-2.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Yield {
    pub behind: f64,
    pub ahead: f64,
    pub lane: f64,
}

impl Default for Yield {
    fn default() -> Self {
        Self {
            behind: -2.0,
            ahead: 14.0,
            lane: 2.3,
        }
    }
}

/// One route's centreline.
#[derive(Clone, Debug, Default)]
pub struct Road {
    pub points: Vec<Vec3>,
    pub distances: Vec<f32>,
    pub yaws: Vec<f32>,
    pub rule: Yield,
}

/// `DdRouteBase.forward_of`: `Vector3(-sin(yaw), 0, -cos(yaw))`, the sine in `f64`.
#[inline]
pub fn forward_of(yaw: f64) -> Vec3 {
    Vec3::new(-yaw.sin() as f32, 0.0, -yaw.cos() as f32)
}

/// `DdRouteBase.right_of`: `Vector3(cos(yaw), 0, -sin(yaw))`.
#[inline]
pub fn right_of(yaw: f64) -> Vec3 {
    Vec3::new(yaw.cos() as f32, 0.0, -yaw.sin() as f32)
}

/// The script's `clampi`: `v < lo ? lo : (v > hi ? hi : v)`.
#[inline]
fn clampi(v: i64, lo: i64, hi: i64) -> i64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

impl Road {
    /// Whether every sample has its distance and yaw.
    pub fn is_whole(&self) -> bool {
        !self.points.is_empty()
            && self.distances.len() == self.points.len()
            && self.yaws.len() == self.points.len()
    }

    /// `length()`: the last distance, 0 for no samples.
    pub fn length(&self) -> f64 {
        self.distances.last().map_or(0.0, |&d| f64::from(d))
    }

    /// `index_at_distance(d)`: `distances.bsearch(clampf(d, 0, length()))` (the first sample at or
    /// past it, compared in `f32`), clamped to the samples.
    pub fn index_at_distance(&self, d: f64) -> i64 {
        let value = clampf(d, 0.0, self.length()) as f32;
        let (mut lo, mut hi) = (0usize, self.distances.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.distances[mid] < value {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        clampi(lo as i64, 0, self.points.len() as i64 - 1)
    }

    /// `nearest_index(position, hint)`: the sample nearest in three dimensions (squared, `f32`,
    /// first of equals), within [`HINT_WINDOW`] of `hint` when it is not negative, the whole road
    /// when the best is at an inner edge of that window.
    pub fn nearest_index(&self, position: Vec3, hint: i64) -> i64 {
        if self.points.is_empty() {
            return 0;
        }
        let last = self.points.len() as i64 - 1;
        let (mut lo, mut hi) = (0i64, last);
        if hint >= 0 {
            lo = (hint - HINT_WINDOW).max(0);
            hi = (hint + HINT_WINDOW).min(last);
        }
        let mut best = lo;
        let mut best_d = f64::INFINITY;
        let mut i = lo;
        while i <= hi {
            let dist = f64::from((position - self.points[i as usize]).length_squared());
            if dist < best_d {
                best_d = dist;
                best = i;
            }
            i += 1;
        }
        if hint >= 0 && ((best == lo && lo > 0) || (best == hi && hi < last)) {
            return self.nearest_index(position, -1);
        }
        best
    }

    /// `_car_blocked(spec, d, trucks)`: whether any truck (route-local) stands in the lane
    /// `lateral` metres right of the centreline within the rule ahead of a car at `d`.
    pub fn car_blocked(&self, d: f64, lateral: f64, trucks: &[Vec3]) -> bool {
        if !self.is_whole() {
            return false;
        }
        let hint = self.index_at_distance(d);
        for &p in trucks {
            let i = self.nearest_index(p, hint) as usize;
            let yaw = f64::from(self.yaws[i]);
            let rel = p - self.points[i];
            let along = f64::from(self.distances[i]) + f64::from(forward_of(yaw).dot(rel));
            let side = f64::from(right_of(yaw).dot(rel));
            let ahead = d - along;
            if ahead > self.rule.behind
                && ahead < self.rule.ahead
                && (side - lateral).abs() < self.rule.lane
            {
                return true;
            }
        }
        false
    }
}
