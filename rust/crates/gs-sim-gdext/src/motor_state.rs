//! `GSSimMotorState`: the motor state a player keeps between ticks beside its `GSFpsState`
//! columns (the rest stamp, `gs_sim_motor::state`), kept natively so `GSSimMotorBatch.run_states`
//! reads and stamps it without the script, and the effect and tunable values a row asks for.

use godot::prelude::*;
use gs_sim_motor::state::{CONSTS_LEN, Rest};

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimMotorState {
    pub(crate) rest: Rest,
    /// The row's effect and tunable values (row 23..38) as the script last sent them, for rows
    /// that ask for them (`gs_sim_motor::state::ROW_CONSTS`).
    pub(crate) consts: Option<[f64; CONSTS_LEN]>,
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

    /// Drops the rest stamp and the kept values.
    #[func]
    fn clear(&mut self) {
        self.rest.valid = false;
        self.consts = None;
    }

    /// Keeps a row's effect and tunable values (row 23..38, 16 values) for the rows that ask for
    /// them with `extra[1]`'s bit 4, so the script writes them only when they change
    /// (gs-player-controller 40). A shorter `vals` drops them; such rows then keep `extra`'s.
    #[func]
    fn set_consts(&mut self, vals: PackedFloat64Array) {
        self.consts = vals
            .as_slice()
            .get(..CONSTS_LEN)
            .and_then(|v| <[f64; CONSTS_LEN]>::try_from(v).ok());
    }
}
