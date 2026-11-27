use super::Posture;
use super::shot::*;
use gs_sim_math::Vec3;

fn ahead() -> Vec3 {
    Vec3::new(0.0, 0.0, -1.0)
}

fn angle_deg(a: Vec3, b: Vec3) -> f64 {
    (a.normalized().dot(b.normalized()) as f64)
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

#[test]
fn mix_is_splitmix64() {
    // splitmix64's first output from seed 0 (the finaliser of 0 + gamma).
    assert_eq!(mix(0) as u64, 0xE220_A839_7B1D_CDAF);
    assert_eq!(mix4(1, 2, 3, 4), mix4(1, 2, 3, 4));
    assert_ne!(mix4(1, 2, 3, 4), mix4(2, 1, 3, 4));
}

#[test]
fn unit_covers_the_whole_range() {
    let (mut lo, mut hi) = (1.0f64, 0.0f64);
    for i in 0..4096 {
        let u = unit_from(mix4(7, i, 0, 0));
        assert!((0.0..1.0).contains(&u));
        lo = lo.min(u);
        hi = hi.max(u);
    }
    assert!(lo < 0.01 && hi > 0.99, "{lo} {hi}");
}

#[test]
fn cone_stays_inside_its_angle() {
    for pellet in 0..64 {
        let d = cone(ahead(), 4.0, 3, 100, 2, pellet);
        assert!(d.is_normalized());
        assert!(angle_deg(d, ahead()) <= 4.0 + 1e-3);
    }
    assert_eq!(cone(ahead(), 0.0, 3, 100, 2, 1), ahead());
}

#[test]
fn straight_up_does_not_degenerate() {
    let up = Vec3::new(0.0, 1.0, 0.0);
    let a = cone(up, 5.0, 1, 1, 0, 0);
    let b = cone(up, 5.0, 1, 1, 0, 1);
    assert_ne!(a, b);
    assert!(angle_deg(a, up) <= 5.0 + 1e-3);
}

#[test]
fn fixed_cone_rings_round_a_centre() {
    assert_eq!(fixed_cone(ahead(), 6.0, 0, 9), ahead());
    assert_eq!(fixed_cone(ahead(), 6.0, 3, 1), ahead());
    for pellet in 1..9 {
        let a = angle_deg(fixed_cone(ahead(), 6.0, pellet, 9), ahead());
        assert!((a - 6.0).abs() < 1e-3, "{a}");
    }
}

#[test]
fn scatter_makes_one_per_pellet() {
    let mut out = vec![Vec3::default(); 3];
    let shot = Shot {
        direction: ahead(),
        spread: 2.0,
        entity: 5,
        tick: 9,
        index: 1,
        pellet_count: 0,
        fixed: false,
    };
    scatter(&shot, &mut out);
    assert_eq!(out, vec![cone(ahead(), 2.0, 5, 9, 1, 0)]);
    scatter(
        &Shot {
            pellet_count: 8,
            fixed: true,
            ..shot
        },
        &mut out,
    );
    assert_eq!(out.len(), 8);
    assert_eq!(out[0], ahead());
}

#[test]
fn seed_and_drift_match_the_script() {
    // h = 17; h = h * 31 + c, masked, over "ak".
    assert_eq!(seed_of_id(b"ak"), ((17 * 31 + 97) * 31 + 107) & 0x7fff_ffff);
    for i in 0..200 {
        let d = drift(1234, i);
        assert!((-1.0..=1.0).contains(&d));
    }
    // The lean is the seed's bit 3.
    let lean_pos: f64 = (0..400).map(|i| drift(0, i)).sum();
    let lean_neg: f64 = (0..400).map(|i| drift(8, i)).sum();
    assert!(lean_pos > 0.0 && lean_neg < 0.0);
}

#[test]
fn offset_pitches_up_and_yaws() {
    assert_eq!(offset_direction(ahead(), 0.0, 0.0), ahead());
    let up = Vec3::new(0.0, 1.0, 0.0);
    assert_eq!(offset_direction(up, 3.0, 1.0), up);
    let d = offset_direction(ahead(), 2.0, 0.0);
    assert!(d.y > 0.0, "pitch raises the shot: {d:?}");
    assert!((angle_deg(d, ahead()) - 2.0).abs() < 1e-3);
    let d = offset_direction(ahead(), 0.0, 3.0);
    assert!(d.y.abs() < 1e-6 && (angle_deg(d, ahead()) - 3.0).abs() < 1e-3);
}

fn rules() -> Recoil {
    Recoil {
        rate: 60,
        reset: 0.35,
        recovery: 6.0,
        build: 0.12,
        build_max: 0.6,
        pitch: 1.2,
        yaw: 0.4,
        share: 0.6,
        follow: 0.5,
        seed: seed_of_id(b"rifle"),
        scale: 1.0,
        mult_aim: 0.7,
        mult_crouch: 0.8,
        mult_air: 1.5,
        mult_move: 1.2,
        weapon_aim: 0.9,
        weapon_crouch: 1.0,
        weapon_move: 1.1,
    }
}

fn at(tick: i64) -> Posture {
    Posture {
        tick,
        ..Posture::SETTLED
    }
}

#[test]
fn recoil_builds_through_a_burst_and_resets() {
    let r = rules();
    let mut s = RecoilState::default();
    let first = recoil(&r, &at(100), &mut s);
    assert_eq!(s.n, Some(1));
    assert_eq!(s.tick, Some(100));
    // A first shot has no residue: the bullet takes the unshared part of its own kick.
    assert_eq!(first.view.0, (1.2f64 as f32) * (0.6f64 as f32));
    let second = recoil(&r, &at(106), &mut s);
    assert_eq!(s.n, Some(2));
    assert!(second.view.0 > first.view.0, "the burst grows");
    // A second past `reset` starts the burst over.
    let later = recoil(&r, &at(106 + 60), &mut s);
    assert_eq!(s.n, Some(1));
    assert_eq!(later.view.0, first.view.0);
}

#[test]
fn recoil_posture_multiplies_rules_and_weapon() {
    let r = rules();
    let p = Posture {
        aiming: true,
        crouched: true,
        airborne: true,
        ..at(0)
    };
    assert_eq!(
        r.posture(&p),
        (((1.0 * 0.7) * 0.8) * 1.5) * ((0.9 * 1.0) * 1.1)
    );
    let moving = Posture {
        speed: 0.5,
        ..at(0)
    };
    assert_eq!(r.posture(&moving), 1.2 * 1.1);
    assert_eq!(Recoil::from_row(&[0.0; RECOIL_LEN - 1]), None);
    assert_eq!(
        Recoil::from_row(&[0.0; RECOIL_LEN]).map(|r| r.rate),
        Some(1)
    );
}

/// With no posture and a whole share, the preview is the offset the use itself then fires with, bit for bit.
#[test]
fn recoil_preview_is_the_next_use_offset() {
    let r = Recoil {
        rate: 64,
        reset: 0.4,
        recovery: 6.5,
        build: 0.11,
        build_max: 0.9,
        pitch: 1.7,
        yaw: 0.6,
        share: 0.35,
        follow: 0.8,
        seed: 918_273,
        scale: 1.0,
        mult_aim: 0.7,
        mult_crouch: 0.8,
        mult_air: 1.5,
        mult_move: 1.25,
        weapon_aim: 1.0,
        weapon_crouch: 1.0,
        weapon_move: 1.0,
    };
    let mut s = RecoilState::default();
    for (i, tick) in [100i64, 104, 108, 109, 160, 161, 162, 400]
        .into_iter()
        .enumerate()
    {
        let row = [
            r.reset,
            r.recovery,
            1.0,
            r.build,
            1.0,
            r.build_max,
            r.pitch,
            1.0,
            r.yaw,
            1.0,
            r.share,
            r.follow,
            r.seed as f64,
            s.tick.unwrap_or(-1_000_000) as f64,
            s.n.unwrap_or(0) as f64,
            s.ax.unwrap_or(0.0),
            s.ay.unwrap_or(0.0),
            1.0,
        ];
        let (x, y) = recoil_preview(&row, tick, 64.0).expect("a whole row");
        let kick = recoil(&r, &at(tick), &mut s);
        assert_eq!(
            (x.to_bits(), y.to_bits()),
            (kick.offset.0.to_bits(), kick.offset.1.to_bits()),
            "use {i}"
        );
    }
    assert!(recoil_preview(&[0.0; PREVIEW_LEN - 1], 0, 64.0).is_none());
}
