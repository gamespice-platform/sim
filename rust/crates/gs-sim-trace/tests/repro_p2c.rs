//! The two hull sweeps headless_trace_mirror found off Godot's bracket (P2c): a box shape's top
//! and a box-shaped hull's side, both first touched well inside the motion.
//!
//! Godot's GJK calls both whole-motion sweeps separated (the box passes right through), so a
//! shape tested alone is skipped. In the game a neighbour bisects first and `cast_motion`
//! carries its last fraction into this shape's whole-motion test (`mshape.motion` is never
//! reset), and the shorter sweep finds the hit. The carries are the neighbours' brackets.

use gs_sim_trace::gjk::{GShape, Geom, Xf, cast_aabb};

fn run(s: &GShape, from: [f32; 3], motion: [f32; 3], carried: f32) -> Option<(f32, f32)> {
    let half = [0.15; 3];
    let aabb = cast_aabb(from, half, motion);
    let mut scratch = Vec::new();
    let got = s.cast_carried(
        from,
        motion,
        half,
        &aabb,
        |_, _| Some(0.0),
        &mut scratch,
        carried,
    );
    println!("carried {carried}: {got:?}");
    got.ok().map(|(low, hi, _)| (low, hi))
}

#[test]
fn cast_hits_shape_godot_hits() {
    let floor = GShape::new(
        Geom::Box([14.0, 0.3, 1.5]),
        Xf {
            origin: [0.0, 3.3, -12.5],
            ..Xf::IDENTITY
        },
        1,
        0,
        [0, 0],
    );
    let from = [0.8173586, 5.4907613, -12.636626];
    let motion = [3.8488925, -3.1224613, -1.747445];
    assert_eq!(
        run(&floor, from, motion, 1.0),
        None,
        "pit: alone, Godot misses too"
    );
    for carried in [0.69140625, 0.6953125] {
        assert_eq!(
            run(&floor, from, motion, carried),
            Some((0.5546875, 0.55859375)),
            "pit: Godot brackets the hit at [0.55469, 0.55859]"
        );
    }
    let v: Vec<[f32; 3]> = [
        -123.4059, 23.622, -106.5276, -123.7107, 23.622, -106.5276, -123.4059, 20.574, -106.5276,
        -123.4059, 23.622, -107.1372, -123.7107, 23.622, -107.1372, -123.7107, 20.574, -106.5276,
        -123.4059, 20.574, -107.1372, -123.7107, 20.574, -107.1372,
    ]
    .chunks(3)
    .map(|c| [c[0], c[1], c[2]])
    .collect();
    let hull = GShape::new(Geom::hull(v).unwrap(), Xf::IDENTITY, 1, 0, [0, 0]);
    let from = [-122.6542, 23.709192, -106.53104];
    let motion = [-3.5258546, -4.933139, -1.3866969];
    assert_eq!(
        run(&hull, from, motion, 1.0),
        None,
        "dm-ig: alone, Godot misses too"
    );
    for carried in [387.0 / 512.0, 390.0 / 512.0] {
        assert_eq!(
            run(&hull, from, motion, carried),
            Some((0.16796875, 0.17382813)),
            "dm-ig: Godot brackets the hit at [0.16797, 0.17383]"
        );
    }
}
