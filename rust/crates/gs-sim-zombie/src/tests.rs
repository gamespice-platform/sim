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

mod decide {
    use super::*;
    use crate::decide::*;

    fn boards() -> Barricades {
        // A window two metres wide on the x axis, a metre deep, from the floor to two metres up.
        Barricades::new(
            vec![
                Barricade {
                    position: v(10.0, 0.0, 10.0),
                    size: v(2.0, 2.0, 1.0),
                },
                Barricade {
                    position: v(-30.0, 5.0, -30.0),
                    size: v(1.0, 2.0, 1.0),
                },
            ],
            4.0,
            0.75,
        )
        .unwrap()
    }

    #[test]
    fn hit_when_in_reach_and_level() {
        let mut z = chaser(Who::Nobody, Who::Is(0));
        z.here = v(3.0, 0.0, 3.0);
        let d = decide_chase(&table(), &rules(), &z, 2.6, false);
        assert_eq!((d.plan.code, d.act, d.ask), (TARGET, ACT_HIT, true));
    }

    #[test]
    fn too_high_to_hit_chases_or_drifts() {
        let mut z = chaser(Who::Nobody, Who::Is(0));
        z.here = v(3.0, 2.0, 3.0);
        assert_eq!(
            decide_chase(&table(), &rules(), &z, 2.6, false).act,
            ACT_CHASE
        );
        assert_eq!(
            decide_chase(&table(), &rules(), &z, 2.6, true).act,
            ACT_SLOW
        );
    }

    #[test]
    fn forced_never_asks_and_far_ones_do_not() {
        let z = chaser(Who::Is(0), Who::Nobody);
        let d = decide_chase(&table(), &rules(), &z, 2.6, false);
        assert_eq!((d.plan.code, d.act, d.ask), (FORCED, ACT_CHASE, false));

        let mut far = chaser(Who::Nobody, Who::Is(0));
        far.here = v(30.0, 0.0, 30.0);
        assert!(!decide_chase(&table(), &rules(), &far, 2.6, false).ask);
        // About ring + 3 out still asks.
        far.here = v(3.0, 0.0, 4.0 - 5.6);
        assert!(decide_chase(&table(), &rules(), &far, 2.6, false).ask);
    }

    #[test]
    fn nobody_is_no_act() {
        let d = decide_chase(
            &table(),
            &rules(),
            &chaser(Who::Nobody, Who::Nobody),
            2.6,
            false,
        );
        assert_eq!((d.plan.code, d.act, d.ask), (NOBODY, ACT_NONE, false));
        let d = decide_chase(
            &table(),
            &rules(),
            &chaser(Who::Nobody, Who::Is(2)),
            2.6,
            false,
        );
        assert_eq!((d.plan.code, d.act), (LOST, ACT_NONE));
    }

    #[test]
    fn nearest_skips_the_dead() {
        assert_eq!(
            nearest(&table(), v(9.0, 0.0, 9.0)),
            flat_distance(v(3.0, 0.0, 4.0), v(9.0, 0.0, 9.0))
        );
        assert_eq!(nearest(&[], v(0.0, 0.0, 0.0)), f64::INFINITY);
    }

    #[test]
    fn pressed_into_the_boards() {
        let b = boards();
        // In front of the window, wishing into it.
        assert_eq!(b.in_way(v(11.0, 0.0, 9.5), v(0.0, 0.0, 1.0)), Some(0));
        // Along it or away from it.
        assert_eq!(b.in_way(v(11.0, 0.0, 9.5), v(1.0, 0.0, 0.0)), None);
        assert_eq!(b.in_way(v(11.0, 0.0, 9.5), v(0.0, 0.0, -1.0)), None);
        // Out of reach, or over it.
        assert_eq!(b.in_way(v(11.0, 0.0, 8.0), v(0.0, 0.0, 1.0)), None);
        assert_eq!(b.in_way(v(11.0, 2.5, 9.5), v(0.0, 0.0, 1.0)), None);
        // Inside the box: toward its middle.
        assert_eq!(b.in_way(v(10.5, 0.0, 10.5), v(1.0, 0.0, 0.0)), Some(0));
        assert_eq!(b.in_way(v(10.5, 0.0, 10.5), v(-1.0, 0.0, 0.0)), None);
        // Nowhere near any.
        assert_eq!(b.in_way(v(100.0, 0.0, 100.0), v(1.0, 0.0, 0.0)), None);
        // A row below the floor of the second one.
        assert_eq!(b.in_way(v(-29.5, 2.0, -30.5), v(0.0, 0.0, 1.0)), None);
        assert_eq!(b.in_way(v(-29.5, 3.5, -30.5), v(0.0, 0.0, 1.0)), Some(1));
    }

    #[test]
    fn no_cell_no_table() {
        assert!(Barricades::new(vec![], 0.0, 0.75).is_none());
        assert!(Barricades::new(vec![], f64::NAN, 0.75).is_none());
        let empty = Barricades::new(vec![], 4.0, 0.75).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.in_way(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)), None);
    }
}
