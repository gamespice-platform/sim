//! The spawner's think pacing, held natively: which NPCs think this tick and with how much time.
//!
//! A paced brain (dot-npc `GSNpcBrain.paces`) thinks once every `every` ticks and is handed the time it
//! owes (the ticks' deltas summed in order). Script side that is three fields on each brain and a call into
//! every brain every tick, most of which only count; here the counting is one batch per tick and the
//! spawner calls only the brains that are due.
//!
//! Rows keep the spawner's NPC order (insertion order; a removal keeps the rest in place), and a tick runs
//! in three steps, mirroring the script exactly:
//!
//! 1. [`Pacer::advance`] reads which rows are due without changing any: an unpaced row always is (its
//!    brain does its own counting), a paced one when `since + 1 >= every`, owing `owed + delta`.
//! 2. The spawner thinks those, in order, collecting each thought row's new `every`.
//! 3. [`Pacer::settle`] commits the tick: a thought row restarts (`since 0, owed 0, every` new), every
//!    other paced row counts (`since + 1, owed + delta`).
//!
//! A brain that changes a row while it thinks (a stagger it causes, a possession) has the spawner first call
//! [`Pacer::commit_to`] with itself and the rows thought so far: the rows before it are committed as the
//! script holds them by then, it and the rest keep this tick's start, the change lands, and
//! [`Pacer::due_after`] reads the rest of the tick's due rows again. A row added after `advance` sits the
//! tick out (the spawner's loop walks a copy of its keys taken before any brain thinks), and one removed
//! during a tick stays as a gap until [`Pacer::settle`] so the order around it holds.

use std::collections::HashMap;

/// One NPC's pacing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    /// The NPC's instance id.
    pub id: i64,
    /// Ticks since its last thought (`_since_thought`).
    pub since: i64,
    /// Seconds owed since its last thought (`_owed_delta`).
    pub owed: f64,
    /// Think every this many ticks (`_every`).
    pub every: i64,
    /// Whether the pacer counts for it; an unpaced row is due every tick and its brain counts.
    pub paced: bool,
    /// Added after this tick's [`Pacer::advance`]: sits the tick out.
    fresh: bool,
    /// Already committed this tick by [`Pacer::commit_to`].
    done: bool,
    /// Removed during the tick; dropped at [`Pacer::settle`].
    gone: bool,
}

/// A row's counters, as a brain holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Counters {
    pub since: i64,
    pub owed: f64,
    pub every: i64,
}

impl Default for Counters {
    fn default() -> Self {
        Self {
            since: 0,
            owed: 0.0,
            every: 1,
        }
    }
}

impl From<&Row> for Counters {
    fn from(r: &Row) -> Self {
        Self {
            since: r.since,
            owed: r.owed,
            every: r.every,
        }
    }
}

/// The rows, in the spawner's order.
#[derive(Clone, Debug, Default)]
pub struct Pacer {
    rows: Vec<Row>,
    at: HashMap<i64, usize>,
    gone: usize,
    delta: f64,
    looping: bool,
}

impl Pacer {
    pub fn new() -> Self {
        Self::default()
    }

    /// The NPCs with a row.
    pub fn len(&self) -> usize {
        self.rows.len() - self.gone
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.at.clear();
        self.gone = 0;
        self.looping = false;
    }

    fn live(&self, id: i64) -> Option<usize> {
        self.at.get(&id).copied().filter(|i| !self.rows[*i].gone)
    }

    /// The row of `id`, if it has one.
    pub fn row(&self, id: i64) -> Option<&Row> {
        self.live(id).map(|i| &self.rows[i])
    }

    fn reindex(&mut self, from: usize) {
        for (j, row) in self.rows.iter().enumerate().skip(from) {
            self.at.insert(row.id, j);
        }
    }

    /// Appends `id`, unpaced. Returns false, changing nothing, when it already has a row.
    pub fn add(&mut self, id: i64) -> bool {
        if let Some(&i) = self.at.get(&id) {
            if !self.rows[i].gone {
                return false;
            }
            self.rows.remove(i);
            self.gone -= 1;
            self.reindex(i);
        }
        self.at.insert(id, self.rows.len());
        self.rows.push(Row {
            id,
            since: 0,
            owed: 0.0,
            every: 1,
            paced: false,
            fresh: self.looping,
            done: false,
            gone: false,
        });
        true
    }

    /// Drops `id`'s row, keeping the rest in order. Returns its counters, or None when it had none.
    pub fn remove(&mut self, id: i64) -> Option<Counters> {
        let i = self.live(id)?;
        let c = Counters::from(&self.rows[i]);
        if self.looping {
            self.rows[i].gone = true;
            self.gone += 1;
        } else {
            self.at.remove(&id);
            self.rows.remove(i);
            self.reindex(i);
        }
        Some(c)
    }

    /// Turns pacing on (taking the brain's counters `c`) or off. Returns the counters the row held, for the
    /// brain to take back when pacing goes off; None when `id` has no row.
    pub fn set_paced(&mut self, id: i64, on: bool, c: Counters) -> Option<Counters> {
        let i = self.live(id)?;
        let r = &mut self.rows[i];
        let old = Counters::from(&*r);
        r.paced = on;
        if on {
            r.since = c.since;
            r.owed = c.owed;
            r.every = c.every;
        }
        Some(old)
    }

    /// Sets `id`'s `every` (a brain's `_every = n` outside its think). Returns false when it has no row.
    pub fn set_every(&mut self, id: i64, every: i64) -> bool {
        match self.live(id) {
            Some(i) => {
                self.rows[i].every = every;
                true
            }
            None => false,
        }
    }

    fn due(r: &Row, delta: f64) -> Option<f64> {
        if r.fresh || r.done || r.gone {
            None
        } else if !r.paced {
            Some(-1.0)
        } else if r.since + 1 >= r.every {
            Some(r.owed + delta)
        } else {
            None
        }
    }

    /// Starts a tick of `delta` seconds: the due rows in order, each with the seconds it owes
    /// (-1 for an unpaced row, whose brain is handed `delta` and counts for itself). A tick still open is
    /// settled first, with nothing thought.
    pub fn advance(&mut self, delta: f64, ids: &mut Vec<i64>, owed: &mut Vec<f64>) {
        if self.looping {
            self.settle(&[], &[]);
        }
        self.delta = delta;
        self.looping = true;
        ids.clear();
        owed.clear();
        for r in &self.rows {
            if let Some(o) = Self::due(r, delta) {
                ids.push(r.id);
                owed.push(o);
            }
        }
    }

    fn commit(r: &mut Row, delta: f64, thought: Option<i64>) {
        if r.fresh || r.done || r.gone {
            return;
        }
        r.done = true;
        if !r.paced {
            return;
        }
        match thought {
            Some(every) => {
                r.since = 0;
                r.owed = 0.0;
                r.every = every;
            }
            None => {
                r.since += 1;
                r.owed += delta;
            }
        }
    }

    fn thought_map(thought: &[i64], every: &[i64]) -> HashMap<i64, i64> {
        thought.iter().copied().zip(every.iter().copied()).collect()
    }

    /// Commits the rows before `id` (through it when `through`; all of them when `id` has no row) with the
    /// rows thought since the last commit and their new `every`, as the script holds them partway through
    /// its loop. The rest keep this tick's start. Returns false outside a tick.
    pub fn commit_to(&mut self, id: i64, through: bool, thought: &[i64], every: &[i64]) -> bool {
        if !self.looping {
            return false;
        }
        let end = self
            .at
            .get(&id)
            .map_or(self.rows.len(), |i| i + usize::from(through));
        let map = Self::thought_map(thought, every);
        let delta = self.delta;
        for r in &mut self.rows[..end] {
            let t = map.get(&r.id).copied();
            Self::commit(r, delta, t);
        }
        true
    }

    /// The rows after `id` (all rows when `id` has none) still due this tick, in order, as
    /// [`Pacer::advance`] gives them.
    pub fn due_after(&self, id: i64, ids: &mut Vec<i64>, owed: &mut Vec<f64>) {
        ids.clear();
        owed.clear();
        if !self.looping {
            return;
        }
        let start = self.at.get(&id).map_or(0, |i| i + 1);
        for r in &self.rows[start..] {
            if let Some(o) = Self::due(r, self.delta) {
                ids.push(r.id);
                owed.push(o);
            }
        }
    }

    /// Ends the tick: the rows not committed yet are, with `thought` (and their new `every`), and the rows
    /// removed during it are dropped. Returns false outside a tick.
    pub fn settle(&mut self, thought: &[i64], every: &[i64]) -> bool {
        if !self.looping {
            return false;
        }
        let map = Self::thought_map(thought, every);
        let delta = self.delta;
        for r in &mut self.rows {
            let t = map.get(&r.id).copied();
            Self::commit(r, delta, t);
            r.fresh = false;
            r.done = false;
        }
        if self.gone > 0 {
            for r in self.rows.iter().filter(|r| r.gone) {
                self.at.remove(&r.id);
            }
            self.rows.retain(|r| !r.gone);
            self.gone = 0;
            self.reindex(0);
        }
        self.looping = false;
        true
    }

    /// Whether a tick is between [`Pacer::advance`] and [`Pacer::settle`].
    pub fn looping(&self) -> bool {
        self.looping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The script: each brain counts for itself when called, in the spawner's order.
    #[derive(Clone, Copy, Debug)]
    struct Brain {
        id: i64,
        since: i64,
        owed: f64,
        every: i64,
    }

    impl Brain {
        fn new(id: i64) -> Self {
            Self {
                id,
                since: 0,
                owed: 0.0,
                every: 1,
            }
        }

        fn counters(&self) -> Counters {
            Counters {
                since: self.since,
                owed: self.owed,
                every: self.every,
            }
        }
    }

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    /// What happens while one brain thinks: another is staggered, one spawns, one is removed (maybe itself).
    #[derive(Clone, Copy)]
    struct Event {
        stagger: Option<i64>,
        spawn: Option<i64>,
        remove: Option<i64>,
    }

    #[test]
    fn matches_the_script_through_staggers_spawns_and_removals() {
        for seed in 1..80u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut script: Vec<Brain> = Vec::new();
            let mut pacer = Pacer::new();
            let mut next_id = 1i64;
            let mut ids = Vec::new();
            let mut owed = Vec::new();
            for tick in 0..300 {
                let delta = 1.0 / 60.0 + (rng.below(7) as f64) * 1e-4;
                // Between ticks: a spawn, a removal, a stagger.
                if rng.below(4) == 0 {
                    let b = Brain::new(next_id);
                    script.push(b);
                    pacer.add(next_id);
                    pacer.set_paced(next_id, true, b.counters());
                    next_id += 1;
                }
                if !script.is_empty() && rng.below(9) == 0 {
                    let k = rng.below(script.len() as u64) as usize;
                    let gone = script.remove(k);
                    assert_eq!(pacer.remove(gone.id), Some(gone.counters()));
                }
                if !script.is_empty() && rng.below(5) == 0 {
                    let k = rng.below(script.len() as u64) as usize;
                    script[k].every = 1;
                    pacer.set_every(script[k].id, 1);
                }
                let snapshot: Vec<i64> = script.iter().map(|b| b.id).collect();
                let pos: HashMap<i64, usize> = snapshot
                    .iter()
                    .enumerate()
                    .map(|(k, id)| (*id, k))
                    .collect();
                let nexts: Vec<i64> = snapshot.iter().map(|_| 1 + rng.below(6) as i64).collect();
                // The event fires inside the first brain at or after `event_at` that thinks.
                let event_at = rng.below(2 + snapshot.len() as u64) as usize;
                let pick = |r: &mut Rng| -> Option<i64> {
                    let k = r.below(1 + snapshot.len() as u64) as usize;
                    snapshot.get(k).copied()
                };
                let ev = Event {
                    stagger: pick(&mut rng),
                    spawn: (rng.below(4) == 0).then_some(next_id),
                    remove: if rng.below(3) == 0 {
                        pick(&mut rng)
                    } else {
                        None
                    },
                };

                // The script's loop.
                let mut s_log: Vec<(i64, u64)> = Vec::new();
                let mut s_fired = false;
                for (k, id) in snapshot.iter().enumerate() {
                    let Some(i) = script.iter().position(|b| b.id == *id) else {
                        continue;
                    };
                    let b = &mut script[i];
                    b.since += 1;
                    b.owed += delta;
                    if b.since < b.every {
                        continue;
                    }
                    s_log.push((*id, b.owed.to_bits()));
                    b.owed = 0.0;
                    b.since = 0;
                    if !s_fired && k >= event_at {
                        s_fired = true;
                        if let Some(o) = script.iter_mut().find(|o| Some(o.id) == ev.stagger) {
                            o.every = 1;
                        }
                        if let Some(n) = ev.spawn {
                            script.push(Brain::new(n));
                        }
                        if let Some(r) = ev.remove {
                            script.retain(|o| o.id != r);
                        }
                    }
                    if let Some(b) = script.iter_mut().find(|o| o.id == *id) {
                        b.every = nexts[k];
                    }
                }

                // The paced loop, as the spawner runs it.
                pacer.advance(delta, &mut ids, &mut owed);
                let mut due: Vec<(i64, f64)> =
                    ids.iter().copied().zip(owed.iter().copied()).collect();
                let mut n_log: Vec<(i64, u64)> = Vec::new();
                let mut thought = Vec::new();
                let mut everies = Vec::new();
                let mut alive: Vec<i64> = snapshot.clone();
                let mut n_fired = false;
                let mut q = 0;
                while let Some(&(id, o)) = due.get(q) {
                    q += 1;
                    if !alive.contains(&id) {
                        continue;
                    }
                    n_log.push((id, o.to_bits()));
                    if !n_fired && pos[&id] >= event_at {
                        n_fired = true;
                        pacer.commit_to(id, false, &thought, &everies);
                        thought.clear();
                        everies.clear();
                        if let Some(w) = ev.stagger {
                            pacer.set_every(w, 1);
                        }
                        if let Some(n) = ev.spawn {
                            pacer.add(n);
                            pacer.set_paced(n, true, Brain::new(n).counters());
                            alive.push(n);
                        }
                        if let Some(r) = ev.remove {
                            pacer.remove(r);
                            alive.retain(|a| *a != r);
                        }
                        pacer.due_after(id, &mut ids, &mut owed);
                        due.truncate(q);
                        due.extend(ids.iter().copied().zip(owed.iter().copied()));
                    }
                    thought.push(id);
                    everies.push(nexts[pos[&id]]);
                }
                pacer.settle(&thought, &everies);
                assert_eq!(s_fired, n_fired, "seed {seed} tick {tick}");
                if s_fired && ev.spawn.is_some() {
                    next_id += 1;
                }

                assert_eq!(s_log, n_log, "seed {seed} tick {tick}");
                assert_eq!(pacer.len(), script.len(), "seed {seed} tick {tick}");
                for b in &script {
                    let r = pacer.row(b.id).unwrap();
                    assert_eq!(
                        (r.since, r.owed.to_bits(), r.every),
                        (b.since, b.owed.to_bits(), b.every),
                        "seed {seed} tick {tick} id {}",
                        b.id
                    );
                }
            }
        }
    }

    #[test]
    fn unpaced_rows_are_always_due_and_never_counted() {
        let mut p = Pacer::new();
        for id in 1..=3 {
            p.add(id);
        }
        let mut ids = Vec::new();
        let mut owed = Vec::new();
        p.advance(0.5, &mut ids, &mut owed);
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(owed, vec![-1.0; 3]);
        p.due_after(i64::MIN, &mut ids, &mut owed);
        assert_eq!(ids, vec![1, 2, 3]);
        p.due_after(2, &mut ids, &mut owed);
        assert_eq!(ids, vec![3]);
        assert!(p.settle(&[], &[]));
        assert!(!p.settle(&[], &[]));
        assert_eq!(p.row(2).unwrap().since, 0);
    }

    #[test]
    fn remove_keeps_order_and_add_refuses_twice() {
        let mut p = Pacer::new();
        for id in [5, 3, 9, 1] {
            assert!(p.add(id));
        }
        assert!(!p.add(3));
        p.remove(3);
        let mut ids = Vec::new();
        let mut owed = Vec::new();
        p.advance(0.1, &mut ids, &mut owed);
        assert_eq!(ids, vec![5, 9, 1]);
        assert!(p.remove(9).is_some());
        assert_eq!(p.len(), 2);
        assert!(p.row(9).is_none());
        p.due_after(9, &mut ids, &mut owed);
        assert_eq!(ids, vec![1]);
        p.settle(&[], &[]);
        assert!(p.remove(42).is_none());
        assert_eq!(p.len(), 2);
        p.advance(0.1, &mut ids, &mut owed);
        assert_eq!(ids, vec![5, 1]);
    }

    #[test]
    fn a_row_added_mid_tick_sits_it_out() {
        let mut p = Pacer::new();
        p.add(1);
        let mut ids = Vec::new();
        let mut owed = Vec::new();
        p.advance(0.1, &mut ids, &mut owed);
        p.add(2);
        p.due_after(1, &mut ids, &mut owed);
        assert!(ids.is_empty());
        p.settle(&[], &[]);
        p.advance(0.1, &mut ids, &mut owed);
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn a_row_committed_through_is_not_counted_again() {
        let mut p = Pacer::new();
        for id in 1..=3 {
            p.add(id);
            p.set_paced(
                id,
                true,
                Counters {
                    since: 0,
                    owed: 0.0,
                    every: 2,
                },
            );
        }
        let mut ids = Vec::new();
        let mut owed = Vec::new();
        p.advance(0.25, &mut ids, &mut owed);
        assert!(ids.is_empty());
        // Row 2 turns unpaced during its own turn (a possession) and back on: committed through it first.
        assert!(p.commit_to(2, true, &[], &[]));
        let back = p.set_paced(2, false, Counters::default()).unwrap();
        assert_eq!((back.since, back.owed), (1, 0.25));
        p.set_paced(2, true, back);
        p.due_after(2, &mut ids, &mut owed);
        assert!(ids.is_empty());
        p.settle(&[], &[]);
        for id in 1..=3 {
            let r = p.row(id).unwrap();
            assert_eq!((r.since, r.owed), (1, 0.25), "id {id}");
        }
        p.advance(0.25, &mut ids, &mut owed);
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(owed, vec![0.5; 3]);
    }

    #[test]
    fn a_paced_row_owes_the_deltas_in_order() {
        let mut p = Pacer::new();
        p.add(7);
        p.set_paced(
            7,
            true,
            Counters {
                since: 0,
                owed: 0.0,
                every: 3,
            },
        );
        let mut ids = Vec::new();
        let mut owed = Vec::new();
        let mut sum = 0.0f64;
        for d in [0.1, 0.2] {
            p.advance(d, &mut ids, &mut owed);
            assert!(ids.is_empty());
            p.settle(&[], &[]);
            sum += d;
        }
        p.advance(0.3, &mut ids, &mut owed);
        assert_eq!(ids, vec![7]);
        assert_eq!(owed[0].to_bits(), (sum + 0.3).to_bits());
        p.settle(&[7], &[2]);
        assert_eq!(
            p.set_paced(7, false, Counters::default()),
            Some(Counters {
                since: 0,
                owed: 0.0,
                every: 2
            })
        );
    }
}
