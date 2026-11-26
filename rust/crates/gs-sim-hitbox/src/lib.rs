//! GameSpice Combat's hitbox narrow phase: which hitbox of a set a ray meets first.
//!
//! The same arithmetic as `GSHitbox.intersect_ray` and `GSHitboxSet.intersect_ray`, step for
//! step and in the same precision (vectors in f32, script floats in f64), so the native answer
//! is the script's answer to the bit. The caller then builds the hit from the chosen hitbox.

use gs_sim_math::{Vec3, Xform, clampf, maxf, minf};

/// `GSHitbox.Shape`: the script's enum values.
pub const SHAPE_CAPSULE: i64 = 0;
pub const SHAPE_BOX: i64 = 1;
pub const SHAPE_SPHERE: i64 = 2;

/// One hitbox as the script sees it this call.
#[derive(Clone, Copy, Debug)]
pub struct Hitbox {
    pub xform: Xform,
    pub shape: i64,
    pub radius: f64,
    pub height: f64,
    pub half_extents: Vec3,
    pub enabled: bool,
    pub precedence: i64,
    pub damage_scale: f64,
}

/// `GSHitbox._ray_sphere`: entry distance, 0 from inside, -1 for a miss.
pub fn ray_sphere(from: Vec3, dir: Vec3, centre: Vec3, radius: f64) -> f64 {
    let offset = from - centre;
    let b = f64::from(offset.dot(dir));
    let c = f64::from(offset.length_squared()) - radius * radius;
    if c <= 0.0 {
        return 0.0;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return -1.0;
    }
    let t = -b - disc.sqrt();
    if t >= 0.0 { t } else { -1.0 }
}

fn axis(v: Vec3, i: usize) -> f64 {
    f64::from(match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    })
}

/// `GSHitbox._ray_box`: slab test about the origin.
pub fn ray_box(from: Vec3, dir: Vec3, half: Vec3) -> f64 {
    let mut t_min = f64::NEG_INFINITY;
    let mut t_max = f64::INFINITY;
    for i in 0..3 {
        let d = axis(dir, i);
        let o = axis(from, i);
        let e = axis(half, i);
        if d.abs() < 0.000001 {
            if o < -e || o > e {
                return -1.0;
            }
            continue;
        }
        let mut t1 = (-e - o) / d;
        let mut t2 = (e - o) / d;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = maxf(t_min, t1);
        t_max = minf(t_max, t2);
        if t_min > t_max {
            return -1.0;
        }
    }
    if t_max < 0.0 {
        return -1.0;
    }
    maxf(0.0, t_min)
}

/// `GSHitbox._ray_capsule`: a Y-axis capsule of `height` (cap to cap) and `radius`.
pub fn ray_capsule(from: Vec3, dir: Vec3, radius: f64, height: f64) -> f64 {
    let half = maxf(0.0, height * 0.5 - radius);
    let dx = f64::from(dir.x);
    let dz = f64::from(dir.z);
    let a = dx * dx + dz * dz;
    let mut best = -1.0;
    if a > 0.000001 {
        let ox = f64::from(from.x);
        let oz = f64::from(from.z);
        let b = ox * dx + oz * dz;
        let c = ox * ox + oz * oz - radius * radius;
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let root = disc.sqrt();
            for t in [(-b - root) / a, (-b + root) / a] {
                if t < 0.0 {
                    continue;
                }
                let y = f64::from(from.y) + f64::from(dir.y) * t;
                if y >= -half && y <= half {
                    best = if best < 0.0 { t } else { minf(best, t) };
                    break;
                }
            }
        }
    }
    for cap_y in [-half, half] {
        let centre = Vec3 {
            x: 0.0,
            y: cap_y as f32,
            z: 0.0,
        };
        let t = ray_sphere(from, dir, centre, radius);
        if t >= 0.0 {
            best = if best < 0.0 { t } else { minf(best, t) };
        }
    }
    if best < 0.0 {
        let on_axis = Vec3 {
            x: 0.0,
            y: clampf(f64::from(from.y), -half, half) as f32,
            z: 0.0,
        };
        if f64::from(from.distance_to(on_axis)) <= radius {
            return 0.0;
        }
    }
    best
}

/// `GSHitbox.intersect_ray`: the world distance of the hit, or None for a miss.
pub fn hitbox_ray(b: &Hitbox, from: Vec3, dir: Vec3, max_distance: f64) -> Option<f64> {
    if !b.enabled {
        return None;
    }
    let inverse = b.xform.affine_inverse();
    let local_from = inverse.xform(from);
    let local_dir = inverse.basis_xform(dir).normalized();
    let distance = match b.shape {
        SHAPE_SPHERE => ray_sphere(local_from, local_dir, Vec3::default(), b.radius),
        SHAPE_BOX => ray_box(local_from, local_dir, b.half_extents),
        _ => ray_capsule(local_from, local_dir, b.radius, b.height),
    };
    if distance < 0.0 {
        return None;
    }
    let local_hit = local_from + local_dir.scale(distance);
    let world_hit = b.xform.xform(local_hit);
    let world_distance = f64::from(from.distance_to(world_hit));
    if world_distance > max_distance {
        return None;
    }
    Some(world_distance)
}

/// `GSHitboxSet.intersect_ray`'s loop: the index of the hitbox whose hit the set reports, or None.
/// Nearer by more than 0.0001 wins; within it, higher precedence, then higher damage scale.
pub fn set_nearest(boxes: &[Hitbox], from: Vec3, dir: Vec3, max_distance: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in boxes.iter().enumerate() {
        let Some(d) = hitbox_ray(b, from, dir, max_distance) else {
            continue;
        };
        let Some((bi, bd)) = best else {
            best = Some((i, d));
            continue;
        };
        if d < bd - 0.0001 {
            best = Some((i, d));
            continue;
        }
        if (d - bd).abs() <= 0.0001 {
            let o = &boxes[bi];
            if b.precedence > o.precedence
                || (b.precedence == o.precedence && b.damage_scale > o.damage_scale)
            {
                best = Some((i, d));
            }
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn at(o: Vec3) -> Xform {
        Xform {
            rows: [v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0), v(0.0, 0.0, 1.0)],
            origin: o,
        }
    }

    fn sphere(o: Vec3, r: f64, precedence: i64, scale: f64) -> Hitbox {
        Hitbox {
            xform: at(o),
            shape: SHAPE_SPHERE,
            radius: r,
            height: 0.0,
            half_extents: Vec3::default(),
            enabled: true,
            precedence,
            damage_scale: scale,
        }
    }

    #[test]
    fn sphere_entry_and_inside() {
        assert_eq!(
            ray_sphere(v(0.0, 0.0, -5.0), v(0.0, 0.0, 1.0), Vec3::default(), 1.0),
            4.0
        );
        assert_eq!(
            ray_sphere(v(0.0, 0.0, 0.5), v(0.0, 0.0, 1.0), Vec3::default(), 1.0),
            0.0
        );
        assert_eq!(
            ray_sphere(v(0.0, 3.0, -5.0), v(0.0, 0.0, 1.0), Vec3::default(), 1.0),
            -1.0
        );
        assert_eq!(
            ray_sphere(v(0.0, 0.0, 5.0), v(0.0, 0.0, 1.0), Vec3::default(), 1.0),
            -1.0
        );
    }

    #[test]
    fn box_slabs() {
        let h = v(1.0, 2.0, 1.0);
        assert_eq!(ray_box(v(-5.0, 0.0, 0.0), v(1.0, 0.0, 0.0), h), 4.0);
        assert_eq!(ray_box(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), h), 0.0);
        assert_eq!(ray_box(v(-5.0, 3.0, 0.0), v(1.0, 0.0, 0.0), h), -1.0);
        assert_eq!(ray_box(v(5.0, 0.0, 0.0), v(1.0, 0.0, 0.0), h), -1.0);
    }

    #[test]
    fn capsule_body_caps_and_inside() {
        // Height 2, radius 0.5: body from y -0.5 to 0.5, caps above and below.
        assert_eq!(
            ray_capsule(v(-5.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 0.5, 2.0),
            4.5
        );
        assert_eq!(
            ray_capsule(v(0.0, 5.0, 0.0), v(0.0, -1.0, 0.0), 0.5, 2.0),
            4.0
        );
        assert_eq!(
            ray_capsule(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 0.5, 2.0),
            0.0
        );
        assert_eq!(
            ray_capsule(v(-5.0, 2.0, 0.0), v(1.0, 0.0, 0.0), 0.5, 2.0),
            -1.0
        );
    }

    #[test]
    fn scaled_box_reports_world_distance() {
        let mut b = sphere(v(10.0, 0.0, 0.0), 0.0, 0, 1.0);
        b.shape = SHAPE_BOX;
        b.half_extents = v(1.0, 1.0, 1.0);
        b.xform.rows = [v(2.0, 0.0, 0.0), v(0.0, 2.0, 0.0), v(0.0, 0.0, 2.0)];
        let d = hitbox_ray(&b, v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 100.0).unwrap();
        assert!((d - 8.0).abs() < 1e-5, "{d}");
        assert_eq!(
            hitbox_ray(&b, v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 7.0),
            None
        );
        b.enabled = false;
        assert_eq!(
            hitbox_ray(&b, v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 100.0),
            None
        );
    }

    #[test]
    fn set_picks_nearest_then_precedence_then_scale() {
        let from = v(0.0, 0.0, 0.0);
        let dir = v(0.0, 0.0, 1.0);
        let far = sphere(v(0.0, 0.0, 10.0), 1.0, 9, 9.0);
        let near = sphere(v(0.0, 0.0, 5.0), 1.0, 0, 1.0);
        assert_eq!(set_nearest(&[far, near], from, dir, 100.0), Some(1));
        let low = sphere(v(0.0, 0.0, 5.0), 1.0, 0, 4.0);
        let high = sphere(v(0.0, 0.0, 5.0), 1.0, 1, 1.0);
        assert_eq!(set_nearest(&[high, low], from, dir, 100.0), Some(0));
        assert_eq!(set_nearest(&[low, high], from, dir, 100.0), Some(1));
        let more = sphere(v(0.0, 0.0, 5.0), 1.0, 0, 2.0);
        assert_eq!(set_nearest(&[near, more], from, dir, 100.0), Some(1));
        assert_eq!(set_nearest(&[more, near], from, dir, 100.0), Some(0));
        assert_eq!(set_nearest(&[near], from, v(0.0, 1.0, 0.0), 100.0), None);
        assert_eq!(set_nearest(&[], from, dir, 100.0), None);
    }
}
