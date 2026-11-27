//! The collision mirror against hand-worked answers; the engine parity is the FC suite's.

use gs_sim_trace::mirror::Mirror;
use gs_sim_trace::piece::{Piece, Xform};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn unit_box(at: [f64; 3], half: [f64; 3]) -> Piece {
    let x = Xform {
        origin: at,
        ..Xform::IDENTITY
    };
    Piece::cuboid(&x, half, 1).unwrap()
}

#[test]
fn box_hull_has_six_faces_three_edges() {
    let p = unit_box([0.0; 3], [1.0, 2.0, 3.0]);
    assert_eq!(p.planes.len(), 6);
    assert_eq!(p.edges.len(), 3);
    assert!(p.contains([0.9, -1.9, 2.9]));
    assert!(!p.contains([1.1, 0.0, 0.0]));
}

#[test]
fn ray_enters_box_with_face_normal() {
    let m = Mirror::new(vec![unit_box([0.0; 3], [1.0; 3])]);
    let h = m.ray([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], 1).unwrap();
    assert!(near(h.fraction, 2.0 / 6.0));
    assert_eq!(h.normal, [-1.0, 0.0, 0.0]);
    assert!(m.ray([-3.0, 2.0, 0.0], [3.0, 2.0, 0.0], 1).is_none());
    assert!(
        m.ray([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], 2).is_none(),
        "mask"
    );
    assert!(
        m.ray([0.0; 3], [3.0, 0.0, 0.0], 1).is_none(),
        "starts inside"
    );
}

#[test]
fn nearest_of_many() {
    let pieces: Vec<Piece> = (0..50)
        .map(|i| unit_box([i as f64 * 3.0, 0.0, 0.0], [1.0; 3]))
        .collect();
    let m = Mirror::new(pieces);
    let h = m.ray([200.0, 0.0, 0.0], [-10.0, 0.0, 0.0], 1).unwrap();
    assert_eq!(h.piece, 49);
    assert!(near(h.fraction, (200.0 - 148.0) / 210.0));
    let h = m.ray([10.5, 5.0, 0.0], [10.5, -5.0, 0.0], 1);
    assert!(h.is_none(), "between boxes 3 and 4");
}

#[test]
fn triangle_front_only_with_plane_normal() {
    let t = Piece::triangle([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1).unwrap();
    let n = [t.planes[0][0], t.planes[0][1], t.planes[0][2]];
    let m = Mirror::new(vec![t]);
    let down = m.ray([0.2, 1.0, 0.2], [0.2, -1.0, 0.2], 1);
    let up = m.ray([0.2, -1.0, 0.2], [0.2, 1.0, 0.2], 1);
    // A trimesh without backface_collision: only the ray meeting the face's front hits.
    let (hit, miss) = if n[1] > 0.0 { (down, up) } else { (up, down) };
    let hit = hit.unwrap();
    assert!(miss.is_none());
    assert!(near(hit.fraction, 0.5));
    assert_eq!(hit.normal, n);
}

#[test]
fn box_sweep_lands_on_floor() {
    let m = Mirror::new(vec![unit_box([0.0, -1.0, 0.0], [10.0, 1.0, 10.0])]);
    let h = m
        .sweep([0.0, 2.0, 0.0], [0.0, -4.0, 0.0], [0.5, 0.5, 0.5], 1)
        .unwrap();
    assert!(near(h.fraction, 1.5 / 4.0));
    assert_eq!(h.normal, [0.0, 1.0, 0.0]);
    // As Godot's cast_motion: a shape the box starts inside of is ignored.
    assert!(
        m.sweep([0.0, 0.2, 0.0], [1.0, 0.0, 0.0], [0.5, 0.5, 0.5], 1)
            .is_none()
    );
    assert!(
        m.sweep([0.0, 2.0, 0.0], [1.0, 0.0, 0.0], [0.5; 3], 1)
            .is_none()
    );
}

#[test]
fn box_sweep_against_slope_edge() {
    // A wedge: a ramp rising along +x. The box's corner meets the slope, not an axis face.
    let pts = [
        [0.0, 0.0, -5.0],
        [10.0, 0.0, -5.0],
        [10.0, 10.0, -5.0],
        [0.0, 0.0, 5.0],
        [10.0, 0.0, 5.0],
        [10.0, 10.0, 5.0],
    ];
    let w = Piece::convex(&pts, 1).unwrap();
    assert_eq!(w.planes.len(), 5);
    let m = Mirror::new(vec![w]);
    let h = m
        .sweep([5.0, 10.0, 0.0], [0.0, -10.0, 0.0], [0.5; 3], 1)
        .unwrap();
    // The corner (5.5, y - 0.5) reaches the slope y = x at y - 0.5 = 5.5.
    assert!(near(h.fraction, (10.0 - 6.0) / 10.0), "{}", h.fraction);
    let s = std::f64::consts::FRAC_1_SQRT_2;
    assert!(near(h.normal[0], -s) && near(h.normal[1], s));
}

#[test]
fn point_contents_ors_layers() {
    let mut a = unit_box([0.0; 3], [1.0; 3]);
    a.layer = 1;
    let mut b = unit_box([0.5, 0.0, 0.0], [1.0; 3]);
    b.layer = 4;
    let m = Mirror::new(vec![a, b]);
    assert_eq!(m.point_contents([0.2, 0.0, 0.0], u32::MAX), 5);
    assert_eq!(m.point_contents([-0.9, 0.0, 0.0], u32::MAX), 1);
    assert_eq!(m.point_contents([0.2, 0.0, 0.0], 4), 4);
    assert_eq!(m.point_contents([5.0, 0.0, 0.0], u32::MAX), 0);
}

#[test]
fn dynamic_pieces_are_hit_and_reposed() {
    let m = Mirror::new(vec![]);
    let door = unit_box([0.0; 3], [0.1, 1.0, 1.0]);
    let at = |x: f64| Xform {
        origin: [x, 0.0, 0.0],
        ..Xform::IDENTITY
    };
    m.set_dynamic(vec![door.transformed(&at(2.0))]);
    let h = m.ray([0.0; 3], [4.0, 0.0, 0.0], 1).unwrap();
    assert!(near(h.fraction, 1.9 / 4.0));
    m.set_dynamic(vec![door.transformed(&at(10.0))]);
    assert!(m.ray([0.0; 3], [4.0, 0.0, 0.0], 1).is_none());
}

#[test]
fn rotated_box_planes_follow() {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let x = Xform {
        cols: [[c, c, 0.0], [-c, c, 0.0], [0.0, 0.0, 1.0]],
        origin: [0.0; 3],
    };
    let p = Piece::cuboid(&Xform::IDENTITY, [1.0; 3], 1)
        .unwrap()
        .transformed(&x);
    let m = Mirror::new(vec![p]);
    let h = m.ray([-5.0, 0.0, 0.0], [5.0, 0.0, 0.0], 1).unwrap();
    assert!(near(h.fraction, (5.0 - 2f64.sqrt()) / 10.0));
}

#[test]
fn bake_round_trips() {
    let m = Mirror::new(vec![
        unit_box([0.0; 3], [1.0; 3]),
        Piece::triangle([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2).unwrap(),
    ]);
    let bytes = m.to_bytes();
    let back = Mirror::from_bytes(&bytes).unwrap();
    assert_eq!(back.pieces(), m.pieces());
    assert!(Mirror::from_bytes(&bytes[..bytes.len() - 1]).is_none());
    assert!(Mirror::from_bytes(b"nope").is_none());
}
