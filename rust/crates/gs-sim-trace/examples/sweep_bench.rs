//! Box sweeps per second over a synthetic map: Godot shapes (the `cast_motion` path) and the
//! pieces alone. `cargo run --release --example sweep_bench -p gs-sim-trace [shapes] [sweeps]`.

use gs_sim_trace::gjk::{F3, GShape, Geom, Xf, narrow};
use gs_sim_trace::mirror::Mirror;
use gs_sim_trace::piece::{Piece, Xform};
use std::time::Instant;

fn rng(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 11) as f64 / (1u64 << 53) as f64
}

fn main() {
    let mut args = std::env::args().skip(1).map(|a| a.parse::<usize>().ok());
    let count = args.next().flatten().unwrap_or(2000);
    let sweeps = args.next().flatten().unwrap_or(200_000);
    let mut s = 0x2545_F491_4F6C_DD1D_u64;
    let mut pieces = Vec::new();
    let mut shapes = Vec::new();
    for id in 0..count as u32 {
        let origin = [
            rng(&mut s) * 200.0 - 100.0,
            rng(&mut s) * 10.0,
            rng(&mut s) * 200.0 - 100.0,
        ];
        let half = 0.3 + rng(&mut s) * 2.0;
        let x = Xform {
            origin,
            ..Xform::IDENTITY
        };
        let k = pieces.len() as u32;
        let mut p = Piece::cuboid(&x, [half; 3], 1).expect("a box");
        p.shape = id;
        pieces.push(p);
        let xf = Xf {
            origin: narrow(origin),
            ..Xf::IDENTITY
        };
        shapes.push(GShape::new(Geom::Box([half as f32; 3]), xf, 1, id, [k, k + 1]));
    }
    let floor: Vec<F3> = vec![
        [-100.0, 0.0, -100.0],
        [100.0, 0.0, 100.0],
        [100.0, 0.0, -100.0],
        [-100.0, 0.0, -100.0],
        [-100.0, 0.0, 100.0],
        [100.0, 0.0, 100.0],
    ];
    let k = pieces.len() as u32;
    for t in floor.chunks_exact(3) {
        let w = |p: F3| p.map(f64::from);
        pieces.push(Piece::triangle(w(t[0]), w(t[1]), w(t[2]), 1).expect("a face"));
    }
    shapes.push(GShape::new(
        Geom::faces(floor).expect("faces"),
        Xf::IDENTITY,
        1,
        count as u32,
        [k, k + 2],
    ));
    let queries: Vec<_> = (0..sweeps)
        .map(|_| {
            let from = [
                rng(&mut s) * 200.0 - 100.0,
                1.0 + rng(&mut s) * 8.0,
                rng(&mut s) * 200.0 - 100.0,
            ];
            let motion = [
                rng(&mut s) * 2.0 - 1.0,
                rng(&mut s) * 1.0 - 0.6,
                rng(&mut s) * 2.0 - 1.0,
            ];
            (from, motion)
        })
        .collect();
    let half = [0.4, 0.9, 0.4];
    for (name, m) in [
        ("shapes", Mirror::with_shapes(pieces.clone(), shapes)),
        ("pieces", Mirror::new(pieces)),
    ] {
        let start = Instant::now();
        let mut hits = 0;
        for (from, motion) in &queries {
            hits += usize::from(m.sweep(*from, *motion, half, 1).is_some());
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / sweeps as f64;
        println!("{name}: {us:.3} us/sweep, {hits} hits of {sweeps}");
    }
}
