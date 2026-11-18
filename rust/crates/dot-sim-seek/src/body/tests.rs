use super::*;

fn body(x: f32, z: f32, side: i32) -> Body {
    let at = Vec3::new(x, 0.0, z);
    Body {
        at,
        eye: at + Vec3::new(0.0, 1.6, 0.0),
        forward: Vec3::new(0.0, 0.0, -1.0),
        velocity: Vec3::new(2.0, 0.0, 0.0),
        crouch: 0.0,
        alive: true,
        side,
        ..Body::default()
    }
}

fn clear(_: Vec3, _: Vec3) -> bool {
    true
}

#[test]
fn sees_is_reach_then_cone_then_line() {
    let eye = Vec3::new(0.0, 1.6, 0.0);
    let ahead = Vec3::new(0.0, 0.0, -1.0);
    let mut rays = 0;
    let mut los = |_: Vec3, _: Vec3| {
        rays += 1;
        true
    };
    assert!(sees(
        eye,
        ahead,
        90.0,
        10.0,
        Vec3::new(0.0, 1.6, -5.0),
        &mut los
    ));
    assert!(!sees(
        eye,
        ahead,
        90.0,
        4.0,
        Vec3::new(0.0, 1.6, -5.0),
        &mut los
    ));
    assert!(!sees(
        eye,
        ahead,
        90.0,
        10.0,
        Vec3::new(0.0, 1.6, 5.0),
        &mut los
    ));
    // On the eye: no cone test.
    assert!(sees(eye, ahead, 90.0, 10.0, eye, &mut los));
    assert_eq!(rays, 2);
    assert!(!sees(
        eye,
        ahead,
        90.0,
        10.0,
        Vec3::new(0.0, 1.6, -5.0),
        &mut |_, _| false
    ));
}

#[test]
fn a_still_crouched_body_is_seen_only_whole_and_near() {
    let rules = BodyRules::default();
    let w = body(0.0, 0.0, 1);
    let mut o = body(0.0, -4.0, 2);
    o.velocity = Vec3::default();
    o.crouch = 1.0;
    o.eye = o.at + Vec3::new(0.0, 1.0, 0.0);
    // Sight 30, still and low: 3 m, so 4 m away is unseen.
    assert!(!body_sees(&rules, 90.0, 30.0, &w, &o, &mut clear));
    assert!(body_sees(&rules, 90.0, 50.0, &w, &o, &mut clear));
    // The chest hidden: still and low needs both.
    let chest = o.at + Vec3::new(0.0, 0.6, 0.0);
    let mut chest_hidden = |_: Vec3, b: Vec3| b != chest;
    assert!(!body_sees(&rules, 90.0, 50.0, &w, &o, &mut chest_hidden));
    // Moving: either is enough.
    o.velocity = Vec3::new(3.0, 0.0, 0.0);
    o.crouch = 0.0;
    assert!(body_sees(&rules, 90.0, 50.0, &w, &o, &mut chest_hidden));
}

#[test]
fn a_body_at_arms_length_is_felt_behind() {
    let rules = BodyRules::default();
    let w = body(0.0, 0.0, 1);
    let o = body(0.0, 1.0, 2); // behind the -z look
    assert!(body_sees(&rules, 90.0, 30.0, &w, &o, &mut clear));
    assert!(!body_sees(&rules, 90.0, 30.0, &w, &o, &mut |_, _| false));
}

#[test]
fn nearest_seen_picks_the_nearest_on_the_side_in_order() {
    let rules = BodyRules::default();
    let mut bodies = vec![
        body(0.0, 0.0, 1),
        body(0.0, -8.0, 2),
        body(0.0, -5.0, 2),
        body(0.0, -3.0, 1),
        body(0.0, -2.5, 2),
    ];
    bodies[4].watching = true;
    let at = bodies[0].at;
    let ask =
        |b: &[Body], look| nearest_seen(b, &rules, 90.0, 30.0, 0, at, 2, 30.0, look, &mut clear);
    assert_eq!(ask(&bodies, Look::Seekers), 2);
    // Within bounds the seekers' ask.
    assert_eq!(
        nearest_seen(
            &bodies,
            &rules,
            90.0,
            30.0,
            0,
            at,
            2,
            5.0,
            Look::Seekers,
            &mut clear
        ),
        -1
    );
    // Prey: a marked body behind is known, a caught one is not.
    bodies[1].at = Vec3::new(0.0, 0.0, 1.5);
    bodies[1].eye = bodies[1].at + Vec3::new(0.0, 1.6, 0.0);
    bodies[1].marked = true;
    assert_eq!(
        nearest_seen(
            &bodies,
            &rules,
            90.0,
            30.0,
            0,
            at,
            2,
            0.0,
            Look::Prey,
            &mut |_, _| false
        ),
        1
    );
    bodies[1].caught = true;
    assert_eq!(
        nearest_seen(
            &bodies,
            &rules,
            90.0,
            30.0,
            0,
            at,
            2,
            0.0,
            Look::Prey,
            &mut |_, _| false
        ),
        -1
    );
    assert_eq!(
        nearest_seen(
            &bodies,
            &rules,
            90.0,
            30.0,
            99,
            at,
            2,
            30.0,
            Look::Seekers,
            &mut clear
        ),
        -1
    );
}
