use super::*;

fn square(door: i32) -> Loop {
    let points = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(4.0, 0.0, 0.0),
        Vec3::new(4.0, 0.0, 4.0),
        Vec3::new(0.0, 0.0, 4.0),
    ];
    let lengths: Vec<f32> = (0..4)
        .map(|i| points[i].distance_to(points[(i + 1) % 4]))
        .collect();
    let total: f64 = lengths.iter().map(|l| f64::from(*l)).sum();
    Loop {
        points,
        lengths,
        doors: vec![-1, door, -1, -1],
        total,
        speed: 1.0,
        sight: 10.0,
        cone: 70.0,
    }
}

fn config() -> Config {
    Config {
        speed_scale: 1.0,
        head_sweep: true,
        door_seconds: 1.6,
        sight_scale: 1.0,
        flashlight_sight: 1.5,
        cone_scale: 1.0,
        exposure_rate: 0.9,
        opening_ease: 0.55,
        opening_levels: 4,
        flashlight_exposure: 1.8,
        crouch_exposure: 0.55,
        still_exposure: 0.6,
        listen_hearing: 1.8,
        light_range: 28.0,
        hear_walls: 0.5,
        reaction_seconds: 1.1,
        alert_seconds: 4.0,
        listens: true,
        listen_seconds: 3.5,
        listen_confirm: 0.35,
        listen_cooldown: 10.0,
        turn_delay: 0.6,
        chase_seconds: 6.0,
        turn_rate: 75.0,
        chase_speed: 2.4,
    }
}

#[test]
fn engine_scalars() {
    assert_eq!(wrapf(PI, -PI, PI), -PI);
    assert_eq!(wrapf(3.0 * PI / 2.0, -PI, PI), -PI / 2.0);
    assert_eq!(smoothstep(0.0, 0.0, 0.0), 0.0);
    assert_eq!(smoothstep(0.0, 0.0, 0.1), 1.0);
    assert_eq!(smoothstep(0.0, 2.0, 1.0), 0.5);
    assert_eq!(lerpf(0.55, 1.0, 0.0), 0.55);
}

#[test]
fn walks_her_loop_on_the_clock() {
    let l = square(-1);
    let p = witch_pose(&l, 0, 0.0, 1.0, false, 0.0);
    assert_eq!(p.at, Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(p.door, -1);
    // Heading +x: atan2(-4, -0).
    assert_eq!(p.facing, (-4.0f64).atan2(-0.0));
    let p = witch_pose(&l, 0, 2.0, 1.0, false, 0.0);
    assert_eq!(p.at, Vec3::new(2.0, 0.0, 0.0));
    let p = witch_pose(&l, 0, 6.0, 1.0, false, 0.0);
    assert_eq!(p.at, Vec3::new(4.0, 0.0, 2.0));
    // The full loop wraps.
    assert_eq!(
        witch_pose(&l, 0, 16.0, 1.0, false, 0.0).at,
        Vec3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn stands_at_a_doorway_and_holds_it() {
    let l = square(7);
    // Leg 0 is 4 s, then she stands 1.6 s at point 1 (door 7), opening it.
    let p = witch_pose(&l, 0, 4.4, 1.0, false, 1.6);
    assert_eq!(p.at, Vec3::new(4.0, 0.0, 0.0));
    assert_eq!(p.door, 7);
    assert_eq!(p.open, smoothstep(0.0, 0.8, 4.4 - 4.0));
    // Walking on, the door stays open DOOR_HELD seconds, then closes over one.
    let p = witch_pose(&l, 0, 4.0 + 1.6 + 1.0, 1.0, false, 1.6);
    assert_eq!(p.door, 7);
    assert_eq!(p.open, 1.0);
    let p = witch_pose(&l, 0, 4.0 + 1.6 + 3.5, 1.0, false, 1.6);
    assert_eq!(p.door, -1);
    assert_eq!(p.open, 0.0);
}

fn player(target: Vec3, noise: f64) -> Player {
    Player {
        noise,
        hspeed: 2.0,
        caught_left: 0.0,
        flashlight: false,
        crouching: false,
        level: 1,
        target,
    }
}

#[test]
fn a_sound_makes_her_listen_turn_and_run() {
    let cfg = config();
    let loops = vec![square(-1)];
    let lv = Level {
        origin: Vec3::default(),
        loops: &loops,
    };
    // Behind her, close and loud.
    let p = player(Vec3::new(-3.0, 1.6, 0.0), 9.0);
    let mut rays_cast = 0;
    let mut rays = |_: Vec3, _: Vec3, _: i32| {
        rays_cast += 1;
        true
    };
    let mut states = vec![None];
    let mut out = Vec::new();
    let delta = 1.0 / 30.0;
    let mut listened = 0;
    let mut modes = Vec::new();
    for tick in 0..600 {
        let seconds = f64::from(tick) * delta;
        watch(&cfg, &lv, &p, &states, seconds, delta, &mut rays, &mut out);
        states[0] = Some(out[0].state);
        if out[0].listened {
            listened += 1;
        }
        if modes.last() != Some(&out[0].state.mode) {
            modes.push(out[0].state.mode);
        }
    }
    assert!(listened >= 1);
    assert_eq!(
        &modes[..4],
        &[MODE_WALK, MODE_LISTEN, MODE_TURN, MODE_CHASE]
    );
    assert!(rays_cast > 0);
}

#[test]
fn a_wall_hides_the_player_from_her_eyes() {
    let cfg = config();
    let loops = vec![square(-1)];
    let lv = Level {
        origin: Vec3::default(),
        loops: &loops,
    };
    // In front of her (she faces +x at the start), standing still.
    let mut p = player(Vec3::new(3.0, 2.0, 0.0), 0.0);
    p.hspeed = 0.0;
    let state = fresh(&cfg, &loops[0], 0, 0.0, p.target);
    let seen = exposure(
        &cfg,
        &lv,
        0,
        &p,
        Some(&state),
        0.0,
        &mut |_: Vec3, _: Vec3, _: i32| true,
    );
    let hidden = exposure(
        &cfg,
        &lv,
        0,
        &p,
        Some(&state),
        0.0,
        &mut |_: Vec3, _: Vec3, _: i32| false,
    );
    assert!(seen > 0.0);
    assert_eq!(hidden, 0.0);
    // Behind her she sees nothing, wall or no wall.
    p.target = Vec3::new(-3.0, 2.0, 0.0);
    assert_eq!(
        exposure(
            &cfg,
            &lv,
            0,
            &p,
            Some(&state),
            0.0,
            &mut |_: Vec3, _: Vec3, _: i32| true
        ),
        0.0
    );
}
