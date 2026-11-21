//! The path half of a zombie's move: gs-npc's `GSNpcPathing.path_toward` on a path that is not due a
//! repath (its repath test, `GSNpcPath.needs_repath`, then `GSNpcPath.advance`), so the steer after it
//! ([`crate::steer`]) has its goal without a script call.
//!
//! Pure: [`path_goal`] takes the path's state and returns the goal and where the path's index ends up,
//! or that the path is due (the caller then asks the script, which queues or searches as it always has),
//! in the script's order and precision (engine vectors `f32`, script floats `f64`).

use gs_sim_math::{Vec3, maxf};

/// A `GSNpcPath` as `path_toward` reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathState {
    /// `index`.
    pub index: i64,
    /// `arrive_radius`.
    pub arrive_radius: f64,
    /// `computed_at`.
    pub computed_at: f64,
    /// `goal_at_compute`.
    pub goal_at_compute: Vec3,
    /// `partial`.
    pub partial: bool,
}

/// The spawner's repath tuning: `repath_drift`, `repath_interval`, `repath_drift_ratio`,
/// `repath_interval_per_m`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repath {
    pub drift: f64,
    pub interval: f64,
    pub drift_ratio: f64,
    pub interval_per_m: f64,
}

/// What `path_toward` decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathGoal {
    /// The path wants recomputing: the script's `path_toward` runs (it queues the search or does it).
    Due,
    /// Steer for `goal`; the path's index is now `index`.
    Goal { goal: Vec3, index: i64 },
}

/// `GSNpcPath.is_finished`.
#[inline]
fn finished(points: &[Vec3], index: i64) -> bool {
    points.is_empty() || index >= points.len() as i64
}

/// `GSNpcPath.needs_repath`.
pub fn needs_repath(
    points: &[Vec3],
    p: &PathState,
    now: f64,
    goal: Vec3,
    interval: f64,
    drift: f64,
) -> bool {
    if points.is_empty() {
        return true;
    }
    if finished(points, p.index) && !p.partial {
        return true;
    }
    if interval > 0.0 && now - p.computed_at >= interval {
        return true;
    }
    drift > 0.0 && f64::from(goal.distance_to(p.goal_at_compute)) > drift
}

/// `GSNpcPath.advance(position, fallback)`: past every point `here` is within the arrive radius of
/// (measured flat), returning the next one and the index it stopped at, or `fallback` at the end.
/// `None` for a negative index, which the script reads from the end of the array: the caller lets the
/// script walk such a path.
pub fn advance(
    points: &[Vec3],
    index: i64,
    arrive_radius: f64,
    here: Vec3,
    fallback: Vec3,
) -> Option<(Vec3, i64)> {
    if index < 0 {
        return None;
    }
    let mut index = index;
    while !finished(points, index) {
        let at = points[index as usize];
        let mut to_next = at - here;
        to_next.y = 0.0;
        if f64::from(to_next.length()) > arrive_radius {
            return Some((at, index));
        }
        index += 1;
    }
    Some((fallback, index))
}

/// `path_toward(npc, path, goal)` with a nav graph, a live NPC at `here` and a path: the repath test
/// with the spawner's distance-scaled drift and interval, then the advance. `now` is the spawner's
/// clock. `Due` too for a path the script would have to read backwards (a negative index).
pub fn path_goal(
    points: &[Vec3],
    p: &PathState,
    r: &Repath,
    here: Vec3,
    goal: Vec3,
    now: f64,
) -> PathGoal {
    let mut drift = r.drift;
    let mut interval = r.interval;
    if r.drift_ratio > 0.0 || r.interval_per_m > 0.0 {
        let gap = f64::from(here.distance_to(goal));
        drift = maxf(drift, r.drift_ratio * gap);
        interval = maxf(interval, r.interval_per_m * gap);
    }
    if needs_repath(points, p, now, goal, interval, drift) {
        return PathGoal::Due;
    }
    // needs_repath is true for an empty path, so there are points here.
    match advance(points, p.index, p.arrive_radius, here, goal) {
        Some((goal, index)) => PathGoal::Goal { goal, index },
        None => PathGoal::Due,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn state(index: i64) -> PathState {
        PathState {
            index,
            arrive_radius: 1.2,
            computed_at: 10.0,
            goal_at_compute: v(20.0, 0.0, 0.0),
            partial: false,
        }
    }

    const R: Repath = Repath {
        drift: 2.5,
        interval: 1.5,
        drift_ratio: 0.25,
        interval_per_m: 0.1,
    };

    #[test]
    fn empty_and_finished_paths_are_due() {
        let p = state(0);
        assert!(needs_repath(&[], &p, 10.0, v(20.0, 0.0, 0.0), 1.5, 2.5));
        let pts = [v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0)];
        assert!(needs_repath(
            &pts,
            &state(2),
            10.0,
            v(20.0, 0.0, 0.0),
            1.5,
            2.5
        ));
        // A partial path walked to its end waits.
        let partial = PathState {
            partial: true,
            ..state(2)
        };
        assert!(!needs_repath(
            &pts,
            &partial,
            10.0,
            v(20.0, 0.0, 0.0),
            1.5,
            2.5
        ));
    }

    #[test]
    fn interval_and_drift_trigger() {
        let pts = [v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0)];
        let p = state(1);
        assert!(!needs_repath(&pts, &p, 11.0, v(20.0, 0.0, 0.0), 1.5, 2.5));
        assert!(needs_repath(&pts, &p, 11.5, v(20.0, 0.0, 0.0), 1.5, 2.5));
        assert!(needs_repath(&pts, &p, 11.0, v(23.0, 0.0, 0.0), 1.5, 2.5));
        assert!(!needs_repath(&pts, &p, 11.0, v(23.0, 0.0, 0.0), 1.5, 0.0));
    }

    #[test]
    fn advance_passes_reached_points_flat() {
        let pts = [v(0.0, 0.0, 0.0), v(1.0, 5.0, 0.0), v(6.0, 0.0, 0.0)];
        let far = v(9.0, 9.0, 9.0);
        // The second point is 5 m up but 1 m away flat: reached.
        assert_eq!(
            advance(&pts, 1, 1.2, v(0.0, 0.0, 0.0), far),
            Some((v(6.0, 0.0, 0.0), 2))
        );
        assert_eq!(advance(&pts, 2, 1.2, v(6.0, 0.0, 0.0), far), Some((far, 3)));
        assert_eq!(advance(&pts, -1, 1.2, v(6.0, 0.0, 0.0), far), None);
    }

    #[test]
    fn path_goal_scales_by_the_gap() {
        let pts = [v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0), v(20.0, 0.0, 0.0)];
        // 20 m away: interval 2.0 and drift 5.0.
        let p = state(1);
        let here = v(0.0, 0.0, 0.0);
        let next = PathGoal::Goal {
            goal: v(5.0, 0.0, 0.0),
            index: 1,
        };
        assert_eq!(path_goal(&pts, &p, &R, here, v(20.0, 0.0, 0.0), 11.9), next);
        assert_eq!(
            path_goal(&pts, &p, &R, here, v(20.0, 0.0, 0.0), 12.0),
            PathGoal::Due
        );
        assert_eq!(path_goal(&pts, &p, &R, here, v(20.0, 0.0, 4.9), 11.0), next);
        let unscaled = Repath {
            drift_ratio: 0.0,
            interval_per_m: 0.0,
            ..R
        };
        assert_eq!(
            path_goal(&pts, &p, &unscaled, here, v(20.0, 0.0, 0.0), 11.5),
            PathGoal::Due
        );
    }
}
