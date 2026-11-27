//! The two hull sweeps headless_trace_mirror found off Godot's bracket (P2c): a box shape's top
//! and a box-shaped hull's side, both first touched well inside the motion.

use gs_sim_trace::gjk::{GShape, Geom, Xf, cast_aabb};

fn run(name: &str, s: &GShape, from: [f32; 3], motion: [f32; 3], half: f32) -> Option<(f32, f32)> {
    let aabb = cast_aabb(from, [half; 3], motion);
    let mut scratch = Vec::new();
    let got = s.cast(from, motion, [half; 3], &aabb, |_, _| Some(0.0), &mut scratch);
    println!("{name}: {got:?}");
    got
}

/// P2c leftover (fc-native-tick-p3-1): Godot's cast_motion hits these shapes, `GShape::cast` returns None,
/// so the mirror reports a neighbouring shape's later hit. Un-ignore once gjk.rs agrees.
#[test]
#[ignore = "known P2c mismatch: cast skips the shape Godot hits"]
fn cast_hits_shape_godot_hits() {
    let floor = GShape::new(
        Geom::Box([14.0, 0.3, 1.5]),
        Xf { origin: [0.0, 3.3, -12.5], ..Xf::IDENTITY },
        1, 0, [0, 0],
    );
    let pit = run("pit", &floor, [0.817359, 5.490761, -12.63663], [3.848892, -3.122461, -1.747445], 0.15);
    assert_eq!(pit, Some((0.55469, 0.55859)), "pit: Godot brackets the hit at [0.55469, 0.55859]");
    let v: Vec<[f32; 3]> = [
        -123.4059, 23.622, -106.5276, -123.7107, 23.622, -106.5276, -123.4059, 20.574, -106.5276,
        -123.4059, 23.622, -107.1372, -123.7107, 23.622, -107.1372, -123.7107, 20.574, -106.5276,
        -123.4059, 20.574, -107.1372, -123.7107, 20.574, -107.1372,
    ]
    .chunks(3)
    .map(|c| [c[0], c[1], c[2]])
    .collect();
    let hull = GShape::new(Geom::hull(v).unwrap(), Xf::IDENTITY, 1, 0, [0, 0]);
    let ig = run("dm-ig", &hull, [-122.6542, 23.70919, -106.531], [-3.525855, -4.933139, -1.386697], 0.15);
    assert_eq!(ig, Some((0.16797, 0.17383)), "dm-ig: Godot brackets the hit at [0.16797, 0.17383]");
}
