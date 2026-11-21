use super::*;

fn spinner_at(origin: Vec3) -> Hazard {
    Hazard {
        frame: Xform {
            rows: Xform::IDENTITY.rows,
            origin,
        },
        centre: origin,
        reach: (1.0 + 4.0 + 0.35) * 1.25 + 1.0,
        kind: Kind::Spinner {
            arm_height: 1.0,
            arm_length: 4.0,
            hub_radius: 0.6,
            thickness: 0.35,
            arms: 2,
            phase: 0.0,
            speed: 90.0,
        },
    }
}

fn fall_at(origin: Vec3, duty: f64) -> Hazard {
    let frame = Xform {
        rows: Xform::IDENTITY.rows,
        origin,
    };
    Hazard {
        frame,
        centre: origin,
        reach: 3.0,
        kind: Kind::Fall {
            inverse: frame.affine_inverse(),
            size: Vec3::new(2.0, 3.0, 1.0),
            push: 9.0,
            period: 2.0,
            phase: 0.0,
            duty,
        },
    }
}

fn pusher_at(origin: Vec3) -> Hazard {
    Hazard {
        frame: Xform {
            rows: Xform::IDENTITY.rows,
            origin,
        },
        centre: origin,
        reach: 6.0,
        kind: Kind::Pusher {
            yaw: Xform::IDENTITY,
            size: Vec3::new(2.0, 2.0, 1.0),
            reach: 2.0,
            period: 2.0,
            phase: 0.0,
            duty: 0.35,
        },
    }
}

fn pendulum_at(pivot: Vec3) -> Hazard {
    Hazard {
        frame: Xform {
            rows: Xform::IDENTITY.rows,
            origin: pivot,
        },
        centre: pivot,
        reach: (4.0 + 0.8) * 1.25 + 1.0,
        kind: Kind::Pendulum {
            length: 4.0,
            radius: 0.8,
            swing: 60.0,
            period: 2.0,
            phase: 0.0,
        },
    }
}

fn course() -> Hazards {
    let mut course = Hazards::new(64);
    course.hazards = vec![
        spinner_at(Vec3::new(0.0, 0.0, 0.0)),
        pendulum_at(Vec3::new(0.0, 5.0, 10.0)),
        pusher_at(Vec3::new(5.0, 0.0, 20.0)),
        fall_at(Vec3::new(0.0, 0.0, 30.0), 0.4),
    ];
    course
}

const LIMITS: Vec3 = Vec3::new(1.0, 1.0, 1.0);

#[test]
fn a_spinner_arm_along_x_knocks_feet_on_it_at_tick_zero() {
    let course = course();
    let thrown = course.knock(
        Vec3::new(3.0, 0.0, 0.0),
        false,
        0,
        Vec3::new(2.0, 30.0, 1.5),
    );

    assert_ne!(thrown, Vec3::default());
    assert!(thrown.y >= 1.5);
    let flat = (thrown.x * thrown.x + thrown.z * thrown.z).sqrt();
    assert!((2.0..=30.0).contains(&flat), "{flat}");
}

#[test]
fn feet_far_from_everything_are_not_knocked_with_or_without_the_cull() {
    let mut course = course();
    for cull in [true, false] {
        course.cull = cull;
        assert_eq!(
            course.knock(Vec3::new(200.0, 0.0, -50.0), false, 17, LIMITS),
            Vec3::default()
        );
    }
}

#[test]
fn the_cull_never_changes_an_answer() {
    let mut course = course();
    for x in -12..=12 {
        for z in -6..=40 {
            for tick in [0, 9, 33, 70, 129] {
                let feet = Vec3::new(x as f32 * 0.5, 0.0, z as f32);
                course.cull = true;
                let culled = course.knock(feet, z % 2 == 0, tick, LIMITS);
                course.cull = false;
                assert_eq!(
                    culled,
                    course.knock(feet, z % 2 == 0, tick, LIMITS),
                    "{feet:?} at {tick}"
                );
            }
        }
    }
}

#[test]
fn a_pusher_goes_out_holds_comes_back_and_rests() {
    assert_eq!(pusher_extension(2.0, 0.0, 0.35, 0.0), 0.0);
    assert_eq!(pusher_extension(2.0, 0.0, 0.35, 0.08), 0.5);
    assert_eq!(pusher_extension(2.0, 0.0, 0.35, 0.5), 1.0);
    let back = pusher_extension(2.0, 0.0, 0.35, 0.8);
    assert!(back > 0.0 && back < 1.0, "{back}");
    assert_eq!(pusher_extension(2.0, 0.0, 0.35, 1.9), 0.0);
    // A negative phase wraps like fposmod.
    assert_eq!(pusher_extension(2.0, -1.0, 0.35, 0.5), 1.0);
}

#[test]
fn a_curtain_only_knocks_while_it_is_down() {
    let mut course = Hazards::new(64);
    course.hazards = vec![fall_at(Vec3::new(0.0, 0.0, 0.0), 0.4)];
    let feet = Vec3::new(0.0, 0.0, 0.0);

    assert!(fall_on(2.0, 0.0, 0.4, 0.0));
    assert_eq!(
        course.knock(feet, false, 0, LIMITS),
        Vec3::new(1.0, 1.0, 0.0)
    );
    assert!(!fall_on(2.0, 0.0, 0.4, 1.5));
    assert_eq!(course.knock(feet, false, 96, LIMITS), Vec3::default());
}

#[test]
fn a_ram_throws_forward_hard_while_it_extends() {
    let mut course = Hazards::new(64);
    course.hazards = vec![pusher_at(Vec3::default())];
    // Out by 0.1 s: the ram is still moving out, so the throw is at least 14 and forward (-z).
    let thrown = course.knock(
        Vec3::new(0.0, 0.0, -1.0),
        false,
        3,
        Vec3::new(0.0, 100.0, 0.0),
    );
    assert!(thrown.z <= -14.0, "{thrown:?}");
}

#[test]
fn a_pendulum_ball_at_the_bottom_of_its_swing_knocks() {
    let mut course = Hazards::new(64);
    course.hazards = vec![pendulum_at(Vec3::new(0.0, 5.0, 0.0))];
    let thrown = course.knock(
        Vec3::new(0.0, 0.0, 0.0),
        false,
        0,
        Vec3::new(0.0, 100.0, 0.0),
    );
    assert_ne!(thrown, Vec3::default());
}

#[test]
fn a_path_through_a_spinner_is_not_clear_and_one_beside_it_is() {
    let course = course();
    assert!(!course.path_clear(
        Vec3::new(-3.0, 0.0, 0.0),
        Vec3::new(3.0, 0.0, 0.0),
        0,
        0.1,
        0.5
    ));
    assert!(course.path_clear(
        Vec3::new(-60.0, 0.0, -20.0),
        Vec3::new(60.0, 0.0, -20.0),
        0,
        6.0,
        0.5
    ));
}

#[test]
fn segments_meet_where_they_cross() {
    let (a, b, d) = segments(
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, -1.0, 1.0),
        Vec3::new(0.0, 1.0, 1.0),
    );
    assert_eq!(a, Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(b, Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(d, 1.0);
    assert_eq!(
        point_segment(
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::default(),
            Vec3::new(0.0, 1.0, 0.0)
        ),
        2.0
    );
}
