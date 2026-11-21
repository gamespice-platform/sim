use super::*;

/// A switchback: out along -z, a hairpin, back along +z four metres to the right, 2 m a sample.
fn switchback() -> Road {
    let mut road = Road::default();
    let mut d = 0.0f32;
    let mut prev: Option<Vec3> = None;
    for k in 0..200 {
        let (p, yaw) = if k < 100 {
            (Vec3::new(0.0, k as f32 * 0.1, -2.0 * k as f32), 0.0f32)
        } else {
            let j = (k - 100) as f32;
            (
                Vec3::new(8.0, 10.0 + j * 0.1, -198.0 + 2.0 * j),
                std::f32::consts::PI,
            )
        };
        if let Some(q) = prev {
            d += (p - q).length();
        }
        prev = Some(p);
        road.points.push(p);
        road.distances.push(d);
        road.yaws.push(yaw);
    }
    road
}

#[test]
fn index_at_distance_is_the_first_sample_at_or_past() {
    let road = switchback();
    assert_eq!(road.index_at_distance(-5.0), 0);
    assert_eq!(road.index_at_distance(0.0), 0);
    assert_eq!(road.index_at_distance(f64::from(road.distances[10])), 10);
    assert_eq!(
        road.index_at_distance(f64::from(road.distances[10]) + 0.001),
        11
    );
    assert_eq!(road.index_at_distance(1.0e9), 199);
}

#[test]
fn nearest_index_keeps_to_the_window_then_asks_the_whole_road() {
    let road = switchback();
    let p = road.points[40] + Vec3::new(0.3, 0.0, 0.0);
    assert_eq!(road.nearest_index(p, 40), 40);
    assert_eq!(road.nearest_index(p, -1), 40);
    // A window on the switchback's other leg keeps to that leg, as the script does.
    assert_eq!(road.nearest_index(p, 150), 159);
    // Best at the window's inner edge: the whole road.
    assert_eq!(road.nearest_index(p, 199), 40);
    // A hint past the end still finds it.
    assert_eq!(road.nearest_index(p, 400), 40);
    assert_eq!(Road::default().nearest_index(p, 3), 0);
}

#[test]
fn a_truck_ahead_in_the_lane_blocks_and_one_beside_or_behind_does_not() {
    let road = switchback();
    // A car going down the road (decreasing d) at sample 50, 2 m right of the centreline.
    let d = f64::from(road.distances[50]);
    let lane = 2.0;
    let right = right_of(0.0);
    let at = |i: usize, side: f32| {
        let p = road.points[i];
        Vec3::new(p.x + right.x * side, p.y, p.z + right.z * side)
    };
    assert!(road.car_blocked(d, lane, &[at(45, 2.0)]));
    assert!(!road.car_blocked(d, lane, &[at(45, -2.0)]));
    assert!(!road.car_blocked(d, lane, &[at(40, 2.0)]));
    assert!(!road.car_blocked(d, lane, &[at(53, 2.0)]));
    assert!(road.car_blocked(d, lane, &[at(150, 2.0), at(46, 1.5)]));
    assert!(!road.car_blocked(d, lane, &[]));
    assert!(!Road::default().car_blocked(d, lane, &[at(45, 2.0)]));
}

#[test]
fn the_script_arithmetic_spelled_out_agrees() {
    // The same sum the script does, step by step, for a spread of trucks.
    let road = switchback();
    let mut seed = 0x2545_f491_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed) / f64::from(u32::MAX)
    };
    for _ in 0..2000 {
        let d = next() * road.length();
        let lateral = next() * 4.0 - 2.0;
        let p = Vec3::new(
            (next() * 12.0 - 2.0) as f32,
            (next() * 20.0) as f32,
            (next() * -200.0) as f32,
        );
        let i = road.nearest_index(p, road.index_at_distance(d)) as usize;
        let yaw = f64::from(road.yaws[i]);
        let q = road.points[i];
        let rel = Vec3::new(p.x - q.x, p.y - q.y, p.z - q.z);
        let f = Vec3::new((-yaw.sin()) as f32, 0.0, (-yaw.cos()) as f32);
        let r = Vec3::new(yaw.cos() as f32, 0.0, (-yaw.sin()) as f32);
        let along =
            f64::from(road.distances[i]) + f64::from(f.x * rel.x + f.y * rel.y + f.z * rel.z);
        let side = f64::from(r.x * rel.x + r.y * rel.y + r.z * rel.z);
        let ahead = d - along;
        let want = ahead > -2.0 && ahead < 14.0 && (side - lateral).abs() < 2.3;
        assert_eq!(road.car_blocked(d, lateral, &[p]), want);
    }
}
