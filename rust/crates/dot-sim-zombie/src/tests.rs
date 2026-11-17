use super::*;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn table() -> Vec<Survivor> {
    vec![
        Survivor {
            at: v(3.0, 0.0, 4.0),
            alive: true,
            down: false,
        },
        Survivor {
            at: v(-1.0, 0.5, 2.0),
            alive: true,
            down: true,
        },
        Survivor {
            at: v(9.0, 0.0, 9.0),
            alive: false,
            down: true,
        },
    ]
}

fn rules() -> Rules {
    Rules {
        walk_speed: 4.2,
        run_speed: 6.5,
        angry: false,
        runner_fraction: 0.3,
        down_attack: true,
    }
}

fn chaser(forced: Who, target: Who) -> Chaser {
    Chaser {
        forced,
        target,
        pace: 0.9,
        roll: 0.1,
        reach: 1.55,
        burning: false,
        crawler: false,
        here: v(0.0, 0.0, 0.0),
    }
}

#[test]
fn forced_alive_wins_and_is_kept() {
    let p = plan_chase(&table(), &rules(), &chaser(Who::Is(0), Who::Is(1)));
    assert_eq!((p.code, p.dropped), (FORCED, false));
    assert_eq!(p.at, v(3.0, 0.0, 4.0));
    assert_eq!(p.gap, 5.0);
    assert_eq!(p.speed, 4.2 * 0.9);
    assert_eq!(p.reach, 1.55);
}

#[test]
fn forced_dead_or_unknown_is_dropped_for_the_target() {
    for forced in [Who::Is(2), Who::Unknown] {
        let p = plan_chase(&table(), &rules(), &chaser(forced, Who::Is(0)));
        assert_eq!((p.code, p.dropped), (TARGET, true));
    }
    let p = plan_chase(&table(), &rules(), &chaser(Who::Is(2), Who::Nobody));
    assert_eq!((p.code, p.dropped), (NOBODY, true));
    let p = plan_chase(&table(), &rules(), &chaser(Who::Nobody, Who::Nobody));
    assert_eq!((p.code, p.dropped), (NOBODY, false));
}

#[test]
fn a_dead_or_unknown_target_is_lost() {
    for target in [Who::Is(2), Who::Unknown, Who::Is(99)] {
        let p = plan_chase(&table(), &rules(), &chaser(Who::Nobody, target));
        assert_eq!(p.code, LOST);
    }
}

#[test]
fn ground_strike_caps_reach_unless_crawler_or_rule_off() {
    let p = plan_chase(&table(), &rules(), &chaser(Who::Nobody, Who::Is(1)));
    assert_eq!(p.reach, 1.1);
    let mut c = chaser(Who::Nobody, Who::Is(1));
    c.crawler = true;
    assert_eq!(plan_chase(&table(), &rules(), &c).reach, 1.55);
    let mut r = rules();
    r.down_attack = false;
    assert_eq!(
        plan_chase(&table(), &r, &chaser(Who::Nobody, Who::Is(1))).reach,
        1.55
    );
    let mut c = chaser(Who::Nobody, Who::Is(1));
    c.reach = 0.8;
    assert_eq!(plan_chase(&table(), &rules(), &c).reach, 0.8);
}

#[test]
fn runners_under_an_angry_box_and_burning() {
    let mut r = rules();
    r.angry = true;
    let mut c = chaser(Who::Nobody, Who::Is(0));
    c.burning = true;
    assert_eq!(plan_chase(&table(), &r, &c).speed, 6.5 * 0.9 * 1.2);
    c.roll = 0.3;
    assert_eq!(plan_chase(&table(), &r, &c).speed, 4.2 * 0.9 * 1.2);
}

#[test]
fn flat_distance_rounds_like_vector2() {
    let a = v(1.1, 7.0, -2.3);
    let b = v(-0.7, 0.0, 5.9);
    let x = (f64::from(a.x) - f64::from(b.x)) as f32;
    let z = (f64::from(a.z) - f64::from(b.z)) as f32;
    assert_eq!(flat_distance(a, b), f64::from((x * x + z * z).sqrt()));
    assert_eq!(flat_distance(a, a), 0.0);
}
