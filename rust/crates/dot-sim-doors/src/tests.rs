use super::*;

fn turn_y(deg: f64, at: Vec3) -> Xform {
    let (s, c) = (deg.to_radians() as f32).sin_cos();
    Xform {
        rows: [
            Vec3::new(c, 0.0, s),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-s, 0.0, c),
        ],
        origin: at,
    }
}

/// The inverse of a rotation about y and a move, as `affine_inverse` gives it for these.
fn inverse_of(f: &Xform) -> Xform {
    let r = f.rows;
    let t = [
        Vec3::new(r[0].x, r[1].x, r[2].x),
        Vec3::new(r[0].y, r[1].y, r[2].y),
        Vec3::new(r[0].z, r[1].z, r[2].z),
    ];
    let o = f.origin;
    let back = Xform {
        rows: t,
        origin: Vec3::default(),
    }
    .xform(o);
    Xform {
        rows: t,
        origin: -back,
    }
}

fn panel(deg: f64, at: Vec3) -> Panel {
    let frame = turn_y(deg, at);
    Panel {
        frame,
        inverse: inverse_of(&frame),
        size: Vec3::new(1.0, 2.0, 0.1),
    }
}

#[test]
fn the_nearest_point_is_clamped_into_the_box() {
    let p = panel(0.0, Vec3::new(0.0, 1.0, 0.0));
    let n = nearest(&p, Vec3::new(3.0, 1.5, 2.0));
    assert_eq!(n, Vec3::new(0.5, 1.5, 0.05));
    let inside = nearest(&p, Vec3::new(0.1, 1.2, 0.0));
    assert_eq!(inside, Vec3::new(0.1, 1.2, 0.0));
}

#[test]
fn the_nearest_in_reach_and_in_front_wins() {
    let panels = [
        panel(0.0, Vec3::new(0.0, 1.0, -2.0)),
        panel(0.0, Vec3::new(0.0, 1.0, -1.2)),
        panel(90.0, Vec3::new(1.0, 1.0, 0.0)),
    ];
    let eyes = Vec3::new(0.0, 1.6, 0.0);
    // Looking down -z: the nearer of the two ahead.
    assert_eq!(door_for(&panels, eyes, Vec3::new(0.0, 0.0, -1.0), 2.3), 1);
    // Looking away: none is in front and none is within touching distance.
    assert_eq!(door_for(&panels, eyes, Vec3::new(0.0, 0.0, 1.0), 2.3), -1);
    // Out of reach.
    assert_eq!(door_for(&panels, eyes, Vec3::new(0.0, 0.0, -1.0), 1.0), -1);
    // Within touching distance it is used whatever the aim.
    let near = Vec3::new(0.7, 1.6, 0.0);
    assert_eq!(door_for(&panels, near, Vec3::new(-1.0, 0.0, 0.0), 2.3), 2);
}

#[test]
fn the_first_of_equals_wins_and_none_is_minus_one() {
    let p = panel(0.0, Vec3::new(0.0, 1.0, -1.0));
    let eyes = Vec3::new(0.0, 1.6, 0.0);
    let aim = Vec3::new(0.0, 0.0, -1.0);
    assert_eq!(door_for(&[p, p], eyes, aim, 2.3), 0);
    assert_eq!(door_for(&[], eyes, aim, 2.3), -1);
}

#[test]
fn a_zero_aim_is_never_in_front() {
    let p = panel(0.0, Vec3::new(0.0, 1.0, -2.0));
    let eyes = Vec3::new(0.0, 1.6, 0.0);
    assert_eq!(door_for(&[p], eyes, Vec3::default(), 2.3), -1);
}
