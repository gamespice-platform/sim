//! gs-spawn's scatter strategy (`GSSpawnScoring.pick_scatter`) as numbers, and how many players
//! stand on each site. The same answers as the script to the bit: distances squared in f32 (Godot's
//! `Vector3.distance_squared_to`), the square root and every factor in f64 (GDScript's `float`).

/// The scatter rules a weight reads (`GSSpawnRules.scatter_*`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Scatter {
    pub min_gap: f64,
    pub radius: f64,
    pub enemy_radius: f64,
}

fn dist_sq(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (x, y, z) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    x * x + y * y + z * z
}

/// `GSSpawnScoring._nearest_packed`: the distance to the nearest point, INF for none.
pub fn nearest(at: [f32; 3], points: &[[f32; 3]]) -> f64 {
    let mut best = f64::INFINITY;
    for p in points {
        let d = f64::from(dist_sq(at, *p));
        if d < best {
            best = d;
        }
    }
    best.sqrt()
}

/// `GSSpawnScoring._damping`: 0 inside the minimum gap of an enemy or a recent pick, else the
/// product of the recent-pick and enemy falloffs.
pub fn damping(rules: &Scatter, recent: &[[f32; 3]], at: [f32; 3], enemies: &[[f32; 3]]) -> f64 {
    let mut f = 1.0;
    let mut e_near = -1.0;
    let mut r_near = -1.0;
    if rules.min_gap > 0.0 {
        e_near = nearest(at, enemies);
        if e_near < rules.min_gap {
            return 0.0;
        }
        r_near = nearest(at, recent);
        if r_near < rules.min_gap {
            return 0.0;
        }
    }
    if rules.radius > 0.0 {
        if r_near < 0.0 {
            r_near = nearest(at, recent);
        }
        if r_near < rules.radius {
            f *= 0.02 + 0.98 * (r_near / rules.radius);
        }
    }
    if rules.enemy_radius > 0.0 {
        if e_near < 0.0 {
            e_near = nearest(at, enemies);
        }
        if e_near < rules.enemy_radius {
            let t = e_near / rules.enemy_radius;
            f *= 0.01 + 0.99 * t * t;
        }
    }
    f
}

/// Every site's draw weight, then their sum as one extra value at the end. When every damped
/// weight is zero the sites' own weights are drawn from instead, as the script does.
pub fn scatter_weights(
    rules: &Scatter,
    sites: &[[f32; 3]],
    weights: &[f64],
    recent: &[[f32; 3]],
    enemies: &[[f32; 3]],
) -> Vec<f64> {
    let n = sites.len().min(weights.len());
    let mut out = Vec::with_capacity(n + 1);
    let mut total = 0.0;
    for (at, own) in sites[..n].iter().zip(&weights[..n]) {
        let mut w = own.max(0.0);
        if w > 0.0 {
            w *= damping(rules, recent, *at, enemies);
        }
        out.push(w);
        total += w;
    }
    if total <= 0.0 {
        for (w, own) in out.iter_mut().zip(&weights[..n]) {
            *w = own.max(0.0);
            total += *w;
        }
    }
    out.push(total);
    out
}

/// The cumulative draw for [`scatter_weights`]' output (its trailing sum ignored): the first site
/// with weight whose running sum reaches [param roll], else the last.
pub fn draw(weights: &[f64], roll: f64) -> usize {
    let n = weights.len().saturating_sub(1);
    let mut roll = roll;
    for (i, w) in weights[..n].iter().enumerate() {
        roll -= *w;
        if roll <= 0.0 && *w > 0.0 {
            return i;
        }
    }
    n.saturating_sub(1)
}

/// How many of [param players] stand within [param radius] of each site (`distance_to <= radius`,
/// the square root in f32 as Godot's `Vector3.distance_to` takes it).
pub fn occupancy(sites: &[[f32; 3]], players: &[[f32; 3]], radius: f64) -> Vec<i32> {
    sites
        .iter()
        .map(|s| {
            players
                .iter()
                .filter(|p| f64::from(dist_sq(*s, **p).sqrt()) <= radius)
                .count() as i32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_is_inf_for_no_points() {
        assert_eq!(nearest([0.0; 3], &[]), f64::INFINITY);
        assert_eq!(nearest([0.0; 3], &[[3.0, 4.0, 0.0], [0.0, 0.0, 6.0]]), 5.0);
    }

    #[test]
    fn damping_zero_inside_the_gap_and_falls_off_outside() {
        let r = Scatter {
            min_gap: 2.0,
            radius: 10.0,
            enemy_radius: 20.0,
        };
        assert_eq!(damping(&r, &[], [0.0; 3], &[[1.0, 0.0, 0.0]]), 0.0);
        assert_eq!(damping(&r, &[[0.0, 1.0, 0.0]], [0.0; 3], &[]), 0.0);
        let far = damping(&r, &[], [0.0; 3], &[]);
        assert_eq!(far, 1.0);
        let near = damping(&r, &[[5.0, 0.0, 0.0]], [0.0; 3], &[[10.0, 0.0, 0.0]]);
        let want = (0.02 + 0.98 * 0.5) * (0.01 + 0.99 * 0.25);
        assert!((near - want).abs() < 1e-12);
    }

    #[test]
    fn weights_fall_back_to_the_sites_own_when_all_are_damped_out() {
        let r = Scatter {
            min_gap: 100.0,
            ..Scatter::default()
        };
        let w = scatter_weights(
            &r,
            &[[0.0; 3], [1.0, 0.0, 0.0]],
            &[2.0, -1.0],
            &[],
            &[[0.0; 3]],
        );
        assert_eq!(w, vec![2.0, 0.0, 2.0]);
    }

    #[test]
    fn draw_skips_zero_weights_and_ends_on_the_last() {
        let w = [0.0, 1.0, 0.0, 2.0, 3.0];
        assert_eq!(draw(&w, 0.0), 1);
        assert_eq!(draw(&w, 1.0), 1);
        assert_eq!(draw(&w, 1.5), 3);
        assert_eq!(draw(&w, 99.0), 3);
        assert_eq!(draw(&[5.0], 1.0), 0);
    }

    #[test]
    fn occupancy_counts_within_the_radius_inclusive() {
        let c = occupancy(
            &[[0.0; 3], [10.0, 0.0, 0.0]],
            &[[1.5, 0.0, 0.0], [0.0, 1.6, 0.0], [10.0, 0.0, 1.0]],
            1.5,
        );
        assert_eq!(c, vec![1, 1]);
    }
}
