//! `GSSimHitboxRay`: GameSpice Combat's `GSHitboxSet.intersect_ray` loop natively. Stateless
//! (the hitboxes come in each call and are read live, so a stance change that resizes one is
//! seen at once); the script builds the hit from the hitbox it names.

use godot::classes::Node3D;
use godot::prelude::*;
use gs_sim_hitbox::{Hitbox, SetBounds, set_nearest, sets_nearest};
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
    /// A set's bounds_offset, bounds_radius, enabled, hitboxes() and entity_owner().
    set_names: [StringName; 5],
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
            set_names: [
                "bounds_offset",
                "bounds_radius",
                "enabled",
                "hitboxes",
                "entity_owner",
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

fn dead() -> Hitbox {
    Hitbox {
        xform: Xform::default(),
        shape: 0,
        radius: 0.0,
        height: 0.0,
        half_extents: Vec3::default(),
        enabled: false,
        precedence: 0,
        damage_scale: 0.0,
    }
}

fn object_id(v: &Variant) -> Option<InstanceId> {
    v.try_to::<Gd<Object>>().ok().map(|o| o.instance_id())
}

impl GSSimHitboxRay {
    fn read_all(&self, hitboxes: &AnyArray) -> Vec<Hitbox> {
        hitboxes
            .iter_shared()
            .map(|v| match v.try_to::<Gd<Node3D>>() {
                Ok(node) => self.read(&node),
                Err(_) => dead(),
            })
            .collect()
    }

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
        let boxes = self.read_all(&hitboxes);
        set_nearest(&boxes, v3(from), v3(direction), max_distance).map_or(-1, |i| i as i64)
    }

    /// `GSTrace.ray`'s walk over [param sets] (GSHitboxSet nodes) after its world ray: the
    /// `(set, hitbox)` indices of the hit it keeps, or `(-1, -1)` when the world hit (or nothing)
    /// stands. [param world_distance] is the world hit's distance, negative for none. Same rules
    /// as the script: each set's bounds sphere against the current limit, a disabled or empty set
    /// or one in [param exclude] (the set or its `entity_owner()`) skipped, the set's own pick,
    /// a hit strictly nearer than the best so far, the limit shortened by each kept hit. One
    /// call per ray; a set's hitboxes are read only when the ray reaches its bounds. The script
    /// builds the hit from the named hitbox.
    #[func]
    fn nearest_in_sets(
        &self,
        sets: AnyArray,
        exclude: AnyArray,
        from: Vector3,
        direction: Vector3,
        max_distance: f64,
        world_distance: f64,
    ) -> Vector2i {
        let [offset, radius, enabled, hitboxes, owner] = &self.set_names;
        let nodes: Vec<Option<Gd<Node3D>>> = sets
            .iter_shared()
            .map(|v| v.try_to::<Gd<Node3D>>().ok())
            .collect();
        let bounds: Vec<Option<SetBounds>> = nodes
            .iter()
            .map(|n| {
                n.as_ref().map(|n| SetBounds {
                    centre: xf(&n.get_global_transform())
                        .xform(v3(n.get(offset).try_to::<Vector3>().unwrap_or_default())),
                    radius: number(&n.get(radius)),
                })
            })
            .collect();
        let excluded: Vec<InstanceId> = exclude
            .iter_shared()
            .filter_map(|v| object_id(&v))
            .collect();
        let candidates = |s: usize| {
            let mut node = nodes[s].clone()?;
            if !node.get(enabled).booleanize() {
                return None;
            }
            let boxes = node.call(hitboxes, &[]).try_to::<AnyArray>().ok()?;
            if boxes.is_empty() {
                return None;
            }
            if !excluded.is_empty() {
                let entity = object_id(&node.call(owner, &[]));
                let own = node.instance_id();
                if excluded.iter().any(|&e| e == own || Some(e) == entity) {
                    return None;
                }
            }
            Some(self.read_all(&boxes))
        };
        let world = (world_distance >= 0.0).then_some(world_distance);
        match sets_nearest(
            &bounds,
            candidates,
            v3(from),
            v3(direction),
            max_distance,
            world,
        ) {
            Some((s, i, _)) => Vector2i::new(s as i32, i as i32),
            None => Vector2i::new(-1, -1),
        }
    }
}
