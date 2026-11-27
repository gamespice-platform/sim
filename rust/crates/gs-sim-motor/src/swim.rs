//! gs-player-controller's `GSFpsSwimMode._simulate`: a swimmer's whole tick (surface, room to
//! float, wish, water friction, the climb out at the surface, then the step-less slide), spelled
//! operation for operation so that a native tick and the script agree to the bit.
//!
//! The script stays the specification and the fallback: the motor sends a swim tun tail
//! ([`TAIL_LEN`] values then [`BOX_LEN`] per water box) only for a mode that is exactly `GSFpsSwimMode`.

use crate::crouch::BUTTON_CROUCH;
use crate::slide::{Mover, capsule_centre};
use crate::tick::{AIR, State};
use crate::{DOWN, Space, UP, Vec3};
use gs_sim_math::{clampf, lerp_f32, maxf};

/// `GSFpsCommand.BUTTON_JUMP`.
const BUTTON_JUMP: i64 = 1;
/// The tail's first value for a swim mode (a later data-driven mode takes another).
pub const KIND_SWIM: f64 = 1.0;
/// Values before the boxes: kind, mode id, the eight tunables, pitch, then the box count.
pub const TAIL_LEN: usize = 12;
/// Values per water box: position xyz then size xyz (an `AABB`, so `f32` each).
pub const BOX_LEN: usize = 6;

/// The swim mode's tunables, its mode id, the tick's pitch and its water boxes, as the tail sends them.
#[derive(Clone, Copy, Debug)]
pub struct SwimTun<'a> {
    pub mode_id: i64,
    pub swim_speed: f64,
    pub accelerate: f64,
    pub waist: f64,
    pub float_depth: f64,
    pub exit_speed: f64,
    pub exit_reach: f64,
    pub idle_sink_speed: f64,
    pub friction: f64,
    /// `state.pitch` after the head's clamp; the tick row does not hold it.
    pub pitch: f64,
    boxes: &'a [f64],
}

impl<'a> SwimTun<'a> {
    /// The swim tail at the start of `p`, or none where `p` holds no whole one.
    pub fn read(p: &'a [f64]) -> Option<Self> {
        if p.len() < TAIL_LEN || p[0] != KIND_SWIM {
            return None;
        }
        let n = p[11];
        if !(0.0..=1.0e6).contains(&n) || n.fract() != 0.0 {
            return None;
        }
        let end = TAIL_LEN + n as usize * BOX_LEN;
        if p.len() < end {
            return None;
        }
        Some(Self {
            mode_id: p[1] as i64,
            swim_speed: p[2],
            accelerate: p[3],
            waist: p[4],
            float_depth: p[5],
            exit_speed: p[6],
            exit_reach: p[7],
            idle_sink_speed: p[8],
            friction: p[9],
            pitch: p[10],
            boxes: &p[TAIL_LEN..end],
        })
    }

    /// `surface_at`: the top (`box.end.y`) of the first box holding `point` (`AABB.has_point`,
    /// faces included), or NaN.
    pub fn surface_at(&self, point: Vec3) -> f64 {
        for b in self.boxes.as_chunks::<BOX_LEN>().0 {
            let pos = Vec3::new(b[0] as f32, b[1] as f32, b[2] as f32);
            let end = pos + Vec3::new(b[3] as f32, b[4] as f32, b[5] as f32);
            let outside = point.x < pos.x
                || point.y < pos.y
                || point.z < pos.z
                || point.x > end.x
                || point.y > end.y
                || point.z > end.z;
            if !outside {
                return end.y as f64;
            }
        }
        f64::NAN
    }

    /// `surface_at(position + Vector3.UP * waist)`.
    fn surface_above(&self, position: Vec3) -> f64 {
        self.surface_at(position + UP.scale(self.waist))
    }
}

/// `has_room_to_float`: water over the waist, and the drop to the floating height clear.
pub fn has_room_to_float<S: Space>(mover: &mut Mover<'_, S>, s: &State, t: &SwimTun) -> bool {
    let surface = t.surface_above(s.position);
    if surface.is_nan() {
        return false;
    }
    let drop = s.position.y as f64 - (surface - t.float_depth);
    if drop <= 0.0 {
        return true;
    }
    // `motor.probe`: the skin-margined sweep from the capsule's centre.
    !mover
        .sweep(
            capsule_centre(s.position, s.height),
            DOWN.scale(drop),
            s.height,
        )
        .hit
}

/// One swimmer's tick after the motor's head: `GSFpsSwimMode._simulate`, its `set_mode` to air
/// included (swim has no exit hook). True where the slide ran out (`stuck_ticks += 1`).
#[allow(clippy::too_many_arguments)]
pub fn tick<S: Space>(
    mover: &mut Mover<'_, S>,
    s: &mut State,
    t: &SwimTun,
    yaw: f64,
    buttons: i64,
    move_x: f64,
    move_y: f64,
    delta: f64,
) -> bool {
    let surface = t.surface_above(s.position);
    if surface.is_nan() || !has_room_to_float(mover, s, t) {
        s.mode = AIR;
        return move_and_slide(mover, s, delta);
    }
    let (forward, right) = view_basis(yaw, t.pitch);
    let mut wish =
        limit_length(right.scale(move_x) + forward.scale(move_y), 1.0).scale(t.swim_speed);
    let mut vertical = 0.0f64;
    if buttons & BUTTON_JUMP != 0 {
        vertical += 1.0;
    }
    if buttons & BUTTON_CROUCH != 0 {
        vertical -= 1.0;
    }
    // `command.move == Vector2.ZERO`, on the command's `f32`s.
    let idle = move_x as f32 == 0.0 && move_y as f32 == 0.0;
    if vertical != 0.0 {
        wish.y = (wish.y as f64 + vertical * t.swim_speed) as f32;
    } else if idle && t.idle_sink_speed > 0.0 {
        wish.y = (-t.idle_sink_speed) as f32;
    } else if idle {
        let floating = surface - t.float_depth;
        let half = t.swim_speed * 0.5;
        wish.y = clampf((floating - s.position.y as f64) * 2.0, -half, half) as f32;
    }
    if t.friction > 0.0 {
        s.velocity = s.velocity.scale(maxf(0.0, 1.0 - t.friction * delta));
    }
    let w = clampf(t.accelerate * delta, 0.0, 1.0) as f32;
    let v = s.velocity;
    s.velocity = Vec3::new(
        lerp_f32(v.x, wish.x, w),
        lerp_f32(v.y, wish.y, w),
        lerp_f32(v.z, wish.z, w),
    );
    if vertical > 0.0 && surface - (s.position.y as f64 + t.waist) < t.exit_reach {
        s.velocity.y = maxf(s.velocity.y as f64, t.exit_speed) as f32;
    }
    move_and_slide(mover, s, delta)
}

/// `Vector3.limit_length`: divided by its length, then scaled, where longer than `max`.
pub(crate) fn limit_length(v: Vec3, max: f32) -> Vec3 {
    let l = v.length();
    if l > 0.0 && max < l {
        let d = Vec3::new(v.x / l, v.y / l, v.z / l);
        return Vec3::new(d.x * max, d.y * max, d.z * max);
    }
    v
}

/// `GSFpsMotor._view_basis`'s forward and right: angles in `f64`, each component rounded to `f32`.
pub(crate) fn view_basis(yaw_degrees: f64, pitch_degrees: f64) -> (Vec3, Vec3) {
    let yaw = yaw_degrees * (std::f64::consts::PI / 180.0);
    let pitch = pitch_degrees * (std::f64::consts::PI / 180.0);
    let forward = Vec3::new(
        (-yaw.sin() * pitch.cos()) as f32,
        pitch.sin() as f32,
        (-yaw.cos() * pitch.cos()) as f32,
    );
    let right = Vec3::new(yaw.cos() as f32, 0.0, (-yaw.sin()) as f32);
    (forward, right)
}

/// `GSFpsMotor.move_and_slide(state, delta)` without the step: true where the slide ran out.
pub(crate) fn move_and_slide<S: Space>(
    mover: &mut Mover<'_, S>,
    s: &mut State,
    delta: f64,
) -> bool {
    let motion = s.velocity.scale(delta);
    if motion.length_squared() <= 0.0 {
        return false;
    }
    let r = mover.move_and_slide(s.position, s.velocity, motion, s.height, false);
    s.position = r.position;
    s.velocity = r.velocity;
    r.ran_out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail(boxes: &[[f64; 6]]) -> Vec<f64> {
        let mut p = vec![
            KIND_SWIM,
            3.0,
            3.81,
            10.0,
            1.0,
            1.35,
            5.0,
            0.5,
            0.0,
            1.0,
            0.0,
            boxes.len() as f64,
        ];
        for b in boxes {
            p.extend_from_slice(b);
        }
        p
    }

    #[test]
    fn reads_only_a_whole_swim_tail() {
        let p = tail(&[[0.0, -5.0, 0.0, 10.0, 5.0, 10.0]]);
        assert!(SwimTun::read(&p).is_some());
        assert!(SwimTun::read(&p[..p.len() - 1]).is_none());
        let mut other = p.clone();
        other[0] = 2.0;
        assert!(SwimTun::read(&other).is_none());
        let mut bad = p.clone();
        bad[11] = -1.0;
        assert!(SwimTun::read(&bad).is_none());
        bad[11] = 0.5;
        assert!(SwimTun::read(&bad).is_none());
        assert!(SwimTun::read(&[]).is_none());
    }

    #[test]
    fn surface_is_the_first_box_holding_the_point_faces_included() {
        let p = tail(&[
            [0.0, -5.0, 0.0, 10.0, 5.0, 10.0],
            [0.0, -5.0, 0.0, 10.0, 8.0, 10.0],
        ]);
        let t = SwimTun::read(&p).unwrap();
        assert_eq!(t.surface_at(Vec3::new(5.0, 0.0, 5.0)), 0.0);
        assert_eq!(t.surface_at(Vec3::new(10.0, -5.0, 0.0)), 0.0);
        assert_eq!(t.surface_at(Vec3::new(5.0, 2.0, 5.0)), 3.0);
        assert!(t.surface_at(Vec3::new(5.0, 4.0, 5.0)).is_nan());
        assert!(t.surface_at(Vec3::new(-0.001, 0.0, 5.0)).is_nan());
    }

    #[test]
    fn view_basis_matches_the_walker_at_level_pitch() {
        for yaw in [0.0, 37.5, -120.25, 359.0] {
            let (f, r) = view_basis(yaw, 0.0);
            let y = yaw * (std::f64::consts::PI / 180.0);
            assert_eq!(f.x, (-y.sin()) as f32);
            assert_eq!(f.z, (-y.cos()) as f32);
            assert_eq!(f.y, 0.0);
            assert_eq!(r, Vec3::new(y.cos() as f32, 0.0, (-y.sin()) as f32));
        }
    }

    #[test]
    fn limit_length_leaves_short_vectors_and_divides_long_ones() {
        let short = Vec3::new(0.3, 0.0, 0.4);
        assert_eq!(limit_length(short, 1.0), short);
        let long = Vec3::new(3.0, 0.0, 4.0);
        assert_eq!(
            limit_length(long, 1.0),
            Vec3::new(3.0 / 5.0, 0.0, 4.0 / 5.0)
        );
        assert_eq!(limit_length(Vec3::default(), 1.0), Vec3::default());
    }
}
