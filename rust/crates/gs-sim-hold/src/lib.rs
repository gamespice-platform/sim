//! A held weapon's third-person hand pose for one frame: fc-weapons `AwWorldModel.advance` from the action
//! sample to the holder transform.
//!
//! The script decides what moves (the action clock, the inspect share, the wall ease, the kick decay, the
//! sprint blend) and reads the engine (the person, the wall ray, the art's rest transform); [`hold`] does the
//! arithmetic between: the authored keys sampled ([`sample_field`], `AwWeaponPose.sample_field`), the degree
//! turns made bases ([`euler_yxz`], `Basis.from_euler`), the reload dip, the sprint carry and the wall tilt,
//! composed about the grip. Engine `Vector3`/`Basis` math in `f32`, the script's floats in `f64`, rounded to
//! `f32` exactly where the engine rounds them, so the answer is the script's to the bit.

use gs_sim_math::{Vec3, Xform, clampf, maxf};

/// A basis as the engine stores it: three rows.
pub type Basis = [Vec3; 3];

/// `Basis.IDENTITY`.
pub const IDENTITY: Basis = Xform::IDENTITY.rows;

/// One authored key: `[t, position in metres, rotation in degrees]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Key {
    pub t: f64,
    pub pos: Vec3,
    pub rot: Vec3,
}

/// Floats per key in [`keys_from_rows`]: `t, pos.x, pos.y, pos.z, rot.x, rot.y, rot.z`.
pub const KEY_STRIDE: usize = 7;

/// Keys from packed rows of [`KEY_STRIDE`] floats; a trailing partial row is dropped. The vectors were `f32`
/// in the script, so rounding them back is exact.
pub fn keys_from_rows(rows: &[f64]) -> Vec<Key> {
    rows.as_chunks::<KEY_STRIDE>()
        .0
        .iter()
        .map(|r| Key {
            t: r[0],
            pos: Vec3::new(r[1] as f32, r[2] as f32, r[3] as f32),
            rot: Vec3::new(r[4] as f32, r[5] as f32, r[6] as f32),
        })
        .collect()
}

#[inline]
fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

#[inline]
fn field(k: &Key, rot: bool) -> Vec3 {
    if rot { k.rot } else { k.pos }
}

/// `AwWeaponPose.sample_field`: the keys' position (`rot` false) or rotation at `t`, a Catmull-Rom with the
/// tangents scaled to each span and zero at the two ends; zero for fewer than two keys, a non-finite `t` or a
/// non-finite answer.
pub fn sample_field(keys: &[Key], t: f64, rot: bool) -> Vec3 {
    let n = keys.len();
    if n < 2 || !t.is_finite() {
        return Vec3::default();
    }
    let f = clampf(t, 0.0, 1.0);
    let mut i = 0;
    while i < n - 2 && f > keys[i + 1].t {
        i += 1;
    }
    let k0 = &keys[i.saturating_sub(1)];
    let k1 = &keys[i];
    let k2 = &keys[i + 1];
    let k3 = &keys[(i + 2).min(n - 1)];
    let (t1, t2) = (k1.t, k2.t);
    let span = t2 - t1;
    if span <= 0.0001 {
        return field(k2, rot);
    }
    let u = clampf((f - t1) / span, 0.0, 1.0);
    let p1 = field(k1, rot);
    let p2 = field(k2, rot);
    let mut m1 = Vec3::default();
    let mut m2 = Vec3::default();
    if i > 0 {
        m1 = (p2 - field(k0, rot))
            .div_scalar(maxf(t2 - k0.t, 0.0001))
            .scale(span);
    }
    if i + 2 < n {
        m2 = (field(k3, rot) - p1)
            .div_scalar(maxf(k3.t - t1, 0.0001))
            .scale(span);
    }
    let u2 = u * u;
    let u3 = u2 * u;
    let v = p1.scale(2.0 * u3 - 3.0 * u2 + 1.0)
        + m1.scale(u3 - 2.0 * u2 + u)
        + p2.scale(-2.0 * u3 + 3.0 * u2)
        + m2.scale(u3 - u2);
    if finite(v) { v } else { Vec3::default() }
}

/// The script's `deg_to_rad` on a float.
#[inline]
pub fn deg_to_rad(d: f64) -> f64 {
    d * (std::f64::consts::PI / 180.0)
}

/// The script's `smoothstep(0.0, 1.0, s)`.
#[inline]
pub fn smoothstep01(s: f64) -> f64 {
    let s = clampf(s, 0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// `Basis * Basis`: each entry `b`'s column dotted with `a`'s row, summed in row order (`Basis::tdotx`).
#[inline]
pub fn basis_mul(a: &Basis, b: &Basis) -> Basis {
    let [b0, b1, b2] = *b;
    let row = |r: Vec3| {
        Vec3::new(
            b0.x * r.x + b1.x * r.y + b2.x * r.z,
            b0.y * r.x + b1.y * r.y + b2.y * r.z,
            b0.z * r.x + b1.z * r.y + b2.z * r.z,
        )
    };
    [row(a[0]), row(a[1]), row(a[2])]
}

/// `Basis.from_euler(e)` in the default order, YXZ: `ymat * xmat * zmat`, each from the `f32` angle's `cos` and `sin`.
pub fn euler_yxz(e: Vec3) -> Basis {
    let (c, s) = (e.x.cos(), e.x.sin());
    let xmat = [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, c, -s),
        Vec3::new(0.0, s, c),
    ];
    let (c, s) = (e.y.cos(), e.y.sin());
    let ymat = [
        Vec3::new(c, 0.0, s),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-s, 0.0, c),
    ];
    let (c, s) = (e.z.cos(), e.z.sin());
    let zmat = [
        Vec3::new(c, -s, 0.0),
        Vec3::new(s, c, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ];
    basis_mul(&basis_mul(&ymat, &xmat), &zmat)
}

/// `Basis.from_euler(Vector3(deg_to_rad(d.x), deg_to_rad(d.y), deg_to_rad(d.z)))`: each turn in `f64`, then stored.
pub fn euler_degrees(d: Vec3) -> Basis {
    euler_yxz(Vec3::new(
        deg_to_rad(d.x as f64) as f32,
        deg_to_rad(d.y as f64) as f32,
        deg_to_rad(d.z as f64) as f32,
    ))
}

/// `AwWorldModelKeys.RELOAD_DIP`.
pub const RELOAD_DIP: Vec3 = Vec3::new(0.0, -0.05, 0.03);
/// `AwWorldModelKeys.RELOAD_TURN`, degrees.
pub const RELOAD_TURN: Vec3 = Vec3::new(-18.0, 12.0, 38.0);

/// What the script has settled for the frame before the pose.
#[derive(Clone, Copy, Debug, Default)]
pub struct HoldIn {
    /// An action was playing at the frame's start (`_action < 1.0`), so its keys are sampled at [`HoldIn::action`].
    pub acting: bool,
    /// The action clock after this frame's step.
    pub action: f64,
    /// The inspect shows (`_inspect_shown > 0`), used when not acting.
    pub looking: bool,
    /// `_inspect_turn`'s position and degrees, already scaled by its share.
    pub looked_moved: Vec3,
    pub looked_degrees: Vec3,
    /// `_reload_blend` when the gun dips on its own (no carry), else zero.
    pub reload: f64,
    /// `_rest()`: the art's world transform scaled for its pose.
    pub rest: Xform,
    /// `_sprint` when there is art, else zero; with the art's (or the pose's) sprint offset and degrees.
    pub sprint: f64,
    pub sprint_offset: Vec3,
    pub sprint_degrees: Vec3,
    /// `_wall_amount * wall_tilt_per_m`: the wall tilt in degrees.
    pub wall_tilt: f64,
    /// `_kick + _wall_amount`: metres back along the grip's z.
    pub push: f64,
}

/// The sprint carry's offset and turn (`AwWorldModel._sprint_carry`).
pub fn sprint_carry(sprint: f64, offset: Vec3, degrees: Vec3) -> (Vec3, Basis) {
    if sprint <= 0.0 {
        return (Vec3::default(), IDENTITY);
    }
    (offset.scale(sprint), euler_degrees(degrees.scale(sprint)))
}

/// The holder's transform for the frame, as `AwWorldModel.advance` sets it.
pub fn hold(keys: &[Key], h: &HoldIn) -> Xform {
    let mut turn = IDENTITY;
    let mut moved = Vec3::default();
    if h.acting {
        moved = sample_field(keys, h.action, false);
        turn = euler_degrees(sample_field(keys, h.action, true));
    } else if h.looking {
        moved = h.looked_moved;
        turn = euler_degrees(h.looked_degrees);
    }
    if h.reload > 0.0 {
        let share = smoothstep01(h.reload);
        moved = moved + RELOAD_DIP.scale(share);
        turn = basis_mul(
            &turn,
            &euler_yxz(RELOAD_TURN.scale(share).scale(std::f64::consts::PI / 180.0)),
        );
    }
    let (sprint_offset, sprint_turn) = sprint_carry(h.sprint, h.sprint_offset, h.sprint_degrees);
    let low = euler_yxz(Vec3::new(-deg_to_rad(h.wall_tilt) as f32, 0.0, 0.0));
    let rows = basis_mul(
        &basis_mul(&basis_mul(&low, &sprint_turn), &turn),
        &h.rest.rows,
    );
    let origin = h.rest.origin + moved + sprint_offset + Vec3::new(0.0, 0.0, h.push as f32);
    Xform { rows, origin }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Vec<Key> {
        keys_from_rows(&[
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, //
            0.4, -0.05, 0.04, 0.12, 25.0, -12.0, 8.0, //
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ])
    }

    #[test]
    fn samples_run_through_the_keys_and_end_at_rest() {
        let k = keys();
        assert_eq!(sample_field(&k, 0.0, false), Vec3::default());
        assert_eq!(sample_field(&k, 1.0, true), Vec3::default());
        assert_eq!(sample_field(&k, 0.4, false), k[1].pos);
        assert_eq!(sample_field(&k, 0.4, true), k[1].rot);
        assert_eq!(sample_field(&k, f64::NAN, false), Vec3::default());
        assert_eq!(sample_field(&k[..1], 0.5, false), Vec3::default());
        let mid = sample_field(&k, 0.7, true);
        assert!(mid.x > 0.0 && mid.x < 25.0);
    }

    #[test]
    fn a_zero_euler_is_the_identity() {
        assert_eq!(euler_yxz(Vec3::default()), IDENTITY);
        assert_eq!(euler_degrees(Vec3::default()), IDENTITY);
    }

    #[test]
    fn yaw_turns_minus_z_toward_minus_x() {
        let b = euler_degrees(Vec3::new(0.0, 90.0, 0.0));
        let f = Xform {
            rows: b,
            origin: Vec3::default(),
        }
        .basis_xform(Vec3::new(0.0, 0.0, -1.0));
        assert!((f.x + 1.0).abs() < 1e-6 && f.z.abs() < 1e-6);
    }

    #[test]
    fn a_still_hold_is_the_rest_pushed_back() {
        let rest = Xform {
            rows: IDENTITY,
            origin: Vec3::new(0.1, -0.2, 0.3),
        };
        let h = HoldIn {
            action: 1.0,
            rest,
            push: 0.05,
            ..Default::default()
        };
        let x = hold(&[], &h);
        assert_eq!(x.rows, IDENTITY);
        assert_eq!(
            x.origin,
            Vec3::new(0.1, -0.2, 0.3) + Vec3::new(0.0, 0.0, 0.05)
        );
    }

    #[test]
    fn a_reload_dips_and_turns_without_a_carry() {
        let h = HoldIn {
            action: 1.0,
            rest: Xform::IDENTITY,
            reload: 1.0,
            ..Default::default()
        };
        let x = hold(&[], &h);
        assert_eq!(x.origin, RELOAD_DIP);
        assert_ne!(x.rows, IDENTITY);
    }
}
