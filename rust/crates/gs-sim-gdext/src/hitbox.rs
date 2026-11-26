//! `GSSimHitboxRay`: GameSpice Combat's `GSHitboxSet.intersect_ray` loop natively. Stateless
//! (the hitboxes come in each call and are read live, so a stance change that resizes one is
//! seen at once); the script builds the hit from the hitbox it names.

use godot::classes::Node3D;
use godot::prelude::*;
use gs_sim_hitbox::{Hitbox, set_nearest};
use gs_sim_math::{Vec3, Xform};

fn v3(v: Vector3) -> Vec3 {
    Vec3 {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

fn xf(t: &Transform3D) -> Xform {
    Xform {
        rows: [
            v3(t.basis.rows[0]),
            v3(t.basis.rows[1]),
            v3(t.basis.rows[2]),
        ],
        origin: v3(t.origin),
    }
}

/// Which hitbox of a set a ray meets first, by the set's own rules.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimHitboxRay {
    /// enabled, shape, radius, height, half_extents, precedence, damage_scale.
    names: [StringName; 7],
}

#[godot_api]
impl IRefCounted for GSSimHitboxRay {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            names: [
                "enabled",
                "shape",
                "radius",
                "height",
                "half_extents",
                "precedence",
                "damage_scale",
            ]
            .map(StringName::from),
        }
    }
}

fn number(v: &Variant) -> f64 {
    v.try_to::<f64>()
        .ok()
        .or_else(|| v.try_to::<i64>().ok().map(|n| n as f64))
        .unwrap_or(0.0)
}

impl GSSimHitboxRay {
    fn read(&self, node: &Gd<Node3D>) -> Hitbox {
        let [enabled, shape, radius, height, half, precedence, scale] = &self.names;
        Hitbox {
            xform: xf(&node.get_global_transform()),
            shape: node.get(shape).try_to::<i64>().unwrap_or(0),
            radius: number(&node.get(radius)),
            height: number(&node.get(height)),
            half_extents: v3(node.get(half).try_to::<Vector3>().unwrap_or_default()),
            enabled: node.get(enabled).booleanize(),
            precedence: node.get(precedence).try_to::<i64>().unwrap_or(0),
            damage_scale: number(&node.get(scale)),
        }
    }
}

#[godot_api]
impl GSSimHitboxRay {
    /// The index in [param hitboxes] (GSHitbox nodes) of the one whose hit
    /// `GSHitboxSet.intersect_ray` reports for this ray, or -1 for a miss. A freed or
    /// non-3D entry is skipped, as a disabled one is.
    #[func]
    fn nearest(
        &self,
        hitboxes: AnyArray,
        from: Vector3,
        direction: Vector3,
        max_distance: f64,
    ) -> i64 {
        let mut boxes = Vec::with_capacity(hitboxes.len());
        for v in hitboxes.iter_shared() {
            match v.try_to::<Gd<Node3D>>() {
                Ok(node) => boxes.push(self.read(&node)),
                Err(_) => boxes.push(Hitbox {
                    xform: Xform::default(),
                    shape: 0,
                    radius: 0.0,
                    height: 0.0,
                    half_extents: Vec3::default(),
                    enabled: false,
                    precedence: 0,
                    damage_scale: 0.0,
                }),
            }
        }
        set_nearest(&boxes, v3(from), v3(direction), max_distance).map_or(-1, |i| i as i64)
    }
}
