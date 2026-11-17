use super::*;

fn piece(x: f32, y: f32, z: f32) -> Piece {
    Piece {
        middle: Vec3::new(x, y, z),
        broken: false,
    }
}

const EYE: Vec3 = Vec3 {
    x: 0.0,
    y: 1.6,
    z: 0.0,
};
const AHEAD: Vec3 = Vec3 {
    x: 0.0,
    y: 0.0,
    z: -1.0,
};

#[test]
fn ahead_and_near_is_kept() {
    assert!(in_view(&piece(0.0, 0.5, -5.0), EYE, AHEAD, 90.0, 12.0));
}

#[test]
fn behind_is_dropped() {
    assert!(!in_view(&piece(0.0, 0.5, 5.0), EYE, AHEAD, 90.0, 12.0));
}

#[test]
fn outside_the_cone_is_dropped() {
    // 50 degrees off forward against a 90 degree field of view.
    let a = 50f32.to_radians();
    assert!(!in_view(
        &piece(5.0 * a.sin(), 1.6, -5.0 * a.cos()),
        EYE,
        AHEAD,
        90.0,
        12.0
    ));
    assert!(in_view(
        &piece(5.0 * a.sin(), 1.6, -5.0 * a.cos()),
        EYE,
        AHEAD,
        110.0,
        12.0
    ));
}

#[test]
fn too_far_or_too_near_is_dropped() {
    assert!(!in_view(&piece(0.0, 1.6, -12.5), EYE, AHEAD, 90.0, 12.0));
    // Inside the square reach but past the round one.
    assert!(!in_view(&piece(9.0, 1.6, -9.0), EYE, AHEAD, 180.0, 12.0));
    assert!(!in_view(&piece(0.0, 1.6, -0.4), EYE, AHEAD, 90.0, 12.0));
}

#[test]
fn broken_is_dropped() {
    let mut p = piece(0.0, 0.5, -5.0);
    p.broken = true;
    assert!(!in_view(&p, EYE, AHEAD, 90.0, 12.0));
}

#[test]
fn scan_keeps_the_map_order() {
    let pieces = [
        piece(0.0, 0.5, -5.0),
        piece(0.0, 0.5, 5.0),
        piece(1.0, 0.5, -3.0),
        piece(-1.0, 0.5, -11.0),
    ];
    let mut out = Vec::new();
    scan(&pieces, EYE, AHEAD, 90.0, 12.0, &mut out);
    assert_eq!(out, vec![0, 2, 3]);
}
