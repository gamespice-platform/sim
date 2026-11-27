//! `GSSimTickEvents`: a tick's event rows (`gs_sim_events`), the motor edges gs-player-controller's
//! `GSFpsController` emitted one signal at a time, derived for every player of a tick in one call
//! and drained by the game in two (`ints`, `floats`). The script twin is `GSFpsTickEvents`
//! (gs-player-controller 50); `tick_events_parity` holds the two to the bit. `GSSim.has(&"events")`
//! switches it.

use godot::prelude::*;
use gs_sim_events::{EventTable, FLOATS, IN, INTS, kind};

/// One tick's event table.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimTickEvents {
    table: EventTable,
}

#[godot_api]
impl GSSimTickEvents {
    /// `[IN, INTS, FLOATS, kinds]`: the layout the script twin checks before trusting the table.
    #[func]
    fn layout(&self) -> PackedInt32Array {
        PackedInt32Array::from(&[IN as i32, INTS as i32, FLOATS as i32, kind::COUNT][..])
    }

    /// Replaces the table with the events of the first [param count] rows of [param rows] (18 floats
    /// each, `GSFpsTickEvents`' `IN_*` columns); answers how many events there are.
    #[func]
    fn derive(&mut self, rows: PackedFloat64Array, count: i64) -> i64 {
        self.table.clear();
        let n = (usize::try_from(count).unwrap_or(0) * IN).min(rows.len());
        self.table.derive_rows(&rows.as_slice()[..n]);
        self.table.len() as i64
    }

    /// The table's `[entity, kind, i0, i1]` per event.
    #[func]
    fn ints(&self) -> PackedInt32Array {
        PackedInt32Array::from(&self.table.ints[..])
    }

    /// The table's `[f0, f1]` per event.
    #[func]
    fn floats(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(&self.table.floats[..])
    }
}
