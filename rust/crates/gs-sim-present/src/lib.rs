//! Fatal Contact's `FcPlayer.present_lod` judgement for every remote body of a frame at once.
//!
//! The script asks, per body: how far from the eye (squared), is it beyond the figure draw range, is it out of
//! sight (the camera's six frustum planes against a sphere round the chest, else the cone round the view axis),
//! and which distance tier is it in. [`classify`] answers all of that from packed positions, in the script's
//! order and precision: engine `Vector3` math in `f32`, the thresholds and `sqrt` in `f64`, as GDScript floats are.
//! What stays in the script is the one thing that is not arithmetic: the occluder ray, asked only of a body
//! [`OCCLUSION_CANDIDATE`] names.

use gs_sim_math::Vec3;

/// Tier 0: presented in full every frame, no animation gate.
pub const TIER_FULL: i32 = 0;
/// Tier 1: past `ANIM_GATE_NEAR`: animation gated at the near rate.
pub const TIER_ANIM_NEAR: i32 = 1;
/// Tier 2: past `ANIM_GATE_FAR`: animation gated at the far rate.
pub const TIER_ANIM_FAR: i32 = 2;
/// Tier 3: past `LOD_FAR`: presented every second frame.
pub const TIER_LOD_FAR: i32 = 3;
/// Tier 4: past `LOD_FARTHEST`: presented every fourth frame.
pub const TIER_LOD_FARTHEST: i32 = 4;
/// Tier 5: out of sight: presented at the off-screen rate.
pub const TIER_OFF: i32 = 5;
/// The low three bits of a code are the tier.
pub const TIER_MASK: i32 = 7;
/// Bit: the body is past the figure draw range (only ever set when the range is above 0).
pub const BEYOND_FIGURE: i32 = 8;
/// Bit: in the frustum and past the near range, so the script must ask the occluder ray; if that says hidden, the
/// body is [`TIER_OFF`] instead of the tier in the code.
pub const OCCLUSION_CANDIDATE: i32 = 16;

/// A plane as `Plane.distance_to` sees it: `normal.dot(p) - d`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub d: f32,
}

impl Plane {
    /// Signed distance of `p` from the plane, in `f32` as the engine computes it.
    #[inline]
    pub fn distance_to(self, p: Vec3) -> f32 {
        self.normal.dot(p) - self.d
    }
}

/// The frame's settings, squared distances and all, as the script holds them (`f64`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    /// `LOD_NEAR * LOD_NEAR`.
    pub near2: f64,
    /// `LOD_CONE`.
    pub cone: f64,
    /// `LOD_RADIUS`.
    pub radius: f64,
    pub farthest2: f64,
    pub far2: f64,
    pub anim_far2: f64,
    pub anim_near2: f64,
    /// `figure_draw_range * figure_draw_range`, or 0 or less for no limit.
    pub figure2: f64,
    /// Judge by the frustum (the planes handed in are not empty and `lod_frustum` is on), else by the cone.
    pub frustum_on: bool,
    /// `occlusion_enabled`.
    pub occlusion: bool,
}

/// `Vector3.length_squared()`: summed x, y, z in `f32`.
#[inline]
fn length_squared(v: Vec3) -> f32 {
    v.x * v.x + v.y * v.y + v.z * v.z
}

/// The code of one body at `pos`, seen from `eye` looking along `forward`.
pub fn classify_one(cfg: &Config, pos: Vec3, eye: Vec3, forward: Vec3, planes: &[Plane]) -> i32 {
    let to = Vec3::new(pos.x - eye.x, pos.y - eye.y, pos.z - eye.z);
    let d2 = f64::from(length_squared(to));
    let mut code = 0;
    if cfg.figure2 > 0.0 && d2 > cfg.figure2 {
        code |= BEYOND_FIGURE;
    }
    if d2 > cfg.near2 {
        if cfg.frustum_on {
            let centre = Vec3::new(pos.x, pos.y + 1.0, pos.z);
            for plane in planes {
                if f64::from(plane.distance_to(centre)) > cfg.radius {
                    return code | TIER_OFF;
                }
            }
            if cfg.occlusion {
                code |= OCCLUSION_CANDIDATE;
            }
        } else if f64::from(to.dot(forward)) < cfg.cone * d2.sqrt() {
            return code | TIER_OFF;
        }
    }
    code | if d2 > cfg.farthest2 {
        TIER_LOD_FARTHEST
    } else if d2 > cfg.far2 {
        TIER_LOD_FAR
    } else if d2 > cfg.anim_far2 {
        TIER_ANIM_FAR
    } else if d2 > cfg.anim_near2 {
        TIER_ANIM_NEAR
    } else {
        TIER_FULL
    }
}

/// Fills `out` with one code per body of `positions` (x, y, z in a row each); a trailing partial triple is ignored.
pub fn classify(
    cfg: &Config,
    positions: &[f32],
    eye: Vec3,
    forward: Vec3,
    planes: &[Plane],
    out: &mut Vec<i32>,
) {
    out.clear();
    out.extend(
        positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| classify_one(cfg, Vec3::new(p[0], p[1], p[2]), eye, forward, planes)),
    );
}

/// The frame rates and minimum game times of the tiers, as `FcPlayer.present_lod_coded` and `_thinned` hold them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Thinning {
    /// `lod_off_every`: frames between presents of a body out of sight.
    pub off_every: i32,
    /// `LOD_OFF_DT`, `LOD_FARTHEST_DT`, `LOD_FAR_DT`: the least game time between thinned presents.
    pub off_dt: f64,
    pub farthest_dt: f64,
    pub far_dt: f64,
    /// `ANIM_GATE_OFF_DT`, `ANIM_GATE_FAR_DT`, `ANIM_GATE_DT`: the animation gate handed to a presented body.
    pub anim_off_dt: f64,
    pub anim_far_dt: f64,
    pub anim_dt: f64,
    /// `anim_gate_enabled`: thin by game time as well as by frame count.
    pub anim_gate: bool,
}

impl Thinning {
    /// `(every, min_dt, anim_min_dt)` of a tier, as the script's `match tier`.
    pub fn of(&self, tier: i32) -> (i32, f64, f64) {
        match tier {
            TIER_OFF => (self.off_every, self.off_dt, self.anim_off_dt),
            TIER_LOD_FARTHEST => (4, self.farthest_dt, self.anim_far_dt),
            TIER_LOD_FAR => (2, self.far_dt, self.anim_far_dt),
            TIER_ANIM_FAR => (1, 0.0, self.anim_far_dt),
            TIER_ANIM_NEAR => (1, 0.0, self.anim_dt),
            _ => (1, 0.0, 0.0),
        }
    }
}

/// One remote body's thinning between frames: the script's `_lod_frames`, `_lod_delta`, `_held_thin` and
/// `beyond_figure` as last handed out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slot {
    pub frames: i32,
    pub delta: f64,
    pub thin: bool,
    pub beyond: bool,
    seen: u32,
    stale: bool,
}

/// Every remote body's thinning, kept here between frames, keyed by the body's id (its instance id; 0 is a body
/// presented in full and never thinned: the viewer's own). [`Scheduler::step`] is `FcPlayer.present_lod_coded`
/// for the whole crowd: which bodies present this frame and with what delta, and which held guns and figure-range
/// bits changed. A body seen for the first time hands out both bits, so the script's own copies always agree.
#[derive(Debug, Default)]
pub struct Scheduler {
    slots: std::collections::HashMap<i64, Slot>,
    frame: u32,
    /// Rows of (body index, delta to present with, animation gate or NaN to leave it): the bodies to present, in order.
    pub presents: Vec<f64>,
    /// `index << 1 | thin` for each body whose held gun changes thinning this frame.
    pub held: Vec<i32>,
    /// `index << 1 | beyond` for each body whose figure-range bit changes this frame.
    pub beyond: Vec<i32>,
}

impl Scheduler {
    /// One frame: `ids` and `codes` one per body in present order (codes from [`classify`], a body past the end
    /// of `codes` is presented in full), `hidden` the ascending indices the occluder ray hid, `delta` the frame's
    /// game time. Bodies that left the crowd are forgotten.
    pub fn step(&mut self, t: &Thinning, ids: &[i64], codes: &[i32], hidden: &[i32], delta: f64) {
        self.presents.clear();
        self.held.clear();
        self.beyond.clear();
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        let mut next_hidden = hidden.iter().copied().peekable();
        for (i, &id) in ids.iter().enumerate() {
            let at = i as i32;
            let mut hid = false;
            while let Some(&h) = next_hidden.peek() {
                if h > at {
                    break;
                }
                hid |= h == at;
                next_hidden.next();
            }
            let Some(&code) = codes.get(i).filter(|_| id != 0) else {
                self.presents
                    .extend_from_slice(&[f64::from(at), delta, f64::NAN]);
                continue;
            };
            let mut fresh = !self.slots.contains_key(&id);
            let slot = self.slots.entry(id).or_default();
            slot.seen = frame;
            fresh |= std::mem::take(&mut slot.stale);
            let far = code & BEYOND_FIGURE != 0;
            if fresh || far != slot.beyond {
                slot.beyond = far;
                self.beyond.push(at << 1 | i32::from(far));
            }
            let tier = if hid { TIER_OFF } else { code & TIER_MASK };
            let (every, min_dt, anim) = t.of(tier);
            let thin = every > 1;
            if fresh || thin != slot.thin {
                slot.thin = thin;
                self.held.push(at << 1 | i32::from(thin));
            }
            let mut d = delta;
            if thin {
                slot.frames += 1;
                slot.delta += delta;
                if slot.frames < every || (t.anim_gate && slot.delta < min_dt) {
                    continue;
                }
                d = slot.delta;
            }
            slot.frames = 0;
            slot.delta = 0.0;
            self.presents.extend_from_slice(&[f64::from(at), d, anim]);
        }
        self.slots.retain(|_, s| s.seen == frame);
    }

    /// The thinning kept for the body `id`, if it was in the last frame's crowd.
    pub fn slot(&self, id: i64) -> Option<Slot> {
        self.slots.get(&id).copied()
    }

    /// Hands out both bits of body `id` again on the next step, its thinning kept: the body rebuilt the figure or
    /// the gun the bits act on.
    pub fn refresh(&mut self, id: i64) {
        if let Some(s) = self.slots.get_mut(&id) {
            s.stale = true;
        }
    }

    /// Forgets every body (a map change, a switch back to the script).
    pub fn clear(&mut self) {
        self.slots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            near2: 36.0,
            cone: 0.2,
            radius: 1.6,
            farthest2: 100.0 * 100.0,
            far2: 50.0 * 50.0,
            anim_far2: 30.0 * 30.0,
            anim_near2: 10.0 * 10.0,
            figure2: 140.0 * 140.0,
            frustum_on: false,
            occlusion: true,
        }
    }

    const EYE: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    const FWD: Vec3 = Vec3::new(0.0, 0.0, -1.0);

    fn at(c: &Config, x: f32, z: f32) -> i32 {
        classify_one(c, Vec3::new(x, 0.0, z), EYE, FWD, &[])
    }

    #[test]
    fn tiers_by_distance_in_front() {
        let c = cfg();
        assert_eq!(at(&c, 0.0, -3.0), TIER_FULL);
        assert_eq!(at(&c, 0.0, -20.0), TIER_ANIM_NEAR);
        assert_eq!(at(&c, 0.0, -40.0), TIER_ANIM_FAR);
        assert_eq!(at(&c, 0.0, -70.0), TIER_LOD_FAR);
        assert_eq!(at(&c, 0.0, -120.0), TIER_LOD_FARTHEST);
        assert_eq!(at(&c, 0.0, -150.0), TIER_LOD_FARTHEST | BEYOND_FIGURE);
    }

    #[test]
    fn behind_the_cone_is_off_but_only_past_near() {
        let c = cfg();
        assert_eq!(at(&c, 0.0, 20.0), TIER_OFF);
        assert_eq!(at(&c, 0.0, 3.0), TIER_FULL);
    }

    #[test]
    fn frustum_planes_and_occlusion_candidate() {
        let mut c = cfg();
        c.frustum_on = true;
        let planes = [Plane {
            normal: Vec3::new(1.0, 0.0, 0.0),
            d: 0.0,
        }];
        let out = classify_one(&c, Vec3::new(5.0, 0.0, -20.0), EYE, FWD, &planes);
        assert_eq!(out, TIER_OFF);
        let inside = classify_one(&c, Vec3::new(-5.0, 0.0, -20.0), EYE, FWD, &planes);
        assert_eq!(inside, TIER_ANIM_NEAR | OCCLUSION_CANDIDATE);
        c.occlusion = false;
        let plain = classify_one(&c, Vec3::new(-5.0, 0.0, -20.0), EYE, FWD, &planes);
        assert_eq!(plain, TIER_ANIM_NEAR);
        let near = classify_one(&c, Vec3::new(-1.0, 0.0, -2.0), EYE, FWD, &planes);
        assert_eq!(near, TIER_FULL);
    }

    #[test]
    fn no_figure_range_never_sets_the_bit() {
        let mut c = cfg();
        c.figure2 = 0.0;
        assert_eq!(at(&c, 0.0, -500.0), TIER_LOD_FARTHEST);
    }

    #[test]
    fn classify_packs_one_code_per_body() {
        let c = cfg();
        let mut out = Vec::new();
        classify(
            &c,
            &[0.0, 0.0, -3.0, 0.0, 0.0, -40.0, 9.0],
            EYE,
            FWD,
            &[],
            &mut out,
        );
        assert_eq!(out, vec![TIER_FULL, TIER_ANIM_FAR]);
    }

    fn thinning() -> Thinning {
        Thinning {
            off_every: 4,
            off_dt: 1.0 / 30.0,
            farthest_dt: 1.0 / 30.0,
            far_dt: 1.0 / 45.0,
            anim_off_dt: 0.1,
            anim_far_dt: 0.05,
            anim_dt: 1.0 / 30.0,
            anim_gate: true,
        }
    }

    /// The script's `present_lod_coded` + `_thinned` for one body, as the reference.
    #[derive(Default)]
    struct Script {
        frames: i32,
        delta: f64,
        thin: bool,
    }

    impl Script {
        fn step(&mut self, t: &Thinning, tier: i32, delta: f64) -> Option<(f64, f64)> {
            let (every, min_dt, anim) = t.of(tier);
            self.thin = every > 1;
            let mut d = delta;
            if every > 1 {
                self.frames += 1;
                self.delta += delta;
                if self.frames < every || (t.anim_gate && self.delta < min_dt) {
                    return None;
                }
                d = self.delta;
            }
            self.frames = 0;
            self.delta = 0.0;
            Some((d, anim))
        }
    }

    #[test]
    fn schedule_matches_the_script_frame_after_frame() {
        let t = thinning();
        let mut s = Scheduler::default();
        let ids = [11_i64, 0, 13, 14, 15, 16];
        let mut refs: Vec<Script> = (0..ids.len()).map(|_| Script::default()).collect();
        let mut seed = 7_u32;
        for f in 0..400 {
            let mut codes = Vec::new();
            let mut hidden = Vec::new();
            for i in 0..ids.len() {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let beyond = if (seed >> 8) & 1 == 1 {
                    BEYOND_FIGURE
                } else {
                    0
                };
                let mut c = ((seed >> 16) % 6) as i32 | beyond;
                if (seed >> 9).is_multiple_of(4) {
                    c |= OCCLUSION_CANDIDATE;
                    if (seed >> 11) & 1 == 1 {
                        hidden.push(i as i32);
                    }
                }
                codes.push(c);
            }
            let delta = 0.004 + f64::from(f % 7) * 0.003;
            s.step(&t, &ids, &codes, &hidden, delta);
            let mut want = Vec::new();
            for (i, r) in refs.iter_mut().enumerate() {
                if ids[i] == 0 {
                    want.extend_from_slice(&[i as f64, delta, f64::NAN]);
                    continue;
                }
                let tier = if hidden.contains(&(i as i32)) {
                    TIER_OFF
                } else {
                    codes[i] & TIER_MASK
                };
                if let Some((d, a)) = r.step(&t, tier, delta) {
                    want.extend_from_slice(&[i as f64, d, a]);
                }
                let slot = s.slot(ids[i]).unwrap();
                assert_eq!(
                    (slot.frames, slot.delta.to_bits(), slot.thin),
                    (r.frames, r.delta.to_bits(), r.thin)
                );
                assert_eq!(slot.beyond, codes[i] & BEYOND_FIGURE != 0);
            }
            let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
            assert_eq!(bits(&s.presents), bits(&want), "frame {f}");
        }
    }

    #[test]
    fn schedule_hands_bits_to_new_bodies_and_forgets_gone_ones() {
        let t = thinning();
        let mut s = Scheduler::default();
        let codes = [TIER_FULL, TIER_LOD_FAR | BEYOND_FIGURE];
        s.step(&t, &[5, 6], &codes, &[], 0.016);
        assert_eq!(s.beyond, vec![0, 3]);
        assert_eq!(s.held, vec![0, 3]);
        s.step(&t, &[5, 6], &codes, &[], 0.016);
        assert!(s.beyond.is_empty() && s.held.is_empty());
        s.step(&t, &[6], &[TIER_FULL], &[], 0.016);
        assert_eq!((s.beyond.clone(), s.held.clone()), (vec![0], vec![0]));
        assert_eq!(s.slot(6).map(|x| (x.beyond, x.thin)), Some((false, false)));
        assert!(s.slot(5).is_none());
        s.refresh(6);
        s.step(&t, &[6], &[TIER_LOD_FAR], &[], 0.016);
        assert_eq!((s.beyond.clone(), s.held.clone()), (vec![0], vec![1]));
        assert_eq!(s.slot(6).map(|x| x.frames), Some(1));
        s.step(&t, &[6], &[TIER_LOD_FAR], &[], 0.016);
        assert!(s.beyond.is_empty() && s.held.is_empty());
    }

    #[test]
    fn schedule_with_short_codes_presents_in_full() {
        let t = thinning();
        let mut s = Scheduler::default();
        s.step(&t, &[5, 6], &[TIER_OFF], &[], 0.01);
        assert_eq!(s.presents.len(), 3);
        assert_eq!(s.presents[0], 1.0);
        assert_eq!(s.presents[1], 0.01);
    }
}
