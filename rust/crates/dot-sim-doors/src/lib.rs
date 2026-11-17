//! Murder Mystery's door reach: game-murder-mystery's `MmDoors.door_for` over a table of posed
//! panels, to the bit.
//!
//! Each door is its panel's last posed transform, that transform's `affine_inverse()` (computed
//! by the engine when the door is posed, so it is the script's to the bit by construction) and
//! the panel's box size. The ask is the script's loop: the eye clamped into each panel's box in
//! the panel's frame (`clampf` in `f64`, rounded to `f32` by the `Vector3`), the nearest point
//! back in the world, its distance in `f32` against the `f64` reach, and the sixty degree front
//! test unless within touching distance.

use dot_sim_math::{Vec3, Xform, clampf};

/// Within this distance (metres) a panel is used whatever way somebody looks, as the script's `0.6`.
pub const TOUCH: f64 = 0.6;

/// The front test's cosine, as the script's `0.5` (about sixty degrees).
pub const FRONT: f64 = 0.5;

/// One door's panel where it was last posed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Panel {
    /// The panel body's transform.
    pub frame: Xform,
    /// `frame.affine_inverse()`, as the engine computed it.
    pub inverse: Xform,
    /// The panel's box size.
    pub size: Vec3,
}

/// The nearest point of `panel`'s box to `eyes`, in the world, as the script computes it.
#[inline]
pub fn nearest(panel: &Panel, eyes: Vec3) -> Vec3 {
    let local = panel.inverse.xform(eyes);
    let half = |s: f32| f64::from(s) * 0.5;
    let clamp = |v: f32, s: f32| clampf(f64::from(v), -f64::from(s) * 0.5, half(s)) as f32;
    let s = panel.size;
    panel.frame.xform(Vec3::new(
        clamp(local.x, s.x),
        clamp(local.y, s.y),
        clamp(local.z, s.z),
    ))
}

/// `MmDoors.door_for`: the index of the nearest panel within `reach` of `eyes` and in front of
/// `aim` (or within [`TOUCH`]), the first of equals; `-1` for none.
pub fn door_for(panels: &[Panel], eyes: Vec3, aim: Vec3, reach: f64) -> i64 {
    let mut best = -1i64;
    let mut closest = reach;
    // `aim.normalized()` is the same every pass; the script recomputes the same bits.
    let mut facing: Option<Vec3> = None;
    for (index, panel) in panels.iter().enumerate() {
        let to = nearest(panel, eyes) - eyes;
        let distance = to.length();
        let d = f64::from(distance);
        // The script's `>=`: a NaN distance is not skipped here, nor below, as there.
        if d >= closest {
            continue;
        }
        if d > TOUCH {
            let unit = *facing.get_or_insert_with(|| aim.normalized());
            if f64::from(unit.dot(to.div_scalar(d))) < FRONT {
                continue;
            }
        }
        closest = d;
        best = index as i64;
    }
    best
}

#[cfg(test)]
mod tests;
