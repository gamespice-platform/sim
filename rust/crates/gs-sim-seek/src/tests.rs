use super::*;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn rules() -> Rules {
    Rules {
        now: 100.0,
        step: 4.0 / 32.0,
        base: 0.2,
        fov: 90.0,
        sight: 40.0,
        memory: 6.0,
        map_knowledge: 0.5,
        aggression: 0.5,
        round: 1,
    }
}

fn hunter(id: &str) -> Hunter {
    Hunter {
        id: id.into(),
        eye: v(0.0, 1.7, 0.0),
        forward: v(0.0, 0.0, -1.0),
        found: -1,
        found_at: f64::NEG_INFINITY,
        sense: true,
        observe: true,
        ..Hunter::default()
    }
}

fn prop(id: &str, at: Vec3) -> Prop {
    Prop {
        id: id.into(),
        at,
        hull: 1.0,
        alive: true,
        disguised: true,
        kind: 0,
        ..Prop::default()
    }
}

#[test]
fn hash_is_godots_djb2() {
    assert_eq!(godot_string_hash(""), 5381);
    assert_eq!(godot_string_hash("hello"), 261_238_937);
    let d = memory_draw("u900", -1, "u901", 3);
    assert!((0.0..=1.0).contains(&d));
    assert_eq!(
        d,
        f64::from(godot_string_hash("u900|-1|u901|3") & 0xFFFF) / 65535.0
    );
}

#[test]
fn prey_is_the_nearest_known_in_sight() {
    let mut seek = Seek::default();
    let mut moving = prop("a", v(0.0, 0.0, -8.0));
    moving.velocity = v(2.0, 0.0, 0.0);
    let still = prop("b", v(0.0, 0.0, -4.0));
    let mut marked = prop("c", v(0.0, 0.0, 6.0));
    marked.marked = true;
    let props = vec![moving, still, marked];
    let mut out = Vec::new();
    let mut rays = 0;
    seek.sense(
        &rules(),
        &props,
        &[],
        &[hunter("h")],
        &mut |_, _| {
            rays += 1;
            true
        },
        &mut out,
    );
    // The still one is not known; the marked one behind is nearer and needs no view.
    assert_eq!(out[0].prey, 2);
    assert!(!out[0].sensed);
    assert_eq!(rays, 2);
}

#[test]
fn found_is_known_for_its_memory() {
    let mut seek = Seek::default();
    let props = vec![prop("a", v(0.0, 0.0, -4.0))];
    let mut h = hunter("h");
    h.found = 0;
    h.found_at = 95.0;
    let mut out = Vec::new();
    seek.sense(
        &rules(),
        &props,
        &[],
        &[h.clone()],
        &mut |_, _| true,
        &mut out,
    );
    assert_eq!(out[0].prey, 0);
    h.found_at = 80.0;
    seek.sense(&rules(), &props, &[], &[h], &mut |_, _| true, &mut out);
    assert_eq!(out[0].prey, -1);
}

#[test]
fn observe_crosses_off_in_view_and_marks_seen() {
    let mut seek = Seek::default();
    let m = seek.mind("h");
    m.search = Search {
        at: vec![
            v(0.0, 0.0, -5.0),
            v(0.0, 0.0, 5.0),
            v(0.0, 0.0, -30.0),
            v(1.0, 0.0, -3.0),
        ],
        seen_at: vec![
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            95.0,
        ],
        recheck: 75.0,
        look_reach: 9.0,
    };
    let mut out = Vec::new();
    seek.sense(
        &rules(),
        &[],
        &[],
        &[hunter("h")],
        &mut |_, _| true,
        &mut out,
    );
    assert_eq!(out[0].crossed, vec![0]);
    assert_eq!(seek.minds["h"].search.seen_at[0], 100.0);
    // Looked at lately: not again.
    seek.sense(
        &rules(),
        &[],
        &[],
        &[hunter("h")],
        &mut |_, _| true,
        &mut out,
    );
    assert!(out[0].crossed.is_empty());
}

#[test]
fn doubt_rows_pieces_then_props_with_cleared_and_sight() {
    let mut seek = Seek::default();
    seek.set_pieces(
        vec![
            Piece {
                middle: v(0.0, 0.5, -4.0),
                feet: v(0.0, 0.0, -4.0),
                kind: 1,
                broken: false,
            },
            Piece {
                middle: v(0.0, 0.5, -6.0),
                feet: v(0.0, 0.0, -6.0),
                kind: 1,
                broken: false,
            },
            Piece {
                middle: v(0.0, 0.5, 6.0),
                feet: v(0.0, 0.0, 6.0),
                kind: 2,
                broken: false,
            },
        ],
        vec![1, 1, 0],
    );
    seek.mind("h").cleared.insert("p1".into(), 10.0);
    seek.mind("h").cleared.insert("ub".into(), 70.0);
    let mut tilted = prop("a", v(1.0, 0.0, -5.0));
    tilted.tilted = true;
    let props = vec![tilted, prop("b", v(-1.0, 0.0, -5.0))];
    let taunts = [Taunt {
        at: v(0.0, 0.0, -4.0),
        t: 95.0,
    }];
    let mut out = Vec::new();
    seek.sense(
        &rules(),
        &props,
        &taunts,
        &[hunter("h")],
        &mut |_, _| true,
        &mut out,
    );
    let keys: Vec<i32> = out[0].doubt.iter().map(|r| r.key).collect();
    // Piece 1 is cleared for the round; prop b's miss is old (30 s): doubted again.
    assert_eq!(keys, vec![0, -1, -2]);
    let r = rules();
    let d = f64::from(v(0.0, 1.7, 0.0).distance_to(v(0.0, 0.5, -4.0)));
    let mut odd = r.base;
    if memory_draw("h", 0, "", r.round) < r.map_knowledge / 1.6 {
        odd *= 0.15;
    }
    odd += 1.4;
    let closeness = clampf(1.0 - d / DOUBT_REACH, 0.0, 1.0);
    assert_eq!(
        out[0].doubt[0].delta,
        r.step * odd * (0.5 + closeness) * (0.6 + r.aggression * 0.8)
    );
    // A blocked line: no rows.
    seek.sense(
        &rules(),
        &props,
        &taunts,
        &[hunter("h")],
        &mut |_, _| false,
        &mut out,
    );
    assert!(out[0].doubt.is_empty());
}

#[test]
fn a_test_ending_clears_its_key_before_the_doubt() {
    let mut seek = Seek::default();
    seek.set_pieces(
        vec![Piece {
            middle: v(0.0, 0.5, -4.0),
            feet: v(0.0, 0.0, -4.0),
            kind: 1,
            broken: false,
        }],
        vec![0],
    );
    let mut h = hunter("h");
    h.clearing = Some("p0".into());
    let mut out = Vec::new();
    seek.sense(&rules(), &[], &[], &[h], &mut |_, _| true, &mut out);
    assert!(out[0].doubt.is_empty());
    assert_eq!(seek.minds["h"].cleared["p0"], 100.0);
}

#[test]
fn kin_counts_broken_pieces_too() {
    let pieces = vec![
        Piece {
            feet: v(0.0, 0.0, 0.0),
            kind: 3,
            broken: true,
            ..Piece::default()
        },
        Piece {
            feet: v(1.0, 0.0, 0.0),
            kind: 3,
            ..Piece::default()
        },
        Piece {
            feet: v(5.0, 0.0, 0.0),
            kind: 3,
            ..Piece::default()
        },
        Piece {
            feet: v(0.5, 0.0, 0.0),
            kind: 4,
            ..Piece::default()
        },
    ];
    assert_eq!(kin_count(&pieces, 3, v(0.0, 0.0, 0.0)), 2);
    assert_eq!(kin_count(&pieces, -1, v(0.0, 0.0, 0.0)), 0);
}
