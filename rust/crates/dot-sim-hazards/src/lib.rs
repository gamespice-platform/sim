//! Wipeout's hazard knock: mg-wipeout's `WoCourse.knock` and `path_clear` over a table of the
//! course's spinners, pendulums, pushers and falls, to the bit.
//!
//! A hazard's root never moves once the course is built (only its arms, ball or ram are posed), so
//! the table is written once: the root transform (a fall's `affine_inverse()` from the engine with
//! it), the cull sphere the script works out, and the spec's numbers as the script reads them.
//! Every ask is then the script's arithmetic: script floats in `f64`, vectors in `f32`, the
//! engine's `Basis(axis, angle)`, `Transform3D * Transform3D` and `affine_inverse()` in its order.

use dot_sim_math::{Vec3, Xform, clampf, maxf, minf};

#[cfg(test)]
mod tests;

/// The player capsule's radius, as the script's `PLAYER_RADIUS`.
pub const PLAYER_RADIUS: f64 = 0.35;
/// Standing height, as `PLAYER_HEIGHT`.
pub const PLAYER_HEIGHT: f64 = 1.8;
/// Crouched height, as `PLAYER_CROUCHED`.
pub const PLAYER_CROUCHED: f64 = 0.9;

const TAU: f64 = std::f64::consts::TAU;
const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const BACK: Vec3 = Vec3::new(0.0, 0.0, 1.0);
const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);

/// What a hazard is, with its spec's numbers (`float(spec[...])`, defaults already applied).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Arms turning about the root's up: `arm_height`, `arm_length`, `hub_radius`, `arm_thickness`,
    /// `arms` (at least one, as `maxi(int(...), 1)`), `phase` and `speed` in degrees.
    Spinner {
        arm_height: f64,
        arm_length: f64,
        hub_radius: f64,
        thickness: f64,
        arms: i64,
        phase: f64,
        speed: f64,
    },
    /// A ball on a rod: the frame is `Transform3D(_yaw_basis(yaw), pivot)`.
    Pendulum {
        length: f64,
        radius: f64,
        swing: f64,
        period: f64,
        phase: f64,
    },
    /// A ram: `yaw` is `_yaw_basis(yaw)` with `at` as its origin, `size` the ram's box.
    Pusher {
        yaw: Xform,
        size: Vec3,
        reach: f64,
        period: f64,
        phase: f64,
        duty: f64,
    },
    /// A falling curtain: `inverse` is the root's `affine_inverse()` from the engine.
    Fall {
        inverse: Xform,
        size: Vec3,
        push: f64,
        period: f64,
        phase: f64,
        duty: f64,
    },
}

/// One hazard piece, in the course's order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hazard {
    /// The root's transform (a pendulum's yaw frame at its pivot).
    pub frame: Xform,
    /// `_hazard_centre(piece)`.
    pub centre: Vec3,
    /// `_hazard_reach(piece)`.
    pub reach: f64,
    pub kind: Kind,
}

/// A course's hazards and the asks over them.
#[derive(Clone, Debug, Default)]
pub struct Hazards {
    pub hazards: Vec<Hazard>,
    /// The stage's `tick_rate`.
    pub tick_rate: i64,
    /// `cull_hazards`: skip a hazard whose sphere the feet are outside.
    pub cull: bool,
}

impl Hazards {
    pub fn new(tick_rate: i64) -> Self {
        Self {
            hazards: Vec::new(),
            tick_rate,
            cull: true,
        }
    }

    /// `knock(feet, crouched, tick, limits)`: the velocity the first hazard touching the capsule
    /// throws it with, `limits` being `[minimum speed, maximum speed, lift]`; zero for none.
    pub fn knock(&self, feet: Vec3, crouched: bool, tick: i64, limits: Vec3) -> Vec3 {
        let t = tick as f64 / self.tick_rate.max(1) as f64;
        let height = if crouched {
            PLAYER_CROUCHED
        } else {
            PLAYER_HEIGHT
        };
        let top = feet + Vec3::new(0.0, (height - PLAYER_RADIUS) as f32, 0.0);
        let bottom = feet + Vec3::new(0.0, PLAYER_RADIUS as f32, 0.0);

        for hazard in &self.hazards {
            if self.cull
                && feet.distance_to(hazard.centre) as f64 > hazard.reach + PLAYER_HEIGHT + 1.0
            {
                continue;
            }

            let thrown = match &hazard.kind {
                Kind::Spinner { .. } => spinner(hazard, bottom, top, t),
                Kind::Pendulum { .. } => pendulum(hazard, bottom, top, t),
                Kind::Pusher { .. } => pusher(hazard, bottom, top, t),
                Kind::Fall { .. } => fall(hazard, bottom, top, t),
            };

            if thrown != ZERO {
                let flat = Vec3::new(thrown.x, 0.0, thrown.z);
                let speed = clampf(flat.length() as f64, limits.x as f64, limits.y as f64);
                let flat = if flat.length() as f64 > 0.001 {
                    flat.normalized().scale(speed)
                } else {
                    ZERO
                };
                return Vec3::new(
                    flat.x,
                    maxf(thrown.y as f64, limits.z as f64) as f32,
                    flat.z,
                );
            }
        }

        ZERO
    }

    /// `path_clear(from, to, tick, speed, margin_seconds)`: whether a run from `from` to `to`
    /// meets no hazard, asked every sixteenth of a second at the tick the runner is there.
    pub fn path_clear(
        &self,
        from: Vec3,
        to: Vec3,
        tick: i64,
        speed: f64,
        margin_seconds: f64,
    ) -> bool {
        let speed = speed * 0.8;
        let (dx, dz) = (to.x - from.x, to.z - from.z);
        let distance = (dx * dx + dz * dz).sqrt() as f64;
        let seconds = distance / maxf(speed, 0.5) + margin_seconds;
        let steps = ((seconds * 16.0) as i64).max(1);
        let ticks_per_step = self.tick_rate as f64 / 16.0;
        let limits = Vec3::new(1.0, 1.0, 1.0);

        for step in 0..=steps {
            let along = clampf(step as f64 / 16.0 * speed / maxf(distance, 0.01), 0.0, 1.0);
            let at = lerp(from, to, along);

            if self.knock(
                at,
                false,
                tick + (step as f64 * ticks_per_step) as i64,
                limits,
            ) != ZERO
            {
                return false;
            }
        }

        true
    }
}

fn spinner(hazard: &Hazard, bottom: Vec3, top: Vec3, t: f64) -> Vec3 {
    let Kind::Spinner {
        arm_height,
        arm_length,
        hub_radius,
        thickness,
        arms,
        phase,
        speed,
    } = hazard.kind
    else {
        return ZERO;
    };
    let count = arms.max(1);
    let angle = deg_to_rad(phase + speed * t);
    let omega = deg_to_rad(speed);
    let frame = &hazard.frame;
    let raised = Vec3::new(0.0, arm_height as f32, 0.0);
    let hub = frame.xform(raised);

    for arm in 0..count {
        let heading = Xform::from_axis_angle(UP, (angle + TAU * arm as f64 / count as f64) as f32);
        let tip = frame.xform(raised + heading.basis_xform(Vec3::new(arm_length as f32, 0.0, 0.0)));
        let start = hub + (tip - hub).normalized().scale(hub_radius);
        let (on_arm, _, distance) = segments(start, tip, bottom, top);

        if distance as f64 > thickness * 0.5 + PLAYER_RADIUS {
            continue;
        }

        let mut radial = on_arm - hub;
        radial.y = 0.0;
        let tangent = UP.cross(radial).normalized().scale(signf(omega));
        let push = tangent
            .scale(omega.abs())
            .scale(radial.length() as f64)
            .scale(1.25);
        return push + radial.normalized().scale(2.0);
    }

    ZERO
}

fn pendulum(hazard: &Hazard, bottom: Vec3, top: Vec3, t: f64) -> Vec3 {
    let Kind::Pendulum {
        length,
        radius,
        swing,
        period,
        phase,
    } = hazard.kind
    else {
        return ZERO;
    };
    let frame = &hazard.frame;
    let rod = Vec3::new(0.0, -length as f32, 0.0);
    let ball = |at: f64| {
        let angle = deg_to_rad(swing) * (TAU * (at / period + phase)).sin();
        frame.xform(Xform::from_axis_angle(BACK, angle as f32).basis_xform(rod))
    };
    let centre = ball(t);

    if point_segment(centre, bottom, top) as f64 > radius + PLAYER_RADIUS {
        return ZERO;
    }

    let dt = 0.001;
    let mut velocity = (ball(t + dt) - centre).div_scalar(dt);
    velocity.y = 0.0;

    if (velocity.length() as f64) < 0.5 {
        let mut away = (bottom + top).scale(0.5) - centre;
        away.y = 0.0;
        return away.normalized().scale(3.0);
    }

    velocity.scale(1.3)
}

fn pusher(hazard: &Hazard, bottom: Vec3, top: Vec3, t: f64) -> Vec3 {
    let Kind::Pusher {
        yaw,
        size,
        reach,
        period,
        phase,
        duty,
    } = hazard.kind
    else {
        return ZERO;
    };
    let out = pusher_extension(period, phase, duty, t) * reach;
    let ram = Xform {
        rows: yaw.rows,
        origin: yaw.origin + yaw.basis_xform(Vec3::new(0.0, 0.0, -out as f32)),
    };
    let frame = hazard.frame.mul(&ram);
    let half = size.scale(0.5);
    let inverse = frame.affine_inverse();
    let hit = (0..5).any(|step| {
        let local = inverse.xform(lerp(bottom, top, step as f64 / 4.0));
        let clamped = Vec3::new(
            clampf(local.x as f64, -half.x as f64, half.x as f64) as f32,
            clampf(local.y as f64, -half.y as f64, half.y as f64) as f32,
            clampf(local.z as f64, -half.z as f64, half.z as f64) as f32,
        );
        local.distance_to(clamped) as f64 <= PLAYER_RADIUS
    });

    if !hit {
        return ZERO;
    }

    let forward = frame.basis_xform(Vec3::new(0.0, 0.0, -1.0));
    let dt = 0.02;
    let moving = (pusher_extension(period, phase, duty, t + dt)
        - pusher_extension(period, phase, duty, t))
        / dt
        * reach;

    if moving > 0.5 {
        return forward.scale(maxf(moving * 1.2, 14.0));
    }

    forward.scale(4.0)
}

fn fall(hazard: &Hazard, bottom: Vec3, top: Vec3, t: f64) -> Vec3 {
    let Kind::Fall {
        inverse,
        size,
        push,
        period,
        phase,
        duty,
    } = hazard.kind
    else {
        return ZERO;
    };

    if !fall_on(period, phase, duty, t) {
        return ZERO;
    }

    let half = size.scale(0.5);
    let height = size.y as f64;

    for step in 0..3 {
        let local = inverse.xform(lerp(bottom, top, step as f64 / 2.0));

        if (local.x as f64).abs() <= half.x as f64 + PLAYER_RADIUS * 0.5
            && (local.z as f64).abs() <= half.z as f64 + PLAYER_RADIUS
            && local.y as f64 > -1.0
            && (local.y as f64) < height
        {
            return hazard.frame.basis_xform(Vec3::new(push as f32, -1.0, 0.0));
        }
    }

    ZERO
}

/// `pusher_extension`: how far out a ram is, 0 at rest to 1 at full reach.
pub fn pusher_extension(period: f64, phase: f64, duty: f64, t: f64) -> f64 {
    let u = fposmod(t / period + phase, 1.0);
    let duty = clampf(duty, 0.1, 0.8);
    let out_time = 0.08;

    if u < out_time {
        return smoothstep01(u / out_time);
    }

    if u < duty {
        return 1.0;
    }

    let back = minf(0.2, 1.0 - duty);

    if u < duty + back {
        return 1.0 - smoothstep01((u - duty) / back);
    }

    0.0
}

/// `fall_on`: whether a curtain is coming down at `t`.
pub fn fall_on(period: f64, phase: f64, duty: f64, t: f64) -> bool {
    fposmod(t / period + phase, 1.0) < clampf(duty, 0.05, 0.95)
}

/// `_segments`: the closest points of two segments and their distance, as the script spells it
/// (dot products in `f32`, the solve in `f64`).
pub fn segments(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3, f32) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.dot(d1) as f64;
    let e = d2.dot(d2) as f64;
    let f = d2.dot(r) as f64;
    let mut s = 0.0;
    let mut u = 0.0;

    if a <= 0.000001 && e <= 0.000001 {
        return (p1, p2, p1.distance_to(p2));
    }

    if a <= 0.000001 {
        u = clampf(f / e, 0.0, 1.0);
    } else {
        let c = d1.dot(r) as f64;

        if e <= 0.000001 {
            s = clampf(-c / a, 0.0, 1.0);
        } else {
            let b = d1.dot(d2) as f64;
            let denom = a * e - b * b;
            s = if denom > 0.000001 {
                clampf((b * f - c * e) / denom, 0.0, 1.0)
            } else {
                0.0
            };
            u = (b * s + f) / e;

            if u < 0.0 {
                u = 0.0;
                s = clampf(-c / a, 0.0, 1.0);
            } else if u > 1.0 {
                u = 1.0;
                s = clampf((b - c) / a, 0.0, 1.0);
            }
        }
    }

    let c1 = p1 + d1.scale(s);
    let c2 = p2 + d2.scale(u);
    (c1, c2, c1.distance_to(c2))
}

/// `_point_segment`: the distance from `point` to the segment `a`-`b`.
pub fn point_segment(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let length = ab.length_squared() as f64;

    if length <= 0.000001 {
        return point.distance_to(a);
    }

    let s = clampf((point - a).dot(ab) as f64 / length, 0.0, 1.0);
    point.distance_to(a + ab.scale(s))
}

/// `Vector3.lerp`: the weight rounded to `f32`, `from + (to - from) * weight` per component.
#[inline]
fn lerp(from: Vec3, to: Vec3, weight: f64) -> Vec3 {
    let w = weight as f32;
    Vec3::new(
        from.x + (to.x - from.x) * w,
        from.y + (to.y - from.y) * w,
        from.z + (to.z - from.z) * w,
    )
}

#[inline]
fn deg_to_rad(d: f64) -> f64 {
    d * (std::f64::consts::PI / 180.0)
}

#[inline]
fn signf(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// The engine's `Math::fposmod`.
#[inline]
fn fposmod(x: f64, y: f64) -> f64 {
    let mut value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value += y;
    }
    value + 0.0
}

/// `smoothstep(0.0, 1.0, s)`.
#[inline]
fn smoothstep01(s: f64) -> f64 {
    let s = clampf(s, 0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}
