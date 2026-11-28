//! The remote-body render stages of game-fatal-contact's `FcPlayer.present`, for a whole frame of bodies:
//! the held gun's retell gate and shot count (`r_held`), the name tag's screen-parallel transform (`r_tag`) and the
//! figure's motion numbers (`r_anim`). In the script's order and precision: f32 where the engine computes in
//! `real_t`, f64 where the script computes in floats.

use std::collections::HashMap;

/// `AwWeaponNet.uses_between`: shots from [param previous] to [param now] on a sequence that wraps at [param wrap].
pub fn uses_between(previous: i64, now: i64, wrap: i64) -> i64 {
    if wrap == 0 {
        return 0;
    }
    let mut d = (now - previous) % wrap;
    if d < 0 {
        d += wrap;
    }
    d
}

/// What one body's held gun was last told (`_held_told*` and `_seen_fire_seq` in script), with the script's
/// starting values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeldSlot {
    pub told: i64,
    pub state: i64,
    pub charge: f64,
    pub seen: i64,
}

impl Default for HeldSlot {
    fn default() -> Self {
        Self {
            told: 0,
            state: -1,
            charge: 0.0,
            seen: 0,
        }
    }
}

/// One body's held-gun inputs of a frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeldIn {
    /// A body this client only draws (the retell gate applies only to those).
    pub remote: bool,
    /// switching | reloading << 1 | aiming << 2 | shown << 3.
    pub state: i64,
    pub charge: f64,
    /// The gun is the one last told and the one equipped.
    pub same: bool,
    pub seq: i64,
}

/// One frame of `_present_held`'s gate for one body: whether the hooks are told again, and how many shots the
/// sequence moved by. Updates [param slot] the way the script updates its members.
pub fn held_step(slot: &mut HeldSlot, i: &HeldIn, retell: i64, wrap: i64) -> (bool, i64) {
    slot.told += 1;
    let keep = i.remote
        && slot.told < retell
        && i.state == slot.state
        && i.charge == slot.charge
        && i.same;
    if !keep {
        slot.told = 0;
        slot.state = i.state;
        slot.charge = i.charge;
    }
    let fired = uses_between(slot.seen, i.seq, wrap);
    slot.seen = i.seq;
    (!keep, fired)
}

/// Steps after which a body not stepped since is forgotten (freed bodies leave no slot behind).
pub const HELD_KEEP: u64 = 256;

/// Every body's held-gun slot, by instance id, kept between frames.
#[derive(Default)]
pub struct HeldGate {
    slots: HashMap<i64, (HeldSlot, u64)>,
    steps: u64,
    /// One code per body of the last [method step]: `fired << 1 | tell`.
    pub codes: Vec<i64>,
}

impl HeldGate {
    /// One frame over [param ids] and [param inputs] (the same length).
    pub fn step(&mut self, ids: &[i64], inputs: &[HeldIn], retell: i64, wrap: i64) {
        self.steps += 1;
        let now = self.steps;
        self.codes.clear();
        for (id, i) in ids.iter().zip(inputs) {
            let (slot, used) = self.slots.entry(*id).or_default();
            *used = now;
            let (tell, fired) = held_step(slot, i, retell, wrap);
            self.codes.push((fired << 1) | i64::from(tell));
        }
        if now.is_multiple_of(HELD_KEEP) {
            self.slots.retain(|_, (_, used)| now - *used < HELD_KEEP);
        }
    }

    /// The slot of body [param id], if it has one.
    pub fn slot(&self, id: i64) -> Option<&HeldSlot> {
        self.slots.get(&id).map(|(s, _)| s)
    }

    /// Starts body [param id] from [param slot] (what the script had told it), replacing any slot it had.
    pub fn seed(&mut self, id: i64, slot: HeldSlot) {
        self.slots.insert(id, (slot, self.steps));
    }

    /// Forgets body [param id] (it was freed or rebuilt its gun).
    pub fn forget(&mut self, id: i64) {
        self.slots.remove(&id);
    }

    /// Forgets every body.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.codes.clear();
    }
}

/// A basis as the engine stores it: three rows.
pub type Rows = [[f32; 3]; 3];

/// `basis.scaled_local(Vector3.ONE * scale)`, which scales each row by the vector (`Basis.scale_local`):
/// [param scale] is the script's float, rounded to `real_t` as the vector product does.
pub fn tag_basis(basis: &Rows, scale: f64) -> Rows {
    let s = 1.0f32 * scale as f32;
    let mut out = *basis;
    for row in out.iter_mut() {
        for cell in row.iter_mut() {
            *cell *= s;
        }
    }
    out
}

/// `Vector3.is_zero_approx` of the eye minus the tag point.
pub fn zero_approx(v: [f32; 3]) -> bool {
    const EPS: f32 = 0.00001;
    v.iter().all(|c| c.abs() < EPS)
}

/// The motion numbers `_animate_figure` computes from a body's drawn state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimIn {
    pub velocity: [f32; 3],
    pub position: [f32; 3],
    pub wall_normal: [f32; 3],
    /// The controller's radius (script float).
    pub radius: f64,
    /// The drawn pitch in degrees (script float).
    pub pitch: f64,
    pub previous_seq: i64,
    pub seq: i64,
    pub melee: bool,
}

/// What `_animate_figure` hands the figure, computed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimOut {
    /// `Vector2(v.x, v.z).length()`.
    pub speed: f32,
    /// `deg_to_rad(pitch)`.
    pub aim: f64,
    /// `Plane(n, p - n * radius)` or the empty plane when there is no wall: normal x, y, z and d.
    pub wall: [f32; 4],
    /// A melee swing starts this frame.
    pub swing: bool,
}

/// One body's `_animate_figure` numbers.
pub fn anim_one(i: &AnimIn, wrap: i64) -> AnimOut {
    let v = i.velocity;
    let speed = (v[0] * v[0] + v[2] * v[2]).sqrt();
    let aim = i.pitch * (std::f64::consts::PI / 180.0);
    let n = i.wall_normal;
    let wall = if n != [0.0, 0.0, 0.0] {
        let r = i.radius as f32;
        let p = [
            i.position[0] - n[0] * r,
            i.position[1] - n[1] * r,
            i.position[2] - n[2] * r,
        ];
        [n[0], n[1], n[2], n[0] * p[0] + n[1] * p[1] + n[2] * p[2]]
    } else {
        [0.0; 4]
    };
    let swing = uses_between(i.previous_seq, i.seq, wrap) > 0 && i.melee;
    AnimOut {
        speed,
        aim,
        wall,
        swing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_between_wraps() {
        assert_eq!(uses_between(5, 7, 256), 2);
        assert_eq!(uses_between(250, 3, 256), 9);
        assert_eq!(uses_between(7, 7, 256), 0);
        assert_eq!(uses_between(1, 2, 0), 0);
    }

    #[test]
    fn held_gate_retells_on_change_and_every_retell_frames() {
        let mut s = HeldSlot::default();
        let i = HeldIn {
            remote: true,
            state: 8,
            charge: 0.0,
            same: true,
            seq: 0,
        };
        // First frame: state -1 differs, told.
        assert_eq!(held_step(&mut s, &i, 4, 256), (true, 0));
        assert_eq!(held_step(&mut s, &i, 4, 256), (false, 0));
        assert_eq!(held_step(&mut s, &i, 4, 256), (false, 0));
        assert_eq!(held_step(&mut s, &i, 4, 256), (false, 0));
        // told reaches 4: told again.
        assert_eq!(held_step(&mut s, &i, 4, 256), (true, 0));
        let fire = HeldIn { seq: 3, ..i };
        assert_eq!(held_step(&mut s, &fire, 4, 256), (false, 3));
        let other = HeldIn {
            same: false,
            ..fire
        };
        assert_eq!(held_step(&mut s, &other, 4, 256), (true, 0));
        let local = HeldIn {
            remote: false,
            ..fire
        };
        assert_eq!(held_step(&mut s, &local, 4, 256), (true, 0));
    }

    #[test]
    fn held_gate_keeps_slots_by_id() {
        let mut g = HeldGate::default();
        let i = HeldIn {
            remote: true,
            state: 8,
            same: true,
            ..Default::default()
        };
        g.step(&[1, 2], &[i, HeldIn { seq: 2, ..i }], 30, 256);
        assert_eq!(g.codes, vec![1, (2 << 1) | 1]);
        g.step(&[2], &[HeldIn { seq: 2, ..i }], 30, 256);
        assert_eq!(g.codes, vec![0]);
        assert_eq!(g.slot(1).map(|s| s.told), Some(0));
        g.forget(1);
        assert!(g.slot(1).is_none());
        g.seed(
            3,
            HeldSlot {
                told: 2,
                state: 8,
                charge: 0.0,
                seen: 5,
            },
        );
        g.step(&[3], &[HeldIn { seq: 6, ..i }], 30, 256);
        assert_eq!(g.codes, vec![1 << 1]);
        assert_eq!(g.slot(3).map(|s| s.told), Some(3));
    }

    #[test]
    fn held_gate_forgets_bodies_not_stepped() {
        let mut g = HeldGate::default();
        let i = HeldIn::default();
        g.step(&[1], &[i], 30, 256);
        for _ in 1..2 * HELD_KEEP {
            g.step(&[2], &[i], 30, 256);
        }
        assert!(g.slot(1).is_none());
        assert!(g.slot(2).is_some());
    }

    #[test]
    fn tag_basis_scales_columns() {
        let b = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let t = tag_basis(&b, 2.0);
        assert_eq!(t, [[0.0, -2.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 2.0]]);
    }

    #[test]
    fn anim_numbers() {
        let o = anim_one(
            &AnimIn {
                velocity: [3.0, 1.0, 4.0],
                position: [1.0, 2.0, 3.0],
                wall_normal: [1.0, 0.0, 0.0],
                radius: 0.5,
                pitch: 90.0,
                previous_seq: 1,
                seq: 2,
                melee: true,
            },
            256,
        );
        assert_eq!(o.speed, 5.0);
        assert_eq!(o.aim, 90.0 * (std::f64::consts::PI / 180.0));
        assert_eq!(o.wall, [1.0, 0.0, 0.0, 0.5]);
        assert!(o.swing);
        let still = anim_one(&AnimIn::default(), 256);
        assert_eq!(still.wall, [0.0; 4]);
        assert!(!still.swing);
    }
}
