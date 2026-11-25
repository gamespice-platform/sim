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
}
