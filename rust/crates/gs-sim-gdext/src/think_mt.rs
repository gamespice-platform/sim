//! `GSSimBotThinkMt`: the perceive step of every bot's think in one call, the scans and choices on
//! a thread pool (FC `bot_native_think_mt`). Each mind's sight cache and scan are moved out on the
//! main thread into plain data, so nothing on a pool thread touches the engine; a choice that needs
//! a sight answer it does not have stops, the main thread casts that bot's rays (its own query, the
//! same chest-then-head order as `perceive`) and the choice goes on, until every bot has chosen.
//! Each bot's answer is its own and written by its index: the same rows as `perceive_held`, to the
//! bit, whatever the thread count.

use crate::bots::Packed;
use crate::brain::{CLOSE_NOTICE, GSSimBotMind, ask_row, direct_conf, held_far, lod_reuse, v3};
use crate::sight::SightQuery;
use godot::prelude::*;
use gs_sim_bots::choose::{Pick, choose};
use gs_sim_bots::{Found, Query, candidates};
use gs_sim_brain::aim::direction;
use std::collections::HashMap;
use std::sync::Arc;

/// One bot's row in [`GSSimBotThinkMt::perceive_all`]: `perceive_held`'s arguments in order
/// (me_id, me_team, eye x, y, z, yaw, pitch, half_fov_deg, sight, target_id, flags, now, stamp).
pub const STRIDE: usize = 13;

/// Stateless: one per caller is enough.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBotThinkMt {}

#[godot_api]
impl GSSimBotThinkMt {
    /// The row stride of [`Self::perceive_all`].
    #[constant]
    const ROW: i64 = STRIDE as i64;

    /// `perceive_held` for every mind in `minds` (GSSimBotMind, each holding its senses), one
    /// [`STRIDE`] row each in `rows`, `los` the world's sight for all of them, the scans on up to
    /// `threads` threads (0 or 1: this one). Returns [chosen row or -1, dead] per mind, or
    /// [-2, -2] where the mind could not perceive here (nothing held, rows that do not match:
    /// the brain then perceives itself); every row -2 when `rows` is not `minds` long.
    #[func]
    fn perceive_all(
        &self,
        minds: VarArray,
        rows: PackedFloat64Array,
        los: Callable,
        threads: i64,
    ) -> PackedInt32Array {
        let rows = rows.as_slice();
        let n = minds.len();
        let mut out = vec![-2i32; n * 2];
        if rows.len() != n * STRIDE {
            return PackedInt32Array::from(out.as_slice());
        }
        let mut works = Vec::with_capacity(n);
        let mut queries = Vec::with_capacity(n);
        let mut owners = Vec::with_capacity(n);
        for i in 0..n {
            let Ok(mut mind) = minds.at(i).try_to::<Gd<GSSimBotMind>>() else {
                continue;
            };
            let taken = take(&mut mind.bind_mut(), &rows[i * STRIDE..(i + 1) * STRIDE]);
            if let Some((work, query)) = taken {
                works.push(work);
                queries.push(query);
                owners.push((mind, i));
            }
        }
        let direct = direct_conf(&los);
        let threads = threads.max(0) as usize;
        loop {
            each(&mut works, threads, Work::step);
            let mut asked = false;
            for (w, query) in works.iter_mut().zip(queries.iter_mut()) {
                if let Some(Pick::Need(row)) = w.pick {
                    let seen = ask_row(query, &direct, &los, w.eye, &w.held, row);
                    w.sight
                        .insert(w.held.table.ids[row], (w.now, seen, w.stamp));
                    w.pick = None;
                    asked = true;
                }
            }
            if !asked {
                break;
            }
        }
        for ((w, query), (mut mind, i)) in works.into_iter().zip(queries).zip(owners) {
            out[i * 2] = match w.pick {
                Some(Pick::Chosen(row)) => row,
                _ => -1,
            };
            out[i * 2 + 1] = w.found.target_dead as i32;
            let mut m = mind.bind_mut();
            m.sight = w.sight;
            m.found = w.found;
            m.query = query;
        }
        PackedInt32Array::from(out.as_slice())
    }
}

/// One bot's perceive as plain data (sendable to a pool thread).
struct Work {
    held: Arc<Packed>,
    sight: HashMap<i64, (f64, bool, i64)>,
    found: Found,
    query: Query,
    eye: Vector3,
    far: f64,
    far_reuse: f64,
    target_id: i64,
    visible: bool,
    through: bool,
    now: f64,
    stamp: i64,
    scanned: bool,
    pick: Option<Pick>,
}

/// Moves `mind`'s sight cache, scan and query out for row `r`; `None` (the mind untouched)
/// where `perceive_held` would answer nothing.
fn take(mind: &mut GSSimBotMind, r: &[f64]) -> Option<(Work, SightQuery)> {
    let held = mind.held.clone()?;
    let len = held.table.len();
    if held.eyes.len() != len || (mind.far > 0.0 && held.positions.len() != len) {
        return None;
    }
    let eye = Vector3::new(r[2] as f32, r[3] as f32, r[4] as f32);
    let target_id = r[9] as i64;
    let flags = r[10] as i64;
    let visible = flags & 1 != 0;
    let query = Query {
        me_id: r[0] as i64,
        me_team: r[1] as i64 as i32,
        eye: v3(eye),
        forward: direction(r[5], r[6]),
        half_fov_deg: r[7],
        sight: r[8],
        close_notice: CLOSE_NOTICE,
        target_id,
        visible,
    };
    let work = Work {
        held,
        sight: std::mem::take(&mut mind.sight),
        found: std::mem::take(&mut mind.found),
        query,
        eye,
        far: mind.far,
        far_reuse: mind.far_reuse,
        target_id,
        visible,
        through: flags & 2 != 0,
        now: r[11],
        stamp: r[12] as i64,
        scanned: false,
        pick: None,
    };
    Some((work, std::mem::take(&mut mind.query)))
}

impl Work {
    /// Scans once, then chooses as far as the sight cache answers.
    fn step(&mut self) {
        if matches!(self.pick, Some(Pick::Chosen(_))) {
            return;
        }
        let held = &*self.held;
        if !self.scanned {
            candidates(&held.table, &self.query, &mut self.found);
            self.scanned = true;
            // A dead visible target is forgotten before the choice (the script's _forget_target).
            if self.found.target_dead {
                self.visible = false;
                self.target_id = -1;
            }
        }
        let sight = &self.sight;
        let (eye, target_id, now, stamp) = (self.query.eye, self.target_id, self.now, self.stamp);
        let (far, far_reuse) = (self.far, self.far_reuse);
        let pick = choose(
            &mut self.found,
            &held.table,
            target_id,
            self.visible,
            self.through,
            |row| {
                let reuse = lod_reuse(held, row, eye, target_id, far, far_reuse);
                held_far(sight, held.table.ids[row], now, stamp, reuse)
            },
        );
        self.pick = Some(pick);
    }
}

/// `f` over every item, on a pool of `threads` threads (kept between calls) when that is two or
/// more; on this thread otherwise, and always on the web, whose build is single-threaded.
#[cfg(not(target_family = "wasm"))]
fn each<T: Send>(items: &mut [T], threads: usize, f: fn(&mut T)) {
    use rayon::prelude::*;
    use std::sync::Mutex;
    static POOL: Mutex<Option<(usize, Arc<rayon::ThreadPool>)>> = Mutex::new(None);
    if threads < 2 || items.len() < 2 {
        items.iter_mut().for_each(f);
        return;
    }
    let pool = {
        let mut kept = POOL.lock().unwrap_or_else(|e| e.into_inner());
        match &*kept {
            Some((count, pool)) if *count == threads => Some(pool.clone()),
            _ => rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("gs-sim-think-{i}"))
                .build()
                .ok()
                .map(|pool| {
                    let pool = Arc::new(pool);
                    *kept = Some((threads, pool.clone()));
                    pool
                }),
        }
    };
    match pool {
        Some(pool) => pool.install(|| items.par_iter_mut().for_each(f)),
        None => items.iter_mut().for_each(f),
    }
}

#[cfg(target_family = "wasm")]
fn each<T: Send>(items: &mut [T], _threads: usize, f: fn(&mut T)) {
    items.iter_mut().for_each(f);
}
