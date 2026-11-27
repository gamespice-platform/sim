use super::bullet::*;

/// Walls across the -z axis: each a slab from `near` to `far` (both negative z, near > far).
struct Slabs {
    walls: Vec<(f32, f32, f64, bool, f64)>,
}

impl WorldTrace for Slabs {
    fn ray(&mut self, from: V3, dir: V3, max: f64) -> Option<WorldHit> {
        let mut best: Option<WorldHit> = None;
        for (i, &(near, far, resistance, ricochet, sheet)) in self.walls.iter().enumerate() {
            for (face, n) in [(near, 1.0f32), (far, -1.0f32)] {
                if dir[2].abs() < 1e-6 {
                    continue;
                }
                let t = (face - from[2]) / dir[2];
                if t < 0.0 || t as f64 > max {
                    continue;
                }
                // A face only meets rays entering it from its outside.
                if dir[2] * n > 0.0 {
                    continue;
                }
                if best.is_some_and(|b| b.distance <= t as f64) {
                    continue;
                }
                best = Some(WorldHit {
                    distance: t as f64,
                    point: add(from, [dir[0] * t, dir[1] * t, dir[2] * t]),
                    normal: [0.0, 0.0, n],
                    material: i as i64,
                    resistance,
                    ricochet,
                    sheet,
                    pane: false,
                });
            }
        }
        best
    }
}

fn at(o: V3) -> Xform {
    Xform {
        o,
        ..Xform::IDENTITY
    }
}

fn body(entity: i64, z: f32) -> HitSet {
    let mut head = Hitbox::new(at([0.0, 1.6, z]), Shape::Sphere, 0.15, 0.0, [0.0; 3]);
    head.precedence = 10;
    head.damage_scale = 4.0;
    let torso = Hitbox::new(at([0.0, 1.1, z]), Shape::Capsule, 0.25, 0.9, [0.0; 3]);
    HitSet {
        entity,
        enabled: true,
        centre: [0.0, 0.9, z],
        radius: 1.6,
        boxes: vec![head, torso],
    }
}

fn pen() -> Pen {
    Pen {
        power: 100.0,
        max_depth: 1.0,
        body_cost: 10.0,
        min_damage: 0.1,
        max_passes: 4,
        ricochet_keep: 0.6,
        max_ricochets: 1,
        players: true,
        zones: vec![1.0, 8.0],
        ..Pen::default()
    }
}

const AHEAD: V3 = [0.0, 0.0, -1.0];

fn run(world: &mut Slabs, sets: &[HitSet], p: Option<&Pen>, origin: V3, dir: V3) -> Vec<Event> {
    let info: Vec<Vec<BoxInfo>> = sets
        .iter()
        .map(|s| vec![BoxInfo { group: 1 }; s.boxes.len()])
        .collect();
    let mut skip = vec![false; sets.len()];
    let mut out = Vec::new();
    walk(
        world, sets, &info, &mut skip, p, origin, dir, 100.0, &mut out,
    );
    out
}

#[test]
fn inverse_undoes_the_transform() {
    let t = Xform {
        b: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 2.0]],
        o: [1.0, 2.0, 3.0],
    };
    let p = t.affine_inverse().xform(t.xform([0.5, -0.25, 4.0]));
    assert!(distance(p, [0.5, -0.25, 4.0]) < 1e-5);
    let id = t.mul(&t.affine_inverse());
    assert!(distance(id.xform([3.0, 4.0, 5.0]), [3.0, 4.0, 5.0]) < 1e-5);
}

#[test]
fn shapes_meet_rays_and_report_starting_inside_as_zero() {
    assert!((ray_sphere([0.0, 0.0, 5.0], AHEAD, [0.0; 3], 1.0) - 4.0).abs() < 1e-6);
    assert_eq!(ray_sphere([0.0, 0.0, 0.5], AHEAD, [0.0; 3], 1.0), 0.0);
    assert_eq!(ray_sphere([2.0, 0.0, 5.0], AHEAD, [0.0; 3], 1.0), -1.0);
    assert!((ray_box([0.0, 0.0, 5.0], AHEAD, [1.0, 1.0, 1.0]) - 4.0).abs() < 1e-6);
    assert_eq!(ray_box([0.0, 0.0, 0.0], AHEAD, [1.0, 1.0, 1.0]), 0.0);
    assert_eq!(ray_box([0.0, 3.0, 5.0], AHEAD, [1.0, 1.0, 1.0]), -1.0);
    let cap = Hitbox::new(Xform::IDENTITY, Shape::Capsule, 0.5, 2.0, [0.0; 3]);
    let (d, p, n) = cap.intersect([0.0, 0.0, 5.0], AHEAD, 100.0).unwrap();
    assert!((d - 4.5).abs() < 1e-5, "{d}");
    assert!(distance(p, [0.0, 0.0, 0.5]) < 1e-5);
    assert!(distance(n, [0.0, 0.0, 1.0]) < 1e-5);
    // Over the cylinder, onto the top cap.
    let (d, _, _) = cap.intersect([0.0, 0.8, 5.0], AHEAD, 100.0).unwrap();
    assert!((d - 4.6).abs() < 1e-5, "{d}");
    assert!(cap.intersect([0.0, 2.0, 5.0], AHEAD, 100.0).is_none());
    assert!(cap.intersect([0.0, 0.0, 5.0], AHEAD, 4.0).is_none());
}

#[test]
fn open_air_records_the_full_range() {
    let out = run(&mut Slabs { walls: vec![] }, &[], None, [0.0; 3], AHEAD);
    assert_eq!(
        out,
        vec![Event::Impact {
            point: [0.0, 0.0, -100.0]
        }]
    );
}

#[test]
fn the_head_wins_over_the_torso_it_sits_in_front_of() {
    let sets = [body(7, -10.0)];
    let out = run(
        &mut Slabs { walls: vec![] },
        &sets,
        None,
        [0.0, 1.6, 0.0],
        AHEAD,
    );
    let Event::Body {
        set,
        index,
        distance,
        strength,
        passes,
        ..
    } = out[0]
    else {
        panic!("{out:?}")
    };
    assert_eq!((set, index, passes), (0, 0, 0));
    assert!((distance - 9.85).abs() < 1e-4);
    assert_eq!(strength, 1.0);
    assert!(matches!(out[1], Event::Impact { .. }));
}

#[test]
fn a_wall_without_penetration_stops_the_pellet() {
    let mut w = Slabs {
        walls: vec![(-5.0, -5.2, 1.0, false, 0.0)],
    };
    let out = run(&mut w, &[body(7, -10.0)], None, [0.0, 1.6, 0.0], AHEAD);
    assert_eq!(
        out,
        vec![Event::Impact {
            point: [0.0, 1.6, -5.0]
        }]
    );
}

#[test]
fn a_wallbang_spends_power_and_weakens_the_hit() {
    let mut w = Slabs {
        walls: vec![(-5.0, -5.2, 100.0, false, 0.0)],
    };
    let p = pen();
    let out = run(&mut w, &[body(7, -10.0)], Some(&p), [0.0, 1.6, 0.0], AHEAD);
    let Event::Pass {
        thickness,
        cost,
        sheet,
        ..
    } = out[0]
    else {
        panic!("{out:?}")
    };
    assert!((thickness - 0.2).abs() < 1e-5);
    assert!((cost - 20.0).abs() < 1e-3);
    assert!(!sheet);
    let Event::Body {
        strength,
        passes,
        walls,
        distance,
        ..
    } = out[1]
    else {
        panic!("{out:?}")
    };
    assert!((strength - 0.8).abs() < 1e-4, "{strength}");
    assert_eq!((passes, walls), (1, 1));
    // Like the script, the 0.01 m nudge past the exit is not counted.
    assert!((distance - 9.84).abs() < 1e-3, "{distance}");
    // The body cost is the head zone's 8x: 80 more than the 80 left stops it in the body.
    assert!(matches!(out[2], Event::Impact { .. }));
    assert_eq!(out.len(), 3);
}

#[test]
fn too_thick_a_wall_stops_the_round() {
    let mut w = Slabs {
        walls: vec![(-5.0, -5.5, 1000.0, false, 0.0)],
    };
    let p = pen();
    let out = run(&mut w, &[body(7, -10.0)], Some(&p), [0.0, 1.6, 0.0], AHEAD);
    assert_eq!(
        out,
        vec![Event::Impact {
            point: [0.0, 1.6, -5.0]
        }]
    );
}

#[test]
fn a_one_sided_sheet_uses_its_listed_thickness() {
    struct Sheet;
    impl WorldTrace for Sheet {
        fn ray(&mut self, from: V3, dir: V3, max: f64) -> Option<WorldHit> {
            if dir[2] >= 0.0 || from[2] <= -5.0 {
                return None;
            }
            let t = (-5.0 - from[2]) / dir[2];
            (t as f64 <= max).then(|| WorldHit {
                distance: t as f64,
                point: [from[0], from[1], -5.0],
                normal: [0.0, 0.0, 1.0],
                material: 3,
                resistance: 10.0,
                ricochet: false,
                sheet: 0.05,
                pane: false,
            })
        }
    }
    let p = pen();
    let mut out = Vec::new();
    let mut skip = Vec::new();
    walk(
        &mut Sheet,
        &[],
        &[],
        &mut skip,
        Some(&p),
        [0.0; 3],
        AHEAD,
        20.0,
        &mut out,
    );
    let Event::Pass {
        sheet,
        thickness,
        material,
        ..
    } = out[0]
    else {
        panic!("{out:?}")
    };
    assert!(sheet);
    assert_eq!(material, 3);
    assert!((thickness - 0.05).abs() < 1e-5);
    assert_eq!(
        out[1],
        Event::Impact {
            point: [0.0, 0.0, -20.0]
        }
    );
}

#[test]
fn bodies_pass_through_into_the_one_behind() {
    let mut p = pen();
    p.zones = vec![1.0, 1.0];
    let sets = [body(7, -10.0), body(8, -20.0)];
    let out = run(
        &mut Slabs { walls: vec![] },
        &sets,
        Some(&p),
        [0.0, 1.6, 0.0],
        AHEAD,
    );
    let Event::Body { set: 0, .. } = out[0] else {
        panic!("{out:?}")
    };
    let Event::Body {
        set,
        strength,
        passes,
        distance,
        ..
    } = out[1]
    else {
        panic!("{out:?}")
    };
    assert_eq!((set, passes), (1, 1));
    assert!((strength - 0.9).abs() < 1e-4, "{strength}");
    assert!((distance - 19.84).abs() < 1e-3, "{distance}");
}

#[test]
fn the_world_entity_stops_the_pellet() {
    let sets = [body(0, -10.0)];
    let out = run(
        &mut Slabs { walls: vec![] },
        &sets,
        Some(&pen()),
        [0.0, 1.6, 0.0],
        AHEAD,
    );
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0], Event::Impact { .. }));
}

#[test]
fn excluded_sets_are_not_hit() {
    let sets = [body(7, -10.0)];
    let info = vec![vec![BoxInfo::default(); 2]];
    let mut skip = vec![true];
    let mut out = Vec::new();
    walk(
        &mut Slabs { walls: vec![] },
        &sets,
        &info,
        &mut skip,
        None,
        [0.0, 1.6, 0.0],
        AHEAD,
        100.0,
        &mut out,
    );
    assert_eq!(
        out,
        vec![Event::Impact {
            point: [0.0, 1.6, -100.0]
        }]
    );
}

#[test]
fn a_body_inside_a_wall_is_shielded_by_the_reverse_recheck() {
    // The muzzle is inside the wall (from z -4.9 to -5.2 is wall, origin at -5.0): the forward ray misses the face.
    let mut w = Slabs {
        walls: vec![(-4.9, -5.2, 1.0, false, 0.0)],
    };
    let out = run(&mut w, &[body(7, -6.0)], None, [0.0, 1.6, -5.0], AHEAD);
    assert_eq!(out.len(), 1, "{out:?}");
    let Event::Impact { point } = out[0] else {
        panic!()
    };
    assert!((point[2] + 5.2).abs() < 1e-4, "{point:?}");
}

#[test]
fn a_grazing_hit_on_a_ricochet_material_bounces() {
    let mut p = pen();
    p.ricochet_enabled = true;
    p.ricochet_angle = 20.0;
    let mut w = Slabs {
        walls: vec![(-5.0, -5.5, 1000.0, true, 0.0)],
    };
    let dir = normalized([1.0, 0.0, -0.2]);
    let out = run(&mut w, &[], Some(&p), [0.0; 3], dir);
    let Event::Ricochet {
        angle,
        direction,
        material,
        ..
    } = out[0]
    else {
        panic!("{out:?}")
    };
    assert_eq!(material, 0);
    assert!(angle > 10.0 && angle < 12.0, "{angle}");
    assert!(direction[2] > 0.0 && direction[0] > 0.0);
    assert!(matches!(out[1], Event::Impact { .. }));
}

#[test]
fn speed_factor_and_strength_follow_gs_penetration() {
    let mut p = pen();
    assert_eq!(p.speed_factor(50.0), 1.0);
    p.speed = 400.0;
    p.reference_speed = 400.0;
    assert_eq!(p.speed_factor(100.0), 1.0);
    assert!((p.speed_factor(25.0) - 2.0).abs() < 1e-12);
    assert_eq!(p.speed_factor(0.0), 4.0);
    assert_eq!(p.strength(0.0), 0.0);
    assert_eq!(p.strength(5.0), 0.1);
    assert_eq!(p.strength(500.0), 1.0);
}

#[test]
fn event_rows_have_the_fixed_stride() {
    let mut rows = Vec::new();
    Event::Impact {
        point: [1.0, 2.0, 3.0],
    }
    .write(2, &mut rows);
    Event::Script.write(2, &mut rows);
    assert_eq!(rows.len(), 2 * EVENT_STRIDE);
    assert_eq!(&rows[..5], &[EV_IMPACT, 2.0, 1.0, 2.0, 3.0]);
    assert_eq!(rows[EVENT_STRIDE], EV_SCRIPT);
}
