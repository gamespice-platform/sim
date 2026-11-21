use super::*;

/// A world of horizontal floors and one tilted plane, answering rays the way a physics space
/// would for this test's purposes: the nearest crossing along the segment.
struct Planes {
    /// Each `(normal, d)` with points `p` on the plane where `normal . p = d`.
    planes: Vec<(Vec3, f32)>,
    rays: usize,
}

impl Space for Planes {
    fn ray(&mut self, from: Vec3, to: Vec3) -> Option<Hit> {
        self.rays += 1;
        let dir = to - from;
        let mut best: Option<(f32, Hit)> = None;
        for &(n, d) in &self.planes {
            let denom = n.dot(dir);
            if denom.abs() < 1e-9 {
                continue;
            }
            let t = (d - n.dot(from)) / denom;
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            if best.is_some_and(|(bt, _)| bt <= t) {
                continue;
            }
            best = Some((
                t,
                Hit {
                    position: from + dir.scale(t as f64),
                    normal: n,
                },
            ));
        }
        best.map(|(_, h)| h)
    }
}

fn floor() -> Planes {
    Planes {
        planes: vec![(UP, 0.0)],
        rays: 0,
    }
}

fn tun() -> Tunables {
    Tunables {
        max_slope_angle: 45.573,
        gravity: 15.24,
        max_speed: 4.7625,
        air_accelerate: 1000.0,
        max_air_wish_speed: 0.5715,
    }
}

fn point(x: f32, z: f32) -> Point {
    Point {
        at: Vec3::new(x, 0.0, z),
        radius: metres(96.0),
        land: true,
        ..Default::default()
    }
}

#[test]
fn script_helpers() {
    assert_eq!(wrapf(190.0, -180.0, 180.0), -170.0);
    assert_eq!(wrapf(180.0, -180.0, 180.0), -180.0);
    assert_eq!(wrapf(-180.0, -180.0, 180.0), -180.0);
    assert_eq!(wrapf(179.999999, -180.0, 180.0), -180.0);
    assert_eq!(maxf(f64::NAN, 1.0), 1.0);
    assert_eq!(signf(-0.0), 0.0);
    assert_eq!(normalized(Vec3::new(f32::INFINITY, 0.0, 0.0)), ZERO);
    assert_eq!(UNITS_PER_METRE, 1.0 / 0.01905);
}

#[test]
fn state_round_trips() {
    let b = Bot {
        index: 3,
        surfed: 7,
        side: -1.0,
        leg_from: Vec3::new(1.5, 2.0, -3.25),
        unstuck: 2,
        held: 0.5,
        unstick_left: 0.25,
        unstick_turn: -90.0,
        ..Default::default()
    };
    assert_eq!(Bot::from_state(&b.to_state()), b);
    let d = Bot::default();
    assert_eq!(Bot::from_state(&d.to_state()), d);
}

#[test]
fn walks_and_passes_points_on_the_ground() {
    let route = Route {
        points: vec![point(0.0, -2.0), point(0.0, -10.0)],
        finish: Vec3::new(0.0, 0.0, -20.0),
    };
    let mut bot = Bot::default();
    let mut space = floor();
    let c = bot.command(
        &route,
        Vec3::new(0.0, 0.0, -0.5),
        ZERO,
        true,
        &mut space,
        &tun(),
        1.0 / 128.0,
    );
    // Within the first point's radius on the ground: passed, aiming at the second straight ahead.
    assert_eq!(bot.index, 1);
    assert_eq!(c.mv, [0.0, 1.0]);
    assert_eq!(c.yaw, 0.0);
    // A floor under the feet ahead and no wall: no jump. Ramp (3) + ledge (1) + wall (1) rays.
    assert_eq!(c.buttons, 0);
    assert_eq!(space.rays, 5);
}

#[test]
fn a_point_is_passed_only_once_landed() {
    let route = Route {
        points: vec![point(0.0, -2.0)],
        finish: Vec3::new(0.0, 0.0, -20.0),
    };
    let mut bot = Bot::default();
    let mut space = Planes {
        planes: vec![],
        rays: 0,
    };
    bot.command(
        &route,
        Vec3::new(0.0, 1.0, -2.0),
        Vec3::new(0.0, 0.0, -5.0),
        false,
        &mut space,
        &tun(),
        1.0 / 128.0,
    );
    assert_eq!(bot.index, 0);
    bot.command(
        &route,
        Vec3::new(0.0, 0.0, -2.0),
        ZERO,
        true,
        &mut space,
        &tun(),
        1.0 / 128.0,
    );
    assert_eq!(bot.index, 1);
}

#[test]
fn jumps_at_a_lip() {
    let route = Route {
        points: vec![point(0.0, -10.0)],
        finish: Vec3::new(0.0, 0.0, -20.0),
    };
    let mut bot = Bot::default();
    // No ground at all: the lip test finds none ahead.
    let mut space = Planes {
        planes: vec![],
        rays: 0,
    };
    let c = bot.command(&route, ZERO, ZERO, true, &mut space, &tun(), 1.0 / 128.0);
    assert_eq!(c.buttons, BUTTON_JUMP);
    // The `or` stops at the ledge: ramp (3) + ledge (1).
    assert_eq!(space.rays, 4);
}

#[test]
fn surfs_along_a_steep_face() {
    let route = Route {
        points: vec![point(0.0, -30.0)],
        finish: Vec3::new(0.0, 0.0, -60.0),
    };
    let mut bot = Bot::default();
    // A 60-degree face leaning toward +x, under the player.
    let n = normalized(Vec3::new(0.866, 0.5, 0.0));
    let mut space = Planes {
        planes: vec![(n, 0.0)],
        rays: 0,
    };
    let c = bot.command(
        &route,
        Vec3::new(0.0, 0.05, 0.0),
        Vec3::new(0.0, 0.0, -10.0),
        false,
        &mut space,
        &tun(),
        1.0 / 128.0,
    );
    assert_eq!(bot.surfed, 1);
    assert_eq!(c.mv[1], 0.0);
    assert_eq!(c.mv[0].abs(), 1.0);
}

#[test]
fn air_strafes_to_gain() {
    let route = Route {
        points: vec![point(0.0, -30.0)],
        finish: Vec3::new(0.0, 0.0, -60.0),
    };
    let mut bot = Bot::default();
    let mut space = Planes {
        planes: vec![],
        rays: 0,
    };
    // Going straight at the point too slowly: alternate sides, looking along the velocity.
    let c = bot.command(
        &route,
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 3.0, -2.0),
        false,
        &mut space,
        &tun(),
        1.0 / 128.0,
    );
    assert_eq!(c.yaw, 0.0);
    assert_eq!(c.mv, [-1.0, 0.0]);
    let c = bot.command(
        &route,
        Vec3::new(0.0, 1.0, -0.01),
        Vec3::new(0.0, 3.0, -2.0),
        false,
        &mut space,
        &tun(),
        1.0 / 128.0,
    );
    assert_eq!(c.mv, [1.0, 0.0]);
}

#[test]
fn unsticks_after_holding_still() {
    let route = Route {
        points: vec![point(0.0, -30.0)],
        finish: Vec3::new(0.0, 0.0, -60.0),
    };
    let mut bot = Bot::default();
    let mut space = floor();
    let delta = 1.0 / 128.0;
    let mut turned = None;
    for tick in 0..400 {
        let c = bot.command(&route, ZERO, ZERO, true, &mut space, &tun(), delta);
        if bot.unstuck == 1 {
            turned = Some((tick, c));
            break;
        }
    }
    let (tick, c) = turned.expect("never unstuck");
    assert_eq!(tick, 256);
    assert_eq!(c.yaw, 90.0);
    assert_eq!(c.buttons, BUTTON_JUMP);
}

#[test]
fn restart_and_doors() {
    let mut door = point(0.0, -2.0);
    door.door = true;
    let route = Route {
        points: vec![door, point(0.0, -10.0)],
        finish: ZERO,
    };
    let mut bot = Bot::default();
    let mut space = floor();
    bot.command(
        &route,
        Vec3::new(0.0, 0.0, -2.0),
        ZERO,
        true,
        &mut space,
        &tun(),
        1.0 / 128.0,
    );
    assert_eq!(bot.index, 0, "a door is never passed by standing on it");
    bot.teleported(&route);
    assert_eq!(bot.index, 1);
    assert_eq!(bot.leg_from, INF3);
    bot.restart();
    assert_eq!(bot.index, 0);
    assert_eq!(bot.target(&route), door.at);
}
