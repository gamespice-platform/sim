//! `GSSimShots`: one hitscan pellet's walk through the world and the hitbox sets, gs-combat's
//! `GSPelletTracer.trace_pellet` (with `GSTrace.ray` and the `GSHitbox` shapes). See
//! [`gs_sim_weapon::bullet`] for the rules; this class holds the hitbox cache and moves rows.
//!
//! The caller keeps one per combat manager:
//! - [method set_boxes] whenever the set list or its boxes change (rare),
//! - [method set_poses] before a shot (one row per set: where its root is, how crouched),
//! - [method set_world] once: the world ray, a `Callable(from, dir, max) -> PackedFloat64Array`
//!   answering `[distance, point xyz, normal xyz, material, resistance, ricochet, sheet, pane]`
//!   or an empty array for a miss,
//! - [method set_pen] per shot (or [method clear_pen] for a round that cannot penetrate),
//! - [method walk] per pellet: event rows of [constant STRIDE] floats.
//!
//! Event rows: `[kind, pellet, ...]`, kind 1 impact `[point xyz]`, 2 body `[point xyz, normal xyz,
//! set, box, distance, strength, passes, walls, bounces]`, 3 pass `[entry xyz, exit xyz, entry
//! normal xyz, exit normal xyz, material, thickness, cost, sheet]`, 4 ricochet `[point xyz, normal
//! xyz, direction xyz, _, _, _, material, angle]`, 9 script (a pane: the script traces this pellet).

use godot::prelude::*;
use gs_sim_weapon::bullet::{
    BoxInfo, EVENT_STRIDE, Event, HitSet, Hitbox, Pen, Shape, V3, WorldHit, WorldTrace, Xform, walk,
};

/// Floats per [method set_boxes] set row.
const SET_STRIDE: usize = 20;
/// Floats per [method set_boxes] box row.
const BOX_STRIDE: usize = 24;
/// Floats per [method set_poses] row.
const POSE_STRIDE: usize = 14;

/// One box as authored: its local transform below the set, and the stand numbers a crouch fit scales.
#[derive(Clone, Copy)]
struct BoxRest {
    local: Xform,
    shape: Shape,
    radius: f64,
    height: f64,
    half: V3,
    damage_scale: f64,
    precedence: i64,
    /// `stand_y` and `stand_h` metas; NaN when the box is never fitted.
    stand_y: f64,
    stand_h: f64,
}

#[derive(Clone)]
struct SetRest {
    entity: i64,
    /// The set's local transform below its root.
    local: Xform,
    bounds_offset: V3,
    bounds_radius: f64,
    boxes: Vec<BoxRest>,
}

struct Callback(Option<Callable>);

impl WorldTrace for Callback {
    fn ray(&mut self, from: V3, dir: V3, max: f64) -> Option<WorldHit> {
        let c = self.0.as_ref()?;
        let from = Vector3::new(from[0], from[1], from[2]);
        let dir = Vector3::new(dir[0], dir[1], dir[2]);
        let got = c.call(&[from.to_variant(), dir.to_variant(), max.to_variant()]);
        let row = got.try_to::<PackedFloat64Array>().ok()?;
        let r = row.as_slice();
        if r.len() < 12 {
            return None;
        }
        Some(WorldHit {
            distance: r[0],
            point: [r[1] as f32, r[2] as f32, r[3] as f32],
            normal: [r[4] as f32, r[5] as f32, r[6] as f32],
            material: r[7] as i64,
            resistance: r[8],
            ricochet: r[9] != 0.0,
            sheet: r[10],
            pane: r[11] != 0.0,
        })
    }
}

fn xform_at(r: &[f64]) -> Xform {
    let f = |i: usize| r[i] as f32;
    Xform {
        b: [[f(0), f(1), f(2)], [f(3), f(4), f(5)], [f(6), f(7), f(8)]],
        o: [f(9), f(10), f(11)],
    }
}

/// The hitscan pellet walk over cached hitbox sets.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimShots {
    rest: Vec<SetRest>,
    sets: Vec<HitSet>,
    info: Vec<Vec<BoxInfo>>,
    world: Callback,
    pen: Option<Pen>,
    skip: Vec<bool>,
    events: Vec<Event>,
}

#[godot_api]
impl IRefCounted for GSSimShots {
    fn init(_base: Base<RefCounted>) -> Self {
        GSSimShots {
            rest: Vec::new(),
            sets: Vec::new(),
            info: Vec::new(),
            world: Callback(None),
            pen: None,
            skip: Vec::new(),
            events: Vec::new(),
        }
    }
}

#[godot_api]
impl GSSimShots {
    /// Floats per event row.
    #[constant]
    const STRIDE: i64 = EVENT_STRIDE as i64;

    /// The sets, in the trace's set order. Set rows: `[entity, box count, bounds offset xyz, bounds
    /// radius, _, local basis rows 9, local origin 3, _]`. Box rows, set by set: `[shape, radius,
    /// height, half extents xyz, damage scale, precedence, group index, stand_y, stand_h, local basis
    /// rows 9, local origin 3, _]` (stand_y and stand_h NaN for a box never fitted). False when the
    /// rows do not add up (the cache is then empty).
    #[func]
    fn set_boxes(&mut self, sets: PackedFloat64Array, boxes: PackedFloat64Array) -> bool {
        self.rest.clear();
        self.info.clear();
        self.sets.clear();
        let (s, b) = (sets.as_slice(), boxes.as_slice());
        if !s.len().is_multiple_of(SET_STRIDE) || !b.len().is_multiple_of(BOX_STRIDE) {
            return false;
        }
        let mut at = 0usize;
        for r in s.as_chunks::<SET_STRIDE>().0 {
            let count = r[1].max(0.0) as usize;
            if (at + count) * BOX_STRIDE > b.len() {
                self.rest.clear();
                self.info.clear();
                return false;
            }
            let mut rest = SetRest {
                entity: r[0] as i64,
                local: xform_at(&r[7..19]),
                bounds_offset: [r[2] as f32, r[3] as f32, r[4] as f32],
                bounds_radius: r[5],
                boxes: Vec::with_capacity(count),
            };
            let mut info = Vec::with_capacity(count);
            for x in b[at * BOX_STRIDE..(at + count) * BOX_STRIDE]
                .as_chunks::<BOX_STRIDE>()
                .0
            {
                rest.boxes.push(BoxRest {
                    local: xform_at(&x[11..23]),
                    shape: Shape::from_code(x[0] as i64),
                    radius: x[1],
                    height: x[2],
                    half: [x[3] as f32, x[4] as f32, x[5] as f32],
                    damage_scale: x[6],
                    precedence: x[7] as i64,
                    stand_y: x[9],
                    stand_h: x[10],
                });
                info.push(BoxInfo {
                    group: x[8].max(0.0) as usize,
                });
            }
            at += count;
            self.rest.push(rest);
            self.info.push(info);
        }
        if at * BOX_STRIDE != b.len() {
            self.rest.clear();
            self.info.clear();
            return false;
        }
        true
    }

    /// Where every set is for the coming shot, one row per set in [method set_boxes] order:
    /// `[enabled, root basis rows 9, root origin 3, k]`, `k` the crouch fit's height factor
    /// (`fit_hitboxes_to_crouch`), below 0 for the boxes as cached. False when the rows do not match.
    #[func]
    fn set_poses(&mut self, poses: PackedFloat64Array) -> bool {
        let p = poses.as_slice();
        if p.len() != self.rest.len() * POSE_STRIDE {
            return false;
        }
        self.sets.resize_with(self.rest.len(), HitSet::default);
        for ((r, rest), set) in p
            .as_chunks::<POSE_STRIDE>()
            .0
            .iter()
            .zip(&self.rest)
            .zip(&mut self.sets)
        {
            let root = xform_at(&r[1..13]);
            let k = r[13];
            let set_global = root.mul(&rest.local);
            set.entity = rest.entity;
            set.enabled = r[0] != 0.0;
            set.centre = set_global.xform(rest.bounds_offset);
            set.radius = rest.bounds_radius;
            set.boxes.clear();
            for b in &rest.boxes {
                let mut local = b.local;
                let mut height = b.height;
                if k >= 0.0 && !b.stand_y.is_nan() {
                    local.o[1] = (b.stand_y * k) as f32;
                    if b.shape == Shape::Capsule && !b.stand_h.is_nan() {
                        height = (b.stand_h * k).max(b.radius * 2.0 + 0.02);
                    }
                }
                let mut hb = Hitbox::new(set_global.mul(&local), b.shape, b.radius, height, b.half);
                hb.damage_scale = b.damage_scale;
                hb.precedence = b.precedence;
                set.boxes.push(hb);
            }
        }
        true
    }

    /// The world ray; see the class notes for its answer.
    #[func]
    fn set_world(&mut self, ray: Callable) {
        self.world = Callback(ray.is_valid().then_some(ray));
    }

    /// The round's (already scaled) penetration: `[power, max_depth, body_cost, speed,
    /// reference_speed, min_damage, max_passes, ricochet_angle, ricochet_keep, max_ricochets,
    /// ricochet_enabled, penetration_players]`, and the body cost multiplier per group index.
    #[func]
    fn set_pen(&mut self, params: PackedFloat64Array, zones: PackedFloat64Array) -> bool {
        let p = params.as_slice();
        if p.len() < 12 {
            self.pen = None;
            return false;
        }
        self.pen = Some(Pen {
            power: p[0],
            max_depth: p[1],
            body_cost: p[2],
            speed: p[3],
            reference_speed: p[4],
            min_damage: p[5],
            max_passes: p[6] as i64,
            ricochet_angle: p[7],
            ricochet_keep: p[8],
            max_ricochets: p[9] as i64,
            ricochet_enabled: p[10] != 0.0,
            players: p[11] != 0.0,
            zones: zones.as_slice().to_vec(),
        });
        true
    }

    /// A round that cannot penetrate.
    #[func]
    fn clear_pen(&mut self) {
        self.pen = None;
    }

    /// Pellet [param pellet] from [param origin] along [param direction] for [param max_range],
    /// never hitting the sets at [param skip] (indices in [method set_boxes] order). Event rows.
    #[func]
    fn walk(
        &mut self,
        pellet: i64,
        origin: Vector3,
        direction: Vector3,
        max_range: f64,
        skip: PackedInt32Array,
    ) -> PackedFloat64Array {
        self.skip.clear();
        self.skip.resize(self.sets.len(), false);
        for &i in skip.as_slice() {
            if let Some(s) = self.skip.get_mut(i as usize) {
                *s = true;
            }
        }
        self.events.clear();
        walk(
            &mut self.world,
            &self.sets,
            &self.info,
            &mut self.skip,
            self.pen.as_ref(),
            [origin.x, origin.y, origin.z],
            [direction.x, direction.y, direction.z],
            max_range,
            &mut self.events,
        );
        let mut rows = Vec::with_capacity(self.events.len() * EVENT_STRIDE);
        for e in &self.events {
            e.write(pellet.max(0) as usize, &mut rows);
        }
        PackedFloat64Array::from(rows.as_slice())
    }

    /// How many sets the cache holds.
    #[func]
    fn set_count(&self) -> i64 {
        self.rest.len() as i64
    }
}
