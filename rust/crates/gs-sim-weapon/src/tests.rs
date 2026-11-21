use super::*;

fn rifle() -> Tuning {
    Tuning {
        spread: 0.5,
        spread_moving: 3.0,
        spread_speed_ref: 6.0,
        spread_airborne: 5.0,
        spread_crouched: 0.6,
        bloom: 0.4,
        bloom_max: 2.0,
        bloom_recovery: 0.15,
        bloom_delay: 7,
        settle: 4,
        hip: 0.25,
        slide: 1.5,
        sprint: 2.0,
        flinch: 1.0,
        crouch: 0.5,
        aim: 0.5,
    }
}

fn rules(enabled: bool) -> Rules {
    Rules {
        enabled,
        scale: 1.0,
        mult_hip: 1.0,
        mult_aim: 0.5,
        mult_stand: 1.0,
        mult_walk: 1.15,
        mult_run: 1.35,
        mult_air: 1.5,
        mult_sprint: 1.8,
        mult_slide: 1.6,
        mult_crouch: 0.8,
        walk_speed: 0.5,
        run_speed: 4.5,
        recovery_per_tick: 6.0 / 64.0,
        max_spread: 20.0,
    }
}

fn still() -> Posture {
    Posture {
        hurt_tick: -1,
        ..Posture::default()
    }
}

#[test]
fn spread_for_matches_the_script_formula() {
    let t = rifle();
    assert_eq!(t.spread_for(0.0, false, false), 0.5);
    assert_eq!(
        t.spread_for(3.0, false, false),
        0.5 + (3.0 - 0.5) * (3.0 / 6.0)
    );
    assert_eq!(t.spread_for(99.0, false, true), 3.0 * 0.6);
    assert_eq!(t.spread_for(0.0, true, false), 5.0);
}

#[test]
fn target_factor_picks_the_posture_multiplier() {
    let r = rules(true);
    let mut p = still();
    assert_eq!(r.target_factor(&p), 1.0);
    p.speed = 5.0;
    assert_eq!(r.target_factor(&p), 1.35);
    p.speed = 1.0;
    p.crouched = true;
    p.aiming = true;
    assert_eq!(r.target_factor(&p), 1.15 * 0.8 * 0.5);
    p.sliding = true;
    assert_eq!(r.target_factor(&p), 1.6 * 0.8 * 0.5);
    assert_eq!(rules(false).target_factor(&p), 1.0);
}

#[test]
fn a_tick_without_context_sheds_bloom_after_the_delay() {
    let t = rifle();
    let frame = Frame::default();
    let mut s = State {
        since: Some(5),
        bloom: Some(1.0),
        ..State::default()
    };
    let out = tick(&t, None, &frame, &mut s);
    assert_eq!(
        out,
        Ticked {
            shown: None,
            wrote: WROTE_SINCE
        }
    );
    assert_eq!(s.since, Some(6));
    tick(&t, None, &frame, &mut s);
    assert_eq!(s.bloom, Some(1.0 - 0.15));
    let mut fresh = State::default();
    tick(&t, None, &frame, &mut fresh);
    assert_eq!(fresh.since, Some(NEVER));
}

#[test]
fn motion_settles_and_the_key_goes_at_zero() {
    let t = rifle();
    let frame = Frame::default();
    let mut s = State {
        motion: Some(1.0),
        ..State::default()
    };
    let p = still();
    tick(&t, Some(&p), &frame, &mut s);
    assert_eq!(s.motion, Some(0.75));
    assert!(s.now.is_some());
    for _ in 0..3 {
        tick(&t, Some(&p), &frame, &mut s);
    }
    assert_eq!(s.motion, None);
    // The tick that reached zero still had a held motion, so it wrote the cone; the next one does not.
    let out = tick(&t, Some(&p), &frame, &mut s);
    assert_eq!(out.wrote & WROTE_NOW, 0);
}

#[test]
fn the_factor_recovers_at_the_rules_rate() {
    let t = rifle();
    let frame = Frame {
        rules: Some(rules(true)),
        cone: true,
        rate: 64,
    };
    let mut s = State {
        factor: Some(1.8),
        ..State::default()
    };
    let out = tick(&t, Some(&still()), &frame, &mut s);
    assert_eq!(s.factor, Some(1.8 - 6.0 / 64.0));
    assert_ne!(out.wrote & WROTE_NOW, 0);
    assert_eq!(s.now, out.shown);
}

#[test]
fn flinch_fades_over_flinch_sec() {
    let t = rifle();
    let frame = Frame {
        rules: None,
        cone: false,
        rate: 64,
    };
    let mut p = still();
    p.tick = 110;
    p.hurt_tick = 100;
    let s = State::default();
    let left = 1.0 - 10.0 / (0.4 * 64.0);
    assert_eq!(spread_with(&t, &p, &frame, &s), 0.5 + 0.25 + 1.0 * left);
    p.tick = 99;
    assert_eq!(spread_with(&t, &p, &frame, &s), 0.75);
}

#[test]
fn rows_unpack_in_order() {
    let row: Vec<f64> = (0..TUNING_LEN).map(|i| i as f64).collect();
    let t = Tuning::from_row(&row).unwrap();
    assert_eq!((t.spread_moving, t.bloom_delay, t.aim), (1.0, 8, 15.0));
    assert!(Tuning::from_row(&row[..3]).is_none());
    let p = Posture::from_row(&[2.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 9.0, -1.0]).unwrap();
    assert!(p.airborne && p.aiming && p.wall_running && !p.crouched);
    assert_eq!((p.tick, p.hurt_tick), (9, -1));
}

#[test]
fn a_settled_weapon_rests_and_counts_the_tick() {
    let t = rifle();
    let open = Frame::default();
    let mut s = State {
        since: Some(3),
        ..State::default()
    };
    assert!(rests(&t, &open, &s));
    rest(&mut s);
    assert_eq!(s.since, Some(4));
    let mut never = State::default();
    rest(&mut never);
    assert_eq!(never.since, Some(NEVER));
    let moving = State {
        motion: Some(0.5),
        ..s
    };
    assert!(!rests(&t, &open, &moving));
    let bloomed = State {
        bloom: Some(0.1),
        ..s
    };
    assert!(!rests(&t, &open, &bloomed));
    let no_bloom = Tuning { bloom: 0.0, ..t };
    assert!(rests(&no_bloom, &open, &bloomed));
}

#[test]
fn rest_needs_the_settled_factor_and_no_cone() {
    let t = rifle();
    let on = Frame {
        rules: Some(rules(true)),
        cone: false,
        rate: 64,
    };
    let settled = rules(true).target_factor(&Posture::SETTLED);
    let at = State {
        factor: Some(settled),
        ..State::default()
    };
    assert!(rests(&t, &on, &at));
    assert!(!rests(&t, &on, &State::default()));
    let high = State {
        factor: Some(settled + 0.25),
        ..at
    };
    assert!(!rests(&t, &on, &high));
    let off = Frame {
        rules: Some(rules(false)),
        ..on
    };
    assert!(rests(&t, &off, &State::default()));
    let cone = Frame { cone: true, ..off };
    assert!(!rests(&t, &cone, &State::default()));
    assert_eq!(Posture::SETTLED, still());
}
