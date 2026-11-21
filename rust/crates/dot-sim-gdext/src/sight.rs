//! `GSSimSightRay`: a world's bot line of sight as one native ray query. A game binds it to its
//! physics space once a tick and hands its `clear` to GSBotBrain as the sight callable; a
//! [`crate::brain::GSSimBotMind`] that is handed it asks the ray directly, with no call into
//! script. The answer is the script's `PhysicsRayQueryParameters3D` ray to the bit: the same end
//! point (`from + offset / distance * distance` in `f32`), mask, exclusions and flags. A bound jail
//! cell adds dot_map's `GSMapGlassSight.blocks` in front: a ray starting inside the cell asks the
//! glass layer first and is blocked by glass that lies within the cell's reach.

// The Arc is shared by the brains thinking on worker threads: godot-rust (experimental-threads)
// leaves `Gd` neither Send nor Sync, but the space's queries are thread-safe under the game's lock,
// the config is read-only once bound, and `bind` only runs on the main thread between ticks.
#![allow(clippy::arc_with_non_send_sync)]

use godot::classes::{PhysicsDirectSpaceState3D, PhysicsRayQueryParameters3D};
use godot::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Every bind's epoch, unique across rays, so a query that has asked another ray is never taken
/// for bound to this one.
static EPOCH: AtomicU64 = AtomicU64::new(1);

/// What the ray asks of, as bound for the tick. Shared by every mind asking this tick.
pub struct SightConf {
    space: Option<Gd<PhysicsDirectSpaceState3D>>,
    mask: u32,
    exclude: Array<Rid>,
    /// The game's lock around its space's queries (GodotPhysics3D answers out of buffers the
    /// space shares), held around each ray; none: the caller keeps to one thread. A Godot `Mutex`
    /// (godot-rust leaves the thread classes out, so it is called by name).
    lock: Option<Gd<RefCounted>>,
    /// New at every bind (unique across rays), so a reused query knows its mask and exclusions are stale.
    epoch: u64,
    /// The jail cell's glass rule, when one is in force.
    cell: Option<Cell>,
}

/// `GSMapGlassSight` as bound: a ray from within `radius` of `centre` that meets `glass_mask`
/// within `reach` (radius plus margin) of the centre is blocked. The radii stay `f64`, as script
/// compares an `f32` distance to them.
struct Cell {
    space: Gd<PhysicsDirectSpaceState3D>,
    glass_mask: u32,
    centre: Vector3,
    radius: f64,
    reach: f64,
}

fn dist(a: Vector3, b: Vector3) -> f32 {
    let d = b - a;
    (d.x * d.x + d.y * d.y + d.z * d.z).sqrt()
}

impl SightConf {
    /// Whether nothing blocks `from` to `to`: true for a ray of 0.0001 m or less, or with no space.
    pub fn clear(&self, query: &mut SightQuery, from: Vector3, to: Vector3) -> bool {
        if let Some(cell) = &self.cell
            && (dist(from, cell.centre) as f64) < cell.radius
            && self.glass_blocks(cell, query, from, to)
        {
            return false;
        }
        let offset = to - from;
        let distance = (offset.x * offset.x + offset.y * offset.y + offset.z * offset.z).sqrt();
        if (distance as f64) <= 0.0001 {
            return true;
        }
        self.ray(query, from, from + offset / distance * distance)
    }

    /// Whether nothing blocks `from` to `to` as given: script's
    /// `intersect_ray(PhysicsRayQueryParameters3D.create(from, to)).is_empty()` with the bound mask
    /// and exclusions, to the bit (no short-ray answer, no recomputed end point). True with no space.
    pub fn clear_to(&self, query: &mut SightQuery, from: Vector3, to: Vector3) -> bool {
        if let Some(cell) = &self.cell
            && (dist(from, cell.centre) as f64) < cell.radius
            && self.glass_blocks(cell, query, from, to)
        {
            return false;
        }
        self.ray(query, from, to)
    }

    /// The bound ray from `from` to `to`, on the reused query.
    fn ray(&self, query: &mut SightQuery, from: Vector3, to: Vector3) -> bool {
        let Some(space) = &self.space else {
            return true;
        };
        let ray = query.ray.get_or_insert_with(|| {
            let mut q = PhysicsRayQueryParameters3D::new_gd();
            q.set_collide_with_areas(false);
            q.set_collide_with_bodies(true);
            q.set_hit_from_inside(false);
            q
        });
        if query.epoch != Some(self.epoch) {
            ray.set_collision_mask(self.mask);
            ray.set_exclude(&self.exclude);
            query.epoch = Some(self.epoch);
        }
        ray.set_from(from);
        ray.set_to(to);
        let mut space = space.clone();
        self.locked(|| space.intersect_ray(&*ray).is_empty())
    }

    /// `GSMapGlassSight.blocks` for a ray starting inside the cell: the glass layer alone, no
    /// exclusions, from `from` to `to` as given.
    fn glass_blocks(
        &self,
        cell: &Cell,
        query: &mut SightQuery,
        from: Vector3,
        to: Vector3,
    ) -> bool {
        let ray = query.glass.get_or_insert_with(|| {
            let mut q = PhysicsRayQueryParameters3D::new_gd();
            q.set_collide_with_areas(false);
            q.set_collide_with_bodies(true);
            q.set_hit_from_inside(false);
            q
        });
        ray.set_collision_mask(cell.glass_mask);
        ray.set_from(from);
        ray.set_to(to);
        let mut space = cell.space.clone();
        let hit = self.locked(|| space.intersect_ray(&*ray));
        match hit.get("position").and_then(|v| v.try_to::<Vector3>().ok()) {
            Some(at) => (dist(at, cell.centre) as f64) < cell.reach,
            None => false,
        }
    }

    /// `f` with the game's lock held, when it gave one.
    fn locked<R>(&self, f: impl FnOnce() -> R) -> R {
        match &self.lock {
            Some(lock) => {
                let mut lock = lock.clone();
                lock.call("lock", &[]);
                let out = f();
                lock.call("unlock", &[]);
                out
            }
            None => f(),
        }
    }
}

/// One asker's reusable query (one thread at a time), with the bind it was last set for.
#[derive(Default)]
pub struct SightQuery {
    ray: Option<Gd<PhysicsRayQueryParameters3D>>,
    /// The jail glass ray, made on the first ray from inside a cell.
    glass: Option<Gd<PhysicsRayQueryParameters3D>>,
    epoch: Option<u64>,
}

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct GSSimSightRay {
    conf: Arc<SightConf>,
    /// The query `clear` uses when script (or a callable) asks.
    query: std::sync::Mutex<SightQuery>,
}

#[godot_api]
impl IRefCounted for GSSimSightRay {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            conf: Arc::new(SightConf {
                space: None,
                mask: 0,
                exclude: Array::new(),
                lock: None,
                epoch: 0,
                cell: None,
            }),
            query: std::sync::Mutex::new(SightQuery::default()),
        }
    }
}

#[godot_api]
impl GSSimSightRay {
    /// Binds the ray to `space` with collision `mask` and the RIDs to `exclude`; `lock` (a
    /// `Mutex`, or null) is held around each query. A non-zero `glass_mask` with a `glass_space`
    /// puts the jail cell rule in front (`centre`, `radius`, and `reach` = radius + margin, as
    /// `GSMapGlassSight` has them). Call it on the main thread, before any brain asks this tick.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn bind(
        &mut self,
        space: Option<Gd<PhysicsDirectSpaceState3D>>,
        mask: i64,
        exclude: Array<Rid>,
        lock: Option<Gd<RefCounted>>,
        glass_space: Option<Gd<PhysicsDirectSpaceState3D>>,
        glass_mask: i64,
        centre: Vector3,
        radius: f64,
        reach: f64,
    ) {
        let epoch = EPOCH.fetch_add(1, Ordering::Relaxed);
        let cell = glass_space.filter(|_| glass_mask != 0).map(|space| Cell {
            space,
            glass_mask: glass_mask as u32,
            centre,
            radius,
            reach,
        });
        self.conf = Arc::new(SightConf {
            space,
            mask: mask as u32,
            exclude,
            lock,
            epoch,
            cell,
        });
    }

    /// Whether nothing blocks the line from `from` to `to` (the world's `line_of_sight`).
    #[func]
    fn clear(&self, from: Vector3, to: Vector3) -> bool {
        let mut query = self.query.lock().unwrap_or_else(|e| e.into_inner());
        self.conf.clear(&mut query, from, to)
    }

    /// Whether nothing blocks the line from `from` to `to` exactly as script's ray asks it: the
    /// end point as given and no short-ray answer (a game's `_clear_between`, to the bit).
    #[func]
    fn clear_to(&self, from: Vector3, to: Vector3) -> bool {
        let mut query = self.query.lock().unwrap_or_else(|e| e.into_inner());
        self.conf.clear_to(&mut query, from, to)
    }
}

impl GSSimSightRay {
    /// The bound configuration, for a mind that asks it directly.
    pub fn conf(&self) -> Arc<SightConf> {
        self.conf.clone()
    }
}
