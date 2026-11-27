//! gs-player-controller's `GSFpsLadderMode._simulate`: a climber's whole tick (let go off the
//! box, jump off, or the view-split climb, then the step-less slide and the ground check at the
//! foot), spelled operation for operation so that a native tick and the script agree to the bit.
//!
//! The script stays the specification and the fallback: the motor sends a ladder tun tail
//! ([`TAIL_LEN`] values then [`BOX_LEN`] per ladder box) only for a mode that sends one
//! (`GSFpsLadderMode`, and a game's copy that keeps its tick, with [`LadderTun::carry_over_top`]).
//! The jump-off tick a ladder remembers is per-instance script state: the script's tail writes it
//! for the tick it sends (jump held and a box touched at the start position), as `_simulate` would.

use crate::slide::Mover;
use crate::swim::{limit_length, move_and_slide, view_basis};
use crate::tick::{self, AIR, GROUND, State, TickCounters, TickTun};
use crate::{Space, UP, Vec3};
use gs_sim_math::clampf;

/// `GSFpsCommand.BUTTON_JUMP`.
const BUTTON_JUMP: i64 = 1;
/// The tail's first value for a ladder mode ([`crate::swim::KIND_SWIM`] is 1).
pub const KIND_LADDER: f64 = 2.0;
/// Values before the boxes: kind, mode id, four tunables, the carry flag, radius, height, pitch,
/// then the box count.
pub const TAIL_LEN: usize = 11;
/// Values per ladder box: position xyz then size xyz (an `AABB`, so `f32` each).
pub const BOX_LEN: usize = 6;

/// The ladder mode's tunables, its mode id, the hull, the tick's pitch and its boxes, as the tail sends them.
#[derive(Clone, Copy, Debug)]
pub struct LadderTun<'a> {
    pub mode_id: i64,
    pub climb_speed: f64,
    pub jump_off_speed: f64,
    pub reach: f64,
    pub dampen: f64,
    /// Off the top while rising and pushing forward, carry on over the lip at the climb speed
    /// (Fatal Contact's ladder); the library's ladder lets go with the velocity it has.
    pub carry_over_top: bool,
    /// `tunables.radius`.
    pub radius: f64,
    /// `tunables.height_at(state.crouch_fraction)`.
    pub height: f64,
    /// `state.pitch` after the head's clamp; the tick row does not hold it.
    pub pitch: f64,
    boxes: &'a [f64],
}

impl<'a> LadderTun<'a> {
    /// The ladder tail at the start of `p`, or none where `p` holds no whole one.
    pub fn read(p: &'a [f64]) -> Option<Self> {
        if p.len() < TAIL_LEN || p[0] != KIND_LADDER {
            return None;
        }
        let n = p[10];
        if !(0.0..=1.0e6).contains(&n) || n.fract() != 0.0 {
            return None;
        }
        let end = TAIL_LEN + n as usize * BOX_LEN;
        if p.len() < end {
            return None;
        }
        Some(Self {
            mode_id: p[1] as i64,
            climb_speed: p[2],
            jump_off_speed: p[3],
            reach: p[4],
            dampen: p[5],
            carry_over_top: p[6] != 0.0,
            radius: p[7],
            height: p[8],
            pitch: p[9],
            boxes: &p[TAIL_LEN..end],
        })
    }

    /// `touching`: the first box (position, size) whose `AABB.intersects` the reach-widened hull
    /// at `feet`, or none (the script's `AABB()`).
    pub fn touching(&self, feet: Vec3) -> Option<(Vec3, Vec3)> {
        let e = self.radius + self.reach;
        let hp = feet - Vec3::new(e as f32, 0.0, e as f32);
        let hs = Vec3::new((e * 2.0) as f32, self.height as f32, (e * 2.0) as f32);
        for b in self.boxes.as_chunks::<BOX_LEN>().0 {
            let pos = Vec3::new(b[0] as f32, b[1] as f32, b[2] as f32);
            let size = Vec3::new(b[3] as f32, b[4] as f32, b[5] as f32);
            if intersects(pos, size, hp, hs) {
                return Some((pos, size));
            }
        }
        None
    }
}

/// `AABB.intersects`, strict on every face, `self` the box and `o` the hull.
fn intersects(pos: Vec3, size: Vec3, op: Vec3, os: Vec3) -> bool {
    !(pos.x >= op.x + os.x
        || pos.x + size.x <= op.x
        || pos.y >= op.y + os.y
        || pos.y + size.y <= op.y
        || pos.z >= op.z + os.z
        || pos.z + size.z <= op.z)
}

/// `_away`: the horizontal unit vector from the box's face out toward `feet`, on its thin axis.
pub fn away(pos: Vec3, size: Vec3, feet: Vec3) -> Vec3 {
    let c = pos + Vec3::new(size.x * 0.5, size.y * 0.5, size.z * 0.5);
    let dx = feet.x - c.x;
    let dz = feet.z - c.z;
    let sign = |d: f32| if d != 0.0 { d.signum() } else { 1.0 };
    if size.x < size.z {
        Vec3::new(sign(dx), 0.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, sign(dz))
    }
}

/// One climber's tick after the motor's head: `GSFpsLadderMode._simulate`, its `set_mode` to air
/// included (the base mode's exit hook does nothing) and its ground check at the foot. True where
/// the slide ran out (`stuck_ticks += 1`).
#[allow(clippy::too_many_arguments)]
pub fn tick<S: Space>(
    mover: &mut Mover<'_, S>,
    s: &mut State,
    t: &LadderTun,
    tt: &TickTun,
    c: &mut TickCounters,
    yaw: f64,
    buttons: i64,
    move_x: f64,
    move_y: f64,
    delta: f64,
) -> bool {
    let touched = t
        .touching(s.position)
        .filter(|(_, size)| *size != Vec3::default());
    let Some((pos, size)) = touched else {
        // `command.move.y > 0.0`, on the command's `f32`.
        if t.carry_over_top && s.velocity.y > 0.0 && move_y as f32 > 0.0 {
            let (fwd, _) = view_basis(yaw, 0.0);
            let flat = Vec3::new(fwd.x, 0.0, fwd.z)
                .normalized()
                .scale(t.climb_speed);
            s.velocity = flat + UP.scale(s.velocity.y as f64);
        }
        s.mode = AIR;
        return move_and_slide(mover, s, delta);
    };
    let away = away(pos, size, s.position);
    if buttons & BUTTON_JUMP != 0 {
        s.velocity = away.scale(t.jump_off_speed);
        s.mode = AIR;
        return move_and_slide(mover, s, delta);
    }
    let (forward, right) = view_basis(yaw, t.pitch);
    let wish = (forward.scale(clampf(move_y, -1.0, 1.0)) + right.scale(clampf(move_x, -1.0, 1.0)))
        .scale(t.climb_speed);
    let into = wish.dot(away) as f64;
    let lateral = wish - away.scale(into);
    let perp = UP.cross(away).normalized();
    let up_face = away.cross(perp);
    let v = lateral.scale(1.0 - t.dampen) - up_face.scale(into);
    s.velocity = limit_length(v, (t.climb_speed * 1.5) as f32);
    let ran_out = move_and_slide(mover, s, delta);
    if v.y <= 0.0 {
        tick::categorise(mover, s, tt, c);
        if s.mode != GROUND {
            s.mode = t.mode_id;
        }
    }
    ran_out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail(carry: bool, boxes: &[[f64; 6]]) -> Vec<f64> {
        let mut p = vec![
            KIND_LADDER,
            4.0,
            3.81,
            5.14,
            0.05,
            0.0,
            if carry { 1.0 } else { 0.0 },
            0.4,
            1.8,
            0.0,
            boxes.len() as f64,
        ];
        for b in boxes {
            p.extend_from_slice(b);
        }
        p
    }

    #[test]
    fn reads_only_a_whole_ladder_tail() {
        let p = tail(false, &[[0.0, 0.0, 0.0, 0.1, 4.0, 1.0]]);
        let t = LadderTun::read(&p).unwrap();
        assert!(!t.carry_over_top);
        assert!(LadderTun::read(&tail(true, &[])).unwrap().carry_over_top);
        assert!(LadderTun::read(&p[..p.len() - 1]).is_none());
        let mut other = p.clone();
        other[0] = 1.0;
        assert!(LadderTun::read(&other).is_none());
        let mut bad = p.clone();
        bad[10] = -1.0;
        assert!(LadderTun::read(&bad).is_none());
        bad[10] = 0.5;
        assert!(LadderTun::read(&bad).is_none());
        assert!(LadderTun::read(&[]).is_none());
    }

    #[test]
    fn touching_is_strict_on_the_faces_and_widened_by_the_reach() {
        let p = tail(false, &[[1.0, 0.0, -0.5, 0.1, 4.0, 1.0]]);
        let t = LadderTun::read(&p).unwrap();
        // The hull's +x face is at feet.x + 0.45.
        assert!(t.touching(Vec3::new(0.6, 0.0, 0.0)).is_some());
        assert!(t.touching(Vec3::new(0.5, 0.0, 0.0)).is_none());
        // Feet at the box's top face: the hull starts where the box ends.
        assert!(t.touching(Vec3::new(1.0, 4.0, 0.0)).is_none());
        assert!(t.touching(Vec3::new(1.0, 3.9, 0.0)).is_some());
    }

    #[test]
    fn away_is_the_thin_axis_toward_the_feet() {
        let pos = Vec3::new(1.0, 0.0, -0.5);
        let thin_x = Vec3::new(0.1, 4.0, 1.0);
        assert_eq!(
            away(pos, thin_x, Vec3::new(0.5, 0.0, 3.0)),
            Vec3::new(-1.0, 0.0, 0.0)
        );
        assert_eq!(
            away(pos, thin_x, Vec3::new(1.05, 0.0, 0.0)),
            Vec3::new(1.0, 0.0, 0.0)
        );
        let thin_z = Vec3::new(1.0, 4.0, 0.1);
        assert_eq!(
            away(pos, thin_z, Vec3::new(0.0, 0.0, 0.0)),
            Vec3::new(0.0, 0.0, 1.0)
        );
        assert_eq!(
            away(pos, thin_z, Vec3::new(0.0, 0.0, -1.0)),
            Vec3::new(0.0, 0.0, -1.0)
        );
    }
}
