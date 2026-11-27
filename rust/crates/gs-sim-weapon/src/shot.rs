//! A hitscan use's recoil and pellets: gs-weapon's `GSWeaponRecoilRules.apply` (with `drift`, `seed_for` and the
//! weapon's own `recoil_posture`), `GSWeaponRecoilRules.offset_direction`, and gs-combat's `GSShot.scatter`
//! (`GSSpread.cone`, `fixed_cone`, `_rotate_into`, over gs-core's `GSRandomStream.mix4`). Spelled in the scripts'
//! order and precision: `f64` for script floats, `f32` for anything held in a `Vector2` or `Vector3`, wrapping `i64`
//! for script ints, so the native shot and the script's agree to the bit.

use crate::Posture;
use gs_sim_math::{Vec3, Xform, clampf, maxf, minf};

/// `GSRandomStream._GAMMA` (0x9E3779B97F4A7C15 as a signed int).
pub const GAMMA: i64 = -7046029254386353131;
const MIX_A: i64 = -4658895280553007687;
const MIX_B: i64 = -7723592293110705685;

/// `@GlobalScope.TAU`.
const TAU: f64 = std::f64::consts::TAU;

/// `deg_to_rad` (`Math::deg_to_rad`): the degrees times `PI / 180`, in `f64`.
#[inline]
pub fn deg_to_rad(deg: f64) -> f64 {
    deg * (std::f64::consts::PI / 180.0)
}

/// `>>>` as the script spells it (`_unsigned_shift`): a zero-filling right shift.
#[inline]
fn ushr(v: i64, bits: u32) -> i64 {
    ((v as u64) >> bits) as i64
}

/// `GSRandomStream._mix`: the splitmix64 finaliser after one gamma step.
#[inline]
pub fn mix(x: i64) -> i64 {
    let mut z = x.wrapping_add(GAMMA);
    z = (z ^ ushr(z, 30)).wrapping_mul(MIX_A);
    z = (z ^ ushr(z, 27)).wrapping_mul(MIX_B);
    z ^ ushr(z, 31)
}

/// `GSRandomStream.mix4` (`GSSpread.hash4`).
#[inline]
pub fn mix4(a: i64, b: i64, c: i64, d: i64) -> i64 {
    let mut x = mix(a);
    x = mix(x ^ b.wrapping_add(GAMMA));
    x = mix(x ^ c.wrapping_add(GAMMA));
    mix(x ^ d.wrapping_add(GAMMA))
}

/// `GSRandomStream.unit_from`: the top 24 bits as a float in `[0, 1)`.
#[inline]
pub fn unit_from(v: i64) -> f64 {
    ushr(v, 40) as f64 / 16777216.0
}

/// `Vector3(x, y, z)` from script floats: each rounded to `f32`.
#[inline]
fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x as f32, y as f32, z as f32)
}

/// `GSSpread._rotate_into`: `local`, expressed around +Z, turned onto `direction`.
pub fn rotate_into(direction: Vec3, local: Vec3) -> Vec3 {
    let forward = direction.normalized();
    let helper = if (forward.y as f64).abs() < 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let right = helper.cross(forward).normalized();
    let up = forward.cross(right);
    (right.scale(local.x as f64) + up.scale(local.y as f64) + forward.scale(local.z as f64))
        .normalized()
}

/// `GSSpread.cone`: pellet `pellet` of shot `shot` scattered uniformly over the solid angle of a cone of
/// `angle_deg` degrees around `direction`.
pub fn cone(
    direction: Vec3,
    angle_deg: f64,
    entity: i64,
    tick: i64,
    shot: i64,
    pellet: i64,
) -> Vec3 {
    if angle_deg <= 0.0 {
        return direction;
    }
    let h = mix4(entity, tick, shot, pellet);
    let azimuth = unit_from(h) * TAU;
    let radial = unit_from(mix4(h, pellet, shot, tick));
    let max_cos = deg_to_rad(angle_deg).cos();
    let cos_theta = 1.0 - radial * (1.0 - max_cos);
    let sin_theta = maxf(0.0, 1.0 - cos_theta * cos_theta).sqrt();
    rotate_into(
        direction,
        v3(
            azimuth.cos() * sin_theta,
            azimuth.sin() * sin_theta,
            cos_theta,
        ),
    )
}

/// `GSSpread.fixed_cone`: pellet 0 dead centre, the rest of `count` on a ring at `angle_deg`.
pub fn fixed_cone(direction: Vec3, angle_deg: f64, pellet: i64, count: i64) -> Vec3 {
    if angle_deg <= 0.0 || count <= 1 || pellet == 0 {
        return direction;
    }
    let ring = count - 1;
    let azimuth = TAU * (pellet - 1) as f64 / ring as f64;
    let theta = deg_to_rad(angle_deg);
    rotate_into(
        direction,
        v3(
            azimuth.cos() * theta.sin(),
            azimuth.sin() * theta.sin(),
            theta.cos(),
        ),
    )
}

/// One shot as `GSShot.scatter` reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shot {
    pub direction: Vec3,
    pub spread: f64,
    pub entity: i64,
    pub tick: i64,
    pub index: i64,
    pub pellet_count: i64,
    pub fixed: bool,
}

/// `GSShot.scatter`: the pellet directions of `shot`, into `out` (cleared first).
pub fn scatter(shot: &Shot, out: &mut Vec<Vec3>) {
    out.clear();
    let count = shot.pellet_count.max(1);
    for pellet in 0..count {
        out.push(if shot.fixed {
            fixed_cone(shot.direction, shot.spread, pellet, count)
        } else {
            cone(
                shot.direction,
                shot.spread,
                shot.entity,
                shot.tick,
                shot.index,
                pellet,
            )
        });
    }
}

/// `GSWeaponRecoilRules.seed_for` with no `recoil_seed`: the weapon id's UTF-8 bytes folded into 31 bits.
pub fn seed_of_id(id: &[u8]) -> i64 {
    let mut h: i64 = 17;
    for &c in id {
        h = (h.wrapping_mul(31).wrapping_add(c as i64)) & 0x7fff_ffff;
    }
    h
}

/// `GSWeaponRecoilRules.drift`: shot `index`'s sideways drift in -1..1, a third of it a lean to the seed's side.
pub fn drift(seed: i64, index: i64) -> f64 {
    let h =
        (seed.wrapping_mul(73856093) ^ index.wrapping_add(1).wrapping_mul(19349663)) & 0x7fff_ffff;
    let noise = (h % 2001) as f64 / 1000.0 - 1.0;
    let lean = if (seed >> 3) & 1 == 0 { 1.0 } else { -1.0 };
    clampf(noise * 0.67 + lean * 0.33, -1.0, 1.0)
}

/// `GSWeaponRecoilRules.offset_direction`: `dir` turned by `(ox, oy)` degrees (pitch about the view's right, then
/// yaw about up), each angle rounded to `f32` as `Vector3.rotated` takes it.
pub fn offset_direction(dir: Vec3, ox: f32, oy: f32) -> Vec3 {
    if ox == 0.0 && oy == 0.0 {
        return dir;
    }
    let up = Vec3::new(0.0, 1.0, 0.0);
    let right = dir.cross(up);
    if (right.length_squared() as f64) < 0.0001 {
        return dir;
    }
    let right = right.normalized();
    let pitched = Xform::from_axis_angle(right, deg_to_rad(ox as f64) as f32).basis_xform(dir);
    Xform::from_axis_angle(up, deg_to_rad(oy as f64) as f32)
        .basis_xform(pitched)
        .normalized()
}

/// One weapon's recoil numbers under the arsenal's `GSWeaponRecoilRules`, each product already taken in the script's
/// order (`b.recoil_pitch * vertical`, ...), so only the per-use arithmetic is left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Recoil {
    /// `maxi(tick_rate, 1)`.
    pub rate: i64,
    /// `reset`: seconds since the last use after which the burst starts over.
    pub reset: f64,
    /// `b.recoil_recovery * recovery`.
    pub recovery: f64,
    /// `b.recoil_build * build`.
    pub build: f64,
    /// `b.recoil_build_max * build`.
    pub build_max: f64,
    /// `b.recoil_pitch * vertical`.
    pub pitch: f64,
    /// `b.recoil_yaw * horizontal`.
    pub yaw: f64,
    /// `b.recoil_camera` when 0 or more, else `punch`.
    pub share: f64,
    pub follow: f64,
    /// `seed_for(b, id)`.
    pub seed: i64,
    /// The rules' `posture` terms.
    pub scale: f64,
    pub mult_aim: f64,
    pub mult_crouch: f64,
    pub mult_air: f64,
    pub mult_move: f64,
    /// The weapon's `recoil_posture` terms (the field when 0 or more, else the class preset).
    pub weapon_aim: f64,
    pub weapon_crouch: f64,
    pub weapon_move: f64,
}

/// Number of values in a packed [`Recoil`] row (see [`Recoil::from_row`]).
pub const RECOIL_LEN: usize = 18;

impl Recoil {
    /// `[rate, reset, recovery, build, build_max, pitch, yaw, share, follow, seed, scale, mult_aim, mult_crouch,
    /// mult_air, mult_move, weapon_aim, weapon_crouch, weapon_move]`; `None` when short.
    pub fn from_row(r: &[f64]) -> Option<Self> {
        if r.len() < RECOIL_LEN {
            return None;
        }
        Some(Self {
            rate: (r[0] as i64).max(1),
            reset: r[1],
            recovery: r[2],
            build: r[3],
            build_max: r[4],
            pitch: r[5],
            yaw: r[6],
            share: r[7],
            follow: r[8],
            seed: r[9] as i64,
            scale: r[10],
            mult_aim: r[11],
            mult_crouch: r[12],
            mult_air: r[13],
            mult_move: r[14],
            weapon_aim: r[15],
            weapon_crouch: r[16],
            weapon_move: r[17],
        })
    }

    /// `rules.posture(ctx) * weapon.recoil_posture(b, ctx)`.
    pub fn posture(&self, p: &Posture) -> f64 {
        let mut f = self.scale;
        if p.aiming {
            f *= self.mult_aim;
        }
        if p.crouched {
            f *= self.mult_crouch;
        }
        if p.airborne {
            f *= self.mult_air;
        } else if p.speed >= 0.5 {
            f *= self.mult_move;
        }
        let mut w = 1.0;
        if p.aiming {
            w *= self.weapon_aim;
        }
        if p.crouched {
            w *= self.weapon_crouch;
        }
        if p.airborne || p.speed >= 0.5 {
            w *= self.weapon_move;
        }
        f * w
    }
}

/// The behaviour's recoil keys (`recoil_n`, `recoil_tick`, `recoil_ax`, `recoil_ay`), `None` when absent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RecoilState {
    pub n: Option<i64>,
    pub tick: Option<i64>,
    pub ax: Option<f64>,
    pub ay: Option<f64>,
}

/// What a use's recoil hands back: the outcome's view kick (`out.recoil`) and the degrees the bullet is offset by.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Kick {
    pub view: (f32, f32),
    pub offset: (f32, f32),
}

/// `GSWeaponRecoilRules.apply` for the use at `p` (its tick and posture): advances the burst in `s` (all four keys
/// set) and returns the kick. The outcome's `recoil_recovery` is [`Recoil::recovery`].
pub fn recoil(r: &Recoil, p: &Posture, s: &mut RecoilState) -> Kick {
    let rate = r.rate as f64;
    let last = s.tick.unwrap_or(-1_000_000);
    let elapsed = maxf(p.tick.wrapping_sub(last) as f64 / rate, 0.0);
    let n = if elapsed <= r.reset {
        s.n.unwrap_or(0)
    } else {
        0
    };
    let decay = (-r.recovery * elapsed).exp() as f32;
    let mut acc = (
        s.ax.unwrap_or(0.0) as f32 * decay,
        s.ay.unwrap_or(0.0) as f32 * decay,
    );
    let grow = 1.0 + minf(r.build * n as f64, r.build_max);
    let f = r.posture(p);
    let kick = (
        (r.pitch * grow * f) as f32,
        (r.yaw * grow * f * drift(r.seed, n)) as f32,
    );
    let residual = acc;
    acc = (acc.0 + kick.0, acc.1 + kick.1);
    s.n = Some(n.wrapping_add(1));
    s.tick = Some(p.tick);
    s.ax = Some(acc.0 as f64);
    s.ay = Some(acc.1 as f64);
    let share = r.share as f32;
    let keep = (1.0 - r.share) as f32;
    let follow = r.follow as f32;
    Kick {
        view: (kick.0 * share, kick.1 * share),
        offset: (
            acc.0 * keep + residual.0 * share * follow,
            acc.1 * keep + residual.1 * share * follow,
        ),
    }
}

/// Number of values in a [`recoil_preview`] row.
pub const PREVIEW_LEN: usize = 18;

/// The degrees (pitch, yaw) the next bullet of a weapon will be turned by recoil at `tick`, worked as
/// `GSWeaponRecoilRules.apply` would without firing it (no posture, nothing written), times a share: FC's
/// `FcBots.next_recoil_offset(...) * recoil_share(mind)`, in the script's precision (`Vector2` halves are `f32`).
/// The row, raw and in this order: `rules.reset, b.recoil_recovery, rules.recovery, b.recoil_build, rules.build,
/// b.recoil_build_max, b.recoil_pitch, rules.vertical, b.recoil_yaw, rules.horizontal, share (b.recoil_camera when 0
/// or more, else rules.punch), rules.follow, seed, last tick (state recoil_tick, -1000000 when absent), n, ax, ay,
/// the share of the offset taken`. `None` when the row is short.
pub fn recoil_preview(r: &[f64], tick: i64, tick_rate: f64) -> Option<(f32, f32)> {
    if r.len() < PREVIEW_LEN {
        return None;
    }
    let elapsed = maxf(
        tick.wrapping_sub(r[13] as i64) as f64 / maxf(tick_rate, 1.0),
        0.0,
    );
    let n = if elapsed <= r[0] { r[14] as i64 } else { 0 };
    let decay = (-r[1] * r[2] * elapsed).exp() as f32;
    let residual = (r[15] as f32 * decay, r[16] as f32 * decay);
    let grow = 1.0 + minf(r[3] * r[4] * n as f64, r[5] * r[4]);
    let kick = (
        (r[6] * r[7] * grow) as f32,
        (r[8] * r[9] * grow * drift(r[12] as i64, n)) as f32,
    );
    let keep = (1.0 - r[10]) as f32;
    let (share, follow) = (r[10] as f32, r[11] as f32);
    let taken = r[17] as f32;
    let x = (residual.0 + kick.0) * keep + residual.0 * share * follow;
    let y = (residual.1 + kick.1) * keep + residual.1 * share * follow;
    Some((x * taken, y * taken))
}
