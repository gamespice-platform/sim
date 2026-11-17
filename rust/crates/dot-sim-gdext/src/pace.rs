//! `DotSimThinkPacer`: dot-npc's think pacing (`DotNpcPacer`) held natively, one batch per tick.
//! See [`dot_sim_zombie::pace`] for the steps; this class only moves packed arrays across.

use dot_sim_zombie::pace::{Counters, Pacer};
use godot::prelude::*;

fn counters(c: Option<Counters>) -> PackedFloat64Array {
    match c {
        Some(c) => PackedFloat64Array::from(&[c.since as f64, c.owed, c.every as f64][..]),
        None => PackedFloat64Array::new(),
    }
}

/// The spawner's pacing rows, in its NPC order.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimThinkPacer {
    pacer: Pacer,
    ids: Vec<i64>,
    owed: Vec<f64>,
}

#[godot_api]
impl DotSimThinkPacer {
    /// Appends an NPC, unpaced. False when it already has a row.
    #[func]
    fn add(&mut self, id: i64) -> bool {
        self.pacer.add(id)
    }

    /// Drops an NPC's row; its counters [since, owed, every], or empty when it had none.
    #[func]
    fn remove(&mut self, id: i64) -> PackedFloat64Array {
        counters(self.pacer.remove(id))
    }

    /// Paces an NPC from the brain's counters, or stops; returns the counters the row held
    /// [since, owed, every] (empty when it has no row).
    #[func]
    fn set_paced(
        &mut self,
        id: i64,
        on: bool,
        since: i64,
        owed: f64,
        every: i64,
    ) -> PackedFloat64Array {
        counters(
            self.pacer
                .set_paced(id, on, Counters { since, owed, every }),
        )
    }

    /// A brain's `every`, set outside its think. False when it has no row.
    #[func]
    fn set_every(&mut self, id: i64, every: i64) -> bool {
        self.pacer.set_every(id, every)
    }

    /// Starts a tick: the due NPCs in order. [method owed] then gives the seconds each owes (-1: unpaced).
    #[func]
    fn advance(&mut self, delta: f64) -> PackedInt64Array {
        self.pacer.advance(delta, &mut self.ids, &mut self.owed);
        PackedInt64Array::from(&self.ids[..])
    }

    /// The seconds owed by the NPCs of the last [method advance] or [method due_after].
    #[func]
    fn owed(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(&self.owed[..])
    }

    /// Commits the rows before `id` (through it when `through`) with the NPCs thought since the last commit and their new `every`.
    #[func]
    fn commit_to(
        &mut self,
        id: i64,
        through: bool,
        thought: PackedInt64Array,
        every: PackedInt64Array,
    ) -> bool {
        self.pacer
            .commit_to(id, through, thought.as_slice(), every.as_slice())
    }

    /// The NPCs after `id` still due this tick (all rows when `id` has none); [method owed] as after advance.
    #[func]
    fn due_after(&mut self, id: i64) -> PackedInt64Array {
        self.pacer.due_after(id, &mut self.ids, &mut self.owed);
        PackedInt64Array::from(&self.ids[..])
    }

    /// Ends the tick with the NPCs thought since the last commit and their new `every`.
    #[func]
    fn settle(&mut self, thought: PackedInt64Array, every: PackedInt64Array) -> bool {
        self.pacer.settle(thought.as_slice(), every.as_slice())
    }

    /// An NPC's counters [since, owed, every, paced], or empty when it has no row.
    #[func]
    fn row(&self, id: i64) -> PackedFloat64Array {
        match self.pacer.row(id) {
            Some(r) => PackedFloat64Array::from(
                &[
                    r.since as f64,
                    r.owed,
                    r.every as f64,
                    f64::from(u8::from(r.paced)),
                ][..],
            ),
            None => PackedFloat64Array::new(),
        }
    }

    /// Whether a tick is open (between advance and settle).
    #[func]
    fn looping(&self) -> bool {
        self.pacer.looping()
    }

    #[func]
    fn size(&self) -> i64 {
        self.pacer.len() as i64
    }

    #[func]
    fn clear(&mut self) {
        self.pacer.clear();
    }
}
