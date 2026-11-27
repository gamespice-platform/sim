//! The Godot-shape box sweep: hand-worked contacts, the early-out against every shape, and a
//! bake that writes back the same bytes.

use gs_sim_trace::gjk::{CLEAR, F3, GShape, Geom, Xf, cast_aabb, narrow};
use gs_sim_trace::mirror::Mirror;
use gs_sim_trace::piece::{Piece, V3, Xform};

fn at(origin: F3) -> Xf {
    Xf {
        origin,
        ..Xf::IDENTITY
    }
}

fn cube(origin: V3, half: f64, id: u32, pieces: &mut Vec<Piece>) -> GShape {
    let x = Xform {
        origin,
        ..Xform::IDENTITY
    };
    let k = pieces.len() as u32;
    let mut p = Piece::cuboid(&x, [half; 3], 1).unwrap();
    p.shape = id;
    pieces.push(p);
    GShape::new(
        Geom::Box([half as f32; 3]),
        at(narrow(origin)),
        1,
        id,
        [k, k + 1],
    )
}

fn floor_faces(y: f32) -> Vec<F3> {
    let (a, b, c, d) = (
        [-5.0, y, -5.0],
        [5.0, y, -5.0],
        [5.0, y, 5.0],
        [-5.0, y, 5.0],
    );
    vec![a, c, b, a, d, c]
}

/// Godot's safe fraction lies at most one last bracket (< 0.006) short of the true contact.
fn lands(hit: Option<f64>, contact: f64) {
    let f = hit.expect("a hit");
    assert!(f <= contact && f >= contact - 0.006, "{f} vs {contact}");
}

#[test]
fn box_lands_on_a_box_shape() {
    let mut pieces = Vec::new();
    let s = cube([0.0, -1.0, 0.0], 1.0, 0, &mut pieces);
    let m = Mirror::with_shapes(pieces, vec![s]);
    let hit = m.sweep([0.0, 2.0, 0.0], [0.0, -3.0, 0.0], [0.5; 3], 1);
    lands(hit.map(|h| h.fraction), 0.5);
    assert_eq!(hit.unwrap().normal, [0.0, 1.0, 0.0]);
    assert!(
        m.sweep([3.0, 2.0, 0.0], [0.0, -3.0, 0.0], [0.5; 3], 1)
            .is_none()
    );
    assert!(
        m.sweep([0.0, 2.0, 0.0], [0.0, -3.0, 0.0], [0.5; 3], 2)
            .is_none()
    );
}

#[test]
fn box_lands_on_a_trimesh_and_a_hull() {
    let faces = GShape::new(
        Geom::faces(floor_faces(0.0)).unwrap(),
        Xf::IDENTITY,
        1,
        0,
        [0, 0],
    );
    let m = Mirror::with_shapes(Vec::new(), vec![faces]);
    let hit = m.sweep([0.3, 2.0, 0.2], [0.0, -3.0, 0.0], [0.5; 3], 1);
    lands(hit.map(|h| h.fraction), 0.5);
    let pts: Vec<F3> = (0..8)
        .map(|k| {
            [
                if k & 1 == 0 { -1.0 } else { 1.0 },
                if k & 2 == 0 { -2.0 } else { 0.0 },
                if k & 4 == 0 { -1.0 } else { 1.0 },
            ]
        })
        .collect();
    let hull = GShape::new(Geom::hull(pts).unwrap(), at([4.0, 0.0, 0.0]), 1, 1, [0, 0]);
    let m = Mirror::with_shapes(Vec::new(), vec![hull]);
    let hit = m.sweep([0.0, -1.0, 0.0], [6.0, 0.0, 0.0], [0.5; 3], 1);
    lands(hit.map(|h| h.fraction), 2.5 / 6.0);
}

#[test]
fn a_shape_the_box_starts_in_is_ignored() {
    let mut pieces = Vec::new();
    let s = cube([0.0; 3], 1.0, 0, &mut pieces);
    let m = Mirror::with_shapes(pieces, vec![s]);
    assert!(
        m.sweep([0.0, 0.5, 0.0], [0.0, -3.0, 0.0], [0.5; 3], 1)
            .is_none()
    );
}

fn rng(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 11) as f64 / (1u64 << 53) as f64
}

fn scene(seed: u64) -> Mirror {
    let mut s = seed;
    let mut pieces = Vec::new();
    let mut shapes = Vec::new();
    for id in 0..300 {
        let o = [
            rng(&mut s) * 40.0 - 20.0,
            rng(&mut s) * 6.0 - 3.0,
            rng(&mut s) * 40.0 - 20.0,
        ];
        shapes.push(cube(o, 0.2 + rng(&mut s), id, &mut pieces));
    }
    let tilted = Xf {
        rows: [[1.0, 0.0, 0.0], [0.0, 0.96, -0.28], [0.0, 0.28, 0.96]],
        origin: [0.0, -4.0, 0.0],
    };
    shapes.push(GShape::new(
        Geom::faces(floor_faces(0.0)).unwrap(),
        tilted,
        1,
        300,
        [0, 0],
    ));
    Mirror::with_shapes(pieces, shapes)
}

#[test]
fn the_early_out_answers_as_every_shape() {
    let m = scene(0x2545_F491_4F6C_DD1D);
    let mut s = 0x9E37_79B9_7F4A_7C15_u64;
    let mut hits = 0;
    let mut scratch = Vec::new();
    for q in 0..3000 {
        let from = [
            rng(&mut s) * 44.0 - 22.0,
            rng(&mut s) * 8.0 - 4.0,
            rng(&mut s) * 44.0 - 22.0,
        ];
        let motion = [
            rng(&mut s) * 16.0 - 8.0,
            rng(&mut s) * 6.0 - 3.0,
            rng(&mut s) * 16.0 - 8.0,
        ];
        let half = [0.4, 0.9, 0.4];
        let (f, mo, h) = (narrow(from), narrow(motion), narrow(half));
        let aabb = cast_aabb(f, h, mo);
        let entry = |lo: V3, hi: V3| {
            let mut t0 = 0.0_f64;
            let mut t1 = 1.0_f64;
            for a in 0..3 {
                let (l, u) = (lo[a] - half[a] - CLEAR, hi[a] + half[a] + CLEAR);
                if motion[a] == 0.0 {
                    if from[a] < l || from[a] > u {
                        return None;
                    }
                    continue;
                }
                let (mut n, mut x) = ((l - from[a]) / motion[a], (u - from[a]) / motion[a]);
                if n > x {
                    std::mem::swap(&mut n, &mut x);
                }
                t0 = t0.max(n);
                t1 = t1.min(x);
                if t0 > t1 {
                    return None;
                }
            }
            Some(t0)
        };
        let every = m
            .shapes()
            .iter()
            .filter_map(|g| g.cast(f, mo, h, &aabb, entry, &mut scratch))
            .map(|(low, _)| low)
            .fold(None, |b: Option<f32>, l| Some(b.map_or(l, |b| b.min(l))))
            .filter(|l| *l < 1.0);
        let got = m.sweep(from, motion, half, 1).map(|h| h.fraction as f32);
        assert_eq!(got, every, "query {q}");
        hits += usize::from(got.is_some());
    }
    assert!(hits > 300, "{hits}");
}

#[test]
fn a_baked_blob_writes_back_the_same_bytes() {
    let m = scene(7);
    let bytes = m.to_bytes();
    let back = Mirror::from_bytes(&bytes).unwrap();
    assert_eq!(back.to_bytes(), bytes);
    assert_eq!(back.shapes(), m.shapes());
    assert_eq!(back.pieces(), m.pieces());
    let fresh = scene(7).to_bytes();
    assert_eq!(fresh, bytes);
    let q = |m: &Mirror| m.sweep([1.0, 4.0, 2.0], [3.0, -9.0, -1.0], [0.4, 0.9, 0.4], 1);
    assert_eq!(q(&back), q(&m));
    assert!(Mirror::from_bytes(&bytes[..bytes.len() - 1]).is_none());
}

#[test]
fn a_version_2_blob_still_loads() {
    let m = Mirror::new(vec![
        Piece::triangle([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2).unwrap(),
    ]);
    let mut v2 = m.to_bytes();
    v2.truncate(v2.len() - 4);
    v2[4..8].copy_from_slice(&2u32.to_le_bytes());
    let back = Mirror::from_bytes(&v2).unwrap();
    assert_eq!(back.pieces(), m.pieces());
    assert!(back.shapes().is_empty());
}
