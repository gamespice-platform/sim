//! `GSSimBotSenses`: the sight scan of a bot's think, over one packed table per tick.

use godot::prelude::*;
use gs_sim_bots::{Contacts, Found, Grid, Query, Vec3 as V3, candidates_grid};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// One tick's pair sight answers: one tag per (lower row, higher row), shared by every mind that
/// holds the tick, written by whichever asks first (any thread). A tag is `stamp << 2 | 1 + seen`,
/// 0 for not asked yet; a tag under another sight stamp is not an answer.
#[derive(Default, Debug)]
pub struct PairCache {
    n: usize,
    tags: Vec<AtomicU64>,
}

impl PairCache {
    fn new(n: usize) -> Self {
        let mut tags = Vec::with_capacity(n * n);
        tags.resize_with(n * n, || AtomicU64::new(0));
        PairCache { n, tags }
    }

    fn slot(&self, a: usize, b: usize) -> Option<&AtomicU64> {
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        if hi >= self.n {
            return None;
        }
        self.tags.get(lo * self.n + hi)
    }

    /// The answer about rows `a` and `b` under `stamp`, if one was asked this tick.
    pub fn get(&self, a: usize, b: usize, stamp: i64) -> Option<bool> {
        let t = self.slot(a, b)?.load(Ordering::Relaxed);
        (t != 0 && t >> 2 == (stamp as u64) & (u64::MAX >> 2)).then_some(t & 3 == 2)
    }

    /// Keeps the answer about rows `a` and `b` under `stamp`.
    pub fn put(&self, a: usize, b: usize, stamp: i64, seen: bool) {
        if let Some(slot) = self.slot(a, b) {
            slot.store(((stamp as u64) << 2) | (1 + seen as u64), Ordering::Relaxed);
        }
    }
}

/// One tick's packed bodies. Shared by `Arc`: a mind takes a handle on the main thread
/// (`GSSimBotMind.hold_senses`) and reads it on a worker thread without binding the senses
/// object, which several workers must never do at once.
#[derive(Default, Debug, Clone)]
pub struct Packed {
    pub table: Contacts,
    /// The bodies' eyes, row for row with `table` once [`GSSimBotSenses::load_eyes`] gave them.
    pub eyes: Vec<V3>,
    /// The bodies' positions (feet), row for row, for the sight LOD's distance.
    pub positions: Vec<V3>,
    /// The aim points bucketed for the scan (off until [`GSSimBotSenses::set_grid_cell`]).
    pub grid: Grid,
    /// This tick's pair sight answers, fresh with every table or eyes load.
    pub pairs: Arc<PairCache>,
}

impl Packed {
    /// The row of body `id`, if it is in the table.
    pub fn row_of(&self, id: i64) -> Option<usize> {
        self.table.ids.iter().position(|&i| i == id)
    }
}

/// The packed bodies of one tick and the last scan's answer.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBotSenses {
    /// Refilled in place while no mind holds it, else copied on write, so a held tick stays whole.
    pub(crate) packed: Arc<Packed>,
    found: Found,
    rows: Vec<u32>,
    /// The scan grid's cell size in m; 0 scans the whole table.
    grid_cell: f32,
}

impl GSSimBotSenses {
    /// A handle on this tick's bodies for a mind to read later, on any thread.
    pub(crate) fn share(&self) -> Arc<Packed> {
        Arc::clone(&self.packed)
    }
}

#[godot_api]
impl GSSimBotSenses {
    /// Replaces the table: ids, aim points, teams and alive flags, one row per body.
    #[func]
    fn load_contacts(
        &mut self,
        ids: PackedInt64Array,
        points: PackedVector3Array,
        teams: PackedInt32Array,
        alive: PackedByteArray,
    ) {
        let n = ids
            .len()
            .min(points.len())
            .min(teams.len())
            .min(alive.len());
        let pk = Arc::make_mut(&mut self.packed);
        pk.eyes.clear();
        pk.positions.clear();
        let t = &mut pk.table;
        t.ids.clear();
        t.points.clear();
        t.teams.clear();
        t.alive.clear();
        t.ids.extend_from_slice(&ids.as_slice()[..n]);
        t.points.extend(
            points.as_slice()[..n]
                .iter()
                .map(|p| V3::new(p.x, p.y, p.z)),
        );
        t.teams.extend_from_slice(&teams.as_slice()[..n]);
        t.alive
            .extend(alive.as_slice()[..n].iter().map(|a| *a != 0));
        pk.grid.build(&pk.table.points, self.grid_cell);
        pk.pairs = Arc::new(PairCache::new(n));
    }

    /// The scan grid's cell size in m from the next [`Self::load_contacts`] on; 0 (the default)
    /// scans the whole table. The answer is the same either way.
    #[func]
    fn set_grid_cell(&mut self, cell: f64) {
        self.grid_cell = cell.max(0.0) as f32;
    }

    /// The bodies' eyes, in the rows [`Self::load_contacts`] was given; the brain's native perceive
    /// tests sight to them.
    #[func]
    fn load_eyes(&mut self, eyes: PackedVector3Array) {
        let pk = Arc::make_mut(&mut self.packed);
        pk.eyes.clear();
        pk.eyes
            .extend(eyes.as_slice().iter().map(|p| V3::new(p.x, p.y, p.z)));
        pk.pairs = Arc::new(PairCache::new(pk.table.len()));
    }

    /// The bodies' positions, in the rows [`Self::load_contacts`] was given; the native perceive
    /// measures the sight LOD's distance to them.
    #[func]
    fn load_positions(&mut self, positions: PackedVector3Array) {
        let pk = Arc::make_mut(&mut self.packed);
        pk.positions.clear();
        pk.positions
            .extend(positions.as_slice().iter().map(|p| V3::new(p.x, p.y, p.z)));
    }

    /// Indices of the bodies inside range and cone (or still tracked), in table order.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn candidates(
        &mut self,
        me_id: i64,
        me_team: i64,
        eye: Vector3,
        forward: Vector3,
        half_fov_deg: f64,
        sight: f64,
        close_notice: f64,
        target_id: i64,
        visible: bool,
    ) -> PackedInt32Array {
        let q = Query {
            me_id,
            me_team: me_team as i32,
            eye: V3::new(eye.x, eye.y, eye.z),
            forward: V3::new(forward.x, forward.y, forward.z),
            half_fov_deg,
            sight,
            close_notice,
            target_id,
            visible,
        };
        let pk = &*self.packed;
        candidates_grid(&pk.table, &pk.grid, &q, &mut self.found, &mut self.rows);
        PackedInt32Array::from(self.found.index.as_slice())
    }

    /// The last scan's scores, one per index it returned.
    #[func]
    fn scores(&self) -> PackedFloat32Array {
        PackedFloat32Array::from(self.found.scores.as_slice())
    }

    /// Whether the last scan found its visible target dead.
    #[func]
    fn target_dead(&self) -> bool {
        self.found.target_dead
    }
}
