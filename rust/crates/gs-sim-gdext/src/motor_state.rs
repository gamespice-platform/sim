//! `GSSimMotorState`: the motor state a player keeps between ticks beside its `GSFpsState`
//! columns (the rest stamp, `gs_sim_motor::state`), kept natively so `GSSimMotorBatch.run_states`
//! reads and stamps it without the script.

use godot::prelude::*;
use gs_sim_motor::state::Rest;

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimMotorState {
    pub(crate) rest: Rest,
}

#[godot_api]
impl GSSimMotorState {
    /// The rest stamp: valid (1 or 0), position, velocity, ground normal, the ground id's low and
    /// high halves, height (13 values), for the script to take it over.
    #[func]
    fn rest(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(&self.rest.to_vals()[..])
    }

    /// Sets the rest stamp from the script's, in [`Self::rest`]'s layout.
    #[func]
    fn set_rest(&mut self, vals: PackedFloat64Array) {
        self.rest = Rest::from_vals(vals.as_slice());
    }

    /// Drops the rest stamp.
    #[func]
    fn clear(&mut self) {
        self.rest.valid = false;
    }
}
