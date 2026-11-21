//! The rest of a zombie's steer after its path and spacing: dot-npc-ai's `steer_with_spacing` from the
//! spaced heading on, through dot-npc-ai's `steer_toward` bookkeeping, Zombie Zone's `steer_toward` (the
//! body's wish), dot-npc-ai's `face` and `GSNpcAiCharacter.turn_view` (the eased, clamped turn with its
//! overshoot), dot-npc's `face` (the point the body looks at) and `_watch_progress`.
//!
//! Pure: [`steer`] takes the turn state and the watch and returns what the script would have written,
//! in the script's order and precision (engine vectors `f32`, script floats `f64`).

use dot_sim_math::{Vec3, clampf, maxf, minf};

/// `Vector3.UP`.
pub const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// `Vector3.RIGHT`.
pub const RIGHT: Vec3 = Vec3::new(1.0, 0.0, 0.0);
/// `Vector3.INF`.
pub const INF: Vec3 = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);

/// `GSNpcAiCharacter.MASK`: keeps the mixer positive.
const MASK: i64 = i64::MAX;

/// `GSNpcAiCharacter.unit(salt)`: a reproducible number in [0, 1) from the character's seed and `salt`.
/// GDScript ints wrap on overflow, so the products wrap here.
pub fn unit(seed: i64, salt: i64) -> f64 {
    let mut x = (seed.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA6B)) & MASK;
    x = (x ^ (x >> 33)) & MASK;
    x = x.wrapping_mul(0xC2B2_AE3D) & MASK;
    x = (x ^ (x >> 29)) & MASK;
    x = x.wrapping_mul(0x27D4_EB2F) & MASK;
    x = (x ^ (x >> 31)) & MASK;
    let bits = (x >> 10) & 0x1F_FFFF_FFFF_FFFF;
    bits as f64 / 0x20_0000_0000_0000_i64 as f64
}

/// `Vector3.rotated(axis, angle)`: `Basis(axis, angle)` (its `set_axis_angle`) applied to `v`.
pub fn rotated(v: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    let sq = Vec3::new(axis.x * axis.x, axis.y * axis.y, axis.z * axis.z);
    let cosine = angle.cos();
    let r00 = sq.x + cosine * (1.0 - sq.x);
    let r11 = sq.y + cosine * (1.0 - sq.y);
    let r22 = sq.z + cosine * (1.0 - sq.z);
    let sine = angle.sin();
    let t = 1.0 - cosine;
    let (xyzt, zyxs) = (axis.x * axis.y * t, axis.z * sine);
    let (r01, r10) = (xyzt - zyxs, xyzt + zyxs);
    let (xyzt, zyxs) = (axis.x * axis.z * t, axis.y * sine);
    let (r02, r20) = (xyzt + zyxs, xyzt - zyxs);
    let (xyzt, zyxs) = (axis.y * axis.z * t, axis.x * sine);
    let (r12, r21) = (xyzt - zyxs, xyzt + zyxs);
    Vec3::new(
        Vec3::new(r00, r01, r02).dot(v),
        Vec3::new(r10, r11, r12).dot(v),
        Vec3::new(r20, r21, r22).dot(v),
    )
}

/// The script's `deg_to_rad`, in `f64`.
#[inline]
pub fn deg_to_rad(d: f64) -> f64 {
    d * (std::f64::consts::PI / 180.0)
}

/// `Math::is_equal_approx(float, float)`: equal, or within `CMP_EPSILON` scaled by `a` (at least `CMP_EPSILON`).
#[inline]
fn approx(a: f32, b: f32) -> bool {
    if a == b {
        return true;
    }
    let mut tolerance = 0.00001 * a.abs();
    if tolerance < 0.00001 {
        tolerance = 0.00001;
    }
    (a - b).abs() < tolerance
}

/// `Vector3.is_equal_approx`.
#[inline]
pub fn is_equal_approx(a: Vec3, b: Vec3) -> bool {
    approx(a.x, b.x) && approx(a.y, b.y) && approx(a.z, b.z)
}

/// `Vector3.is_finite`.
#[inline]
pub fn is_finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// `Vector2(v.x, v.z).length()`, dot-npc-ai's `_flat_length`.
#[inline]
pub fn flat_length(v: Vec3) -> f32 {
    (v.x * v.x + v.z * v.z).sqrt()
}

/// A `GSNpcAiCharacter`'s turn state: `_peak_turn`, `_over_amt`, `_over_axis`, `_turns`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    pub peak: f64,
    pub over_amt: f64,
    pub over_axis: Vec3,
    pub turns: i64,
}

impl Default for Turn {
    fn default() -> Self {
        Self {
            peak: 0.0,
            over_amt: 0.0,
            over_axis: UP,
            turns: 0,
        }
    }
}

/// A `GSNpcAiCharacter`'s turning: `view_factor`, `view_turn_deg`, `overshoot`, `seed_value`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurnProfile {
    pub view_factor: f64,
    pub view_turn_deg: f64,
    pub overshoot: f64,
    pub seed: i64,
}

impl Default for TurnProfile {
    fn default() -> Self {
        Self {
            view_factor: 0.6,
            view_turn_deg: 360.0,
            overshoot: 0.4,
            seed: 0,
        }
    }
}

/// `GSNpcAiCharacter.turn_view`: `current` turned toward `wanted`, no faster than the character can.
pub fn turn_view(t: &mut Turn, p: &TurnProfile, current: Vec3, wanted: Vec3, delta: f64) -> Vec3 {
    if f64::from(current.length_squared()) <= 0.0001 {
        return wanted;
    }
    if f64::from(wanted.length_squared()) <= 0.0001 {
        return current;
    }

    let from = current.normalized();
    let mut to = wanted.normalized();

    if t.over_amt > 0.001 {
        t.over_amt *= (-delta * 3.0).exp();
        to = rotated(to, t.over_axis, t.over_amt as f32);
    }

    let mut angle = f64::from(from.angle_to(to));
    t.peak = maxf(t.peak, angle);

    if t.peak >= 0.35 && angle <= t.peak * 0.15 {
        to = arm_overshoot(t, p, to, from, t.peak);
        t.peak = 0.0;
        angle = f64::from(from.angle_to(to));
    }

    if angle <= 0.0001 {
        t.peak = 0.0;
        return to;
    }

    let wanted_step = angle * clampf(p.view_factor * delta * 8.0, 0.0, 1.0);
    let limit = deg_to_rad(p.view_turn_deg) * delta;
    let step = minf(wanted_step, limit);

    if step >= angle {
        t.peak = 0.0;
        return to;
    }

    let mut axis = from.cross(to);
    if f64::from(axis.length_squared()) <= 0.000001 {
        axis = if f64::from(from.dot(UP)).abs() < 0.99 {
            UP
        } else {
            RIGHT
        };
    }
    rotated(from, axis.normalized(), step as f32)
}

/// `GSNpcAiCharacter._arm_overshoot`.
fn arm_overshoot(t: &mut Turn, p: &TurnProfile, to: Vec3, from: Vec3, peak: f64) -> Vec3 {
    if p.overshoot <= 0.0 || t.over_amt > 0.001 {
        return to;
    }
    let axis = from.cross(to);
    if f64::from(axis.length_squared()) <= 0.000001 {
        return to;
    }
    t.turns = t.turns.wrapping_add(1);
    t.over_axis = axis.normalized();
    t.over_amt = minf(peak, 1.57)
        * p.overshoot
        * (0.15 + 0.25 * unit(p.seed, t.turns.wrapping_mul(13).wrapping_add(0x71)));
    rotated(to, t.over_axis, t.over_amt as f32)
}

/// What a steer reads beyond the turn: where the body is and faces, the spaced heading, and the watch.
#[derive(Clone, Copy, Debug)]
pub struct SteerIn {
    /// `npc.position()`.
    pub here: Vec3,
    /// `npc.facing()` now, and `_facing_at_tick` (where it faced as the think began).
    pub facing: Vec3,
    pub facing_at_tick: Vec3,
    /// `_spaced_direction(here, goal, spacing)`: flat and unit, or zero.
    pub direction: Vec3,
    /// The final destination (`to`) the watch measures against.
    pub to: Vec3,
    pub speed: f64,
    /// `context.delta`.
    pub delta: f64,
    /// `_world_now()`, `_progress_at`, `_progress_since`, `stuck_seconds`.
    pub now: f64,
    pub progress_at: Vec3,
    pub progress_since: f64,
    pub stuck_seconds: f64,
}

/// What the watch decided: nothing, start over from here, or stuck long enough for the path's rough corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    Keep,
    Reset,
    Stuck,
}

/// What the script would have written. `steered` false: the heading cancelled and nothing was touched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SteerOut {
    pub steered: bool,
    /// The body's new `wish`, when one was set.
    pub wish: Option<Vec3>,
    /// The point `look_at` turned the body to, when it did.
    pub look: Option<Vec3>,
    pub watch: Watch,
}

/// dot-npc-ai's `face(direction)` on a living 3D body, then dot-npc's `face`: the turn ([`turn_view`] from
/// `facing_at_tick`, or `facing` when that is unset; no turn when `delta` is not positive) and the point
/// `look_at` turns the body to, `None` when it is not turned. `t` is updated as the character's turn state would be.
pub fn face(
    t: &mut Turn,
    p: &TurnProfile,
    here: Vec3,
    facing: Vec3,
    facing_at_tick: Vec3,
    direction: Vec3,
    delta: f64,
) -> Option<Vec3> {
    let faced = if delta <= 0.0 {
        direction
    } else {
        let flat = Vec3::new(direction.x, 0.0, direction.z);
        if f64::from(flat.length_squared()) < 0.000001 {
            return None;
        }
        let from = if f64::from(facing_at_tick.length_squared()) > 0.0001 {
            facing_at_tick
        } else {
            facing
        };
        turn_view(t, p, from, flat.normalized(), delta)
    };

    let flat = Vec3::new(faced.x, 0.0, faced.z);
    if f64::from(flat.length()) < 0.001 {
        return None;
    }
    let target = here + flat.normalized();
    (is_finite(here) && !is_equal_approx(target, here)).then_some(target)
}

/// dot-npc-ai's `steer_with_spacing` after its spaced heading, on Zombie Zone's `steer_toward` (the wish) with
/// no avoidance node: the wish, the turn ([`turn_view`] from `facing_at_tick`, or `facing` when that is
/// unset; no turn when `delta` is not positive), the look and the progress watch. `t` is updated as the
/// character's turn state would be.
pub fn steer(t: &mut Turn, p: &TurnProfile, s: &SteerIn) -> SteerOut {
    let mut out = SteerOut {
        steered: false,
        wish: None,
        look: None,
        watch: Watch::Keep,
    };

    if f64::from(s.direction.length()) < 0.001 {
        return out;
    }
    out.steered = true;

    // steer_toward(here + direction * 4.0, speed, delta), measured from npc.position() again.
    let to = s.here + s.direction.scale(4.0);
    let mut direction = to - s.here;
    direction.y = 0.0;

    if f64::from(direction.length()) >= 0.001 {
        direction = direction.normalized();
        out.wish = Some(direction.scale(s.speed));
        out.look = face(t, p, s.here, s.facing, s.facing_at_tick, direction, s.delta);
    }

    // _watch_progress(here, to)
    out.watch = if s.progress_at == INF
        || f64::from(flat_length(s.here - s.progress_at)) > 0.5
        || f64::from(flat_length(s.to - s.here)) < 2.0
    {
        Watch::Reset
    } else if s.now - s.progress_since >= s.stuck_seconds {
        Watch::Stuck
    } else {
        Watch::Keep
    };

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn unit_is_a_reproducible_fraction() {
        for seed in [0_i64, 1, 7, 123_456_789, i64::MAX, -5] {
            for salt in [0_i64, 0x71, 13 + 0x71, 1 << 40] {
                let u = unit(seed, salt);
                assert!((0.0..1.0).contains(&u), "{seed} {salt} {u}");
                assert_eq!(u, unit(seed, salt));
            }
        }
        assert_ne!(unit(1, 0x7e), unit(2, 0x7e));
    }

    #[test]
    fn rotated_turns_about_up() {
        let r = rotated(v(1.0, 0.0, 0.0), UP, std::f32::consts::FRAC_PI_2);
        assert!((r.x).abs() < 1e-6 && (r.z + 1.0).abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn turn_view_is_clamped_by_rate_and_ease() {
        let mut t = Turn::default();
        let p = TurnProfile::default();
        let from = v(0.0, 0.0, -1.0);
        let to = v(1.0, 0.0, 0.0);
        let delta = 1.0 / 60.0;
        let out = turn_view(&mut t, &p, from, to, delta);
        let step = f64::from(from.angle_to(out));
        let want =
            (std::f64::consts::FRAC_PI_2 * (0.6 * delta * 8.0)).min(deg_to_rad(360.0) * delta);
        assert!((step - want).abs() < 1e-4, "{step} {want}");
        assert!(t.peak > 1.5);
        assert_eq!(t.turns, 0);
    }

    #[test]
    fn turn_view_arms_an_overshoot_when_settling() {
        let mut t = Turn {
            peak: 1.0,
            ..Turn::default()
        };
        let p = TurnProfile {
            seed: 42,
            ..TurnProfile::default()
        };
        let from = v(0.0, 0.0, -1.0);
        let to = rotated(from, UP, 0.05);
        turn_view(&mut t, &p, from, to, 1.0 / 60.0);
        assert_eq!(t.turns, 1);
        assert!(
            t.over_amt > 0.0 && t.over_amt <= 1.0 * 0.4 * 0.4 + 1e-9,
            "{}",
            t.over_amt
        );
        assert_eq!(t.peak, 0.0);

        // No overshoot without one configured.
        let mut t = Turn {
            peak: 1.0,
            ..Turn::default()
        };
        let p = TurnProfile {
            overshoot: 0.0,
            ..TurnProfile::default()
        };
        turn_view(&mut t, &p, from, to, 1.0 / 60.0);
        assert_eq!(t.turns, 0);
        assert_eq!(t.over_amt, 0.0);
    }

    #[test]
    fn turn_view_opposite_uses_a_fallback_axis() {
        let mut t = Turn::default();
        let p = TurnProfile::default();
        let out = turn_view(&mut t, &p, v(0.0, 0.0, -1.0), v(0.0, 0.0, 1.0), 1.0 / 60.0);
        assert!(out.length() > 0.99 && out.length() < 1.01);
        assert!(out.z < 0.0);
    }

    #[test]
    fn turn_view_passes_through_degenerate_inputs() {
        let mut t = Turn::default();
        let p = TurnProfile::default();
        assert_eq!(
            turn_view(&mut t, &p, Vec3::default(), v(1.0, 0.0, 0.0), 0.1),
            v(1.0, 0.0, 0.0)
        );
        assert_eq!(
            turn_view(&mut t, &p, v(1.0, 0.0, 0.0), Vec3::default(), 0.1),
            v(1.0, 0.0, 0.0)
        );
    }

    fn input() -> SteerIn {
        SteerIn {
            here: v(1.0, 0.0, 1.0),
            facing: v(0.0, 0.0, -1.0),
            facing_at_tick: Vec3::default(),
            direction: v(1.0, 0.0, 0.0),
            to: v(10.0, 0.0, 1.0),
            speed: 3.0,
            delta: 1.0 / 60.0,
            now: 5.0,
            progress_at: v(1.0, 0.0, 1.0),
            progress_since: 4.0,
            stuck_seconds: 2.0,
        }
    }

    #[test]
    fn steer_wishes_turns_and_looks() {
        let mut t = Turn::default();
        let out = steer(&mut t, &TurnProfile::default(), &input());
        assert!(out.steered);
        assert_eq!(out.wish, Some(v(3.0, 0.0, 0.0)));
        let look = out.look.expect("looks");
        let faced = look - v(1.0, 0.0, 1.0);
        assert!(faced.x > 0.0 && faced.z < 0.0, "{faced:?}");
        assert_eq!(out.watch, Watch::Keep);
    }

    #[test]
    fn steer_without_a_heading_does_nothing() {
        let mut t = Turn::default();
        let mut s = input();
        s.direction = Vec3::default();
        let out = steer(&mut t, &TurnProfile::default(), &s);
        assert!(!out.steered);
        assert_eq!(out.wish, None);
        assert_eq!(t, Turn::default());
    }

    #[test]
    fn steer_without_delta_faces_directly() {
        let mut t = Turn::default();
        let mut s = input();
        s.delta = 0.0;
        let out = steer(&mut t, &TurnProfile::default(), &s);
        assert_eq!(out.look, Some(v(2.0, 0.0, 1.0)));
        assert_eq!(t, Turn::default());
    }

    #[test]
    fn face_turns_flattens_and_skips_what_face_skips() {
        let p = TurnProfile::default();
        let here = v(1.0, 0.0, 1.0);
        let ahead = v(0.0, 0.0, -1.0);
        // A climb's height is dropped before the turn; a turn from facing_at_tick, else the body's facing.
        let mut a = Turn::default();
        let mut b = Turn::default();
        let up = face(
            &mut a,
            &p,
            here,
            ahead,
            Vec3::default(),
            v(2.0, 5.0, 0.0),
            1.0 / 60.0,
        );
        let flat = face(
            &mut b,
            &p,
            here,
            ahead,
            Vec3::default(),
            v(1.0, 0.0, 0.0),
            1.0 / 60.0,
        );
        assert_eq!(up, flat);
        assert_eq!(a, b);
        assert!(a.peak > 0.0);
        let mut c = Turn::default();
        let from_tick = face(
            &mut c,
            &p,
            here,
            v(1.0, 0.0, 0.0),
            ahead,
            v(1.0, 0.0, 0.0),
            1.0 / 60.0,
        );
        assert_eq!(from_tick, flat);
        // Straight up or nothing: no look and no turn.
        let mut t = Turn::default();
        assert_eq!(
            face(&mut t, &p, here, ahead, ahead, v(0.0, 3.0, 0.0), 1.0 / 60.0),
            None
        );
        assert_eq!(
            face(&mut t, &p, here, ahead, ahead, v(0.0, 3.0, 0.0), 0.0),
            None
        );
        assert_eq!(t, Turn::default());
        // No delta: faced as asked, untouched; a body off the map is not looked round.
        assert_eq!(
            face(&mut t, &p, here, ahead, ahead, v(0.0, 0.0, 2.0), 0.0),
            Some(v(1.0, 0.0, 2.0))
        );
        assert_eq!(
            face(&mut t, &p, INF, ahead, ahead, v(0.0, 0.0, 2.0), 0.0),
            None
        );
        assert_eq!(t, Turn::default());
    }

    #[test]
    fn watch_resets_and_flags_stuck() {
        let p = TurnProfile::default();
        let mut s = input();
        s.progress_at = INF;
        assert_eq!(steer(&mut Turn::default(), &p, &s).watch, Watch::Reset);
        s = input();
        s.progress_at = v(0.0, 0.0, 0.0);
        assert_eq!(steer(&mut Turn::default(), &p, &s).watch, Watch::Reset);
        s = input();
        s.to = v(2.0, 0.0, 1.0);
        assert_eq!(steer(&mut Turn::default(), &p, &s).watch, Watch::Reset);
        s = input();
        s.now = 6.0;
        assert_eq!(steer(&mut Turn::default(), &p, &s).watch, Watch::Stuck);
    }
}
