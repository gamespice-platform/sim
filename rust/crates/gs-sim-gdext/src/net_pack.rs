//! `GSSimFpsNetPack`: gs-player-controller's `GSFpsNetSync` packers over a state's columns, one
//! call per pull (the script stays the fallback and the specification). Stateless.

use godot::prelude::*;
use gs_sim_motor::net_pack::{Extra, pack};

/// A state's replicated integers from its columns.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimFpsNetPack {}

#[godot_api]
impl GSSimFpsNetPack {
    /// `[net_flags, net_timers, net_timers2, net_stamina]` from [param cols] (`GSFpsState._f`) and the
    /// state's values outside them. Empty when [param cols] is shorter than a state's.
    #[func]
    fn pack(
        &self,
        cols: PackedFloat64Array,
        crouch_held: bool,
        timer_scale: f64,
        stamina: f64,
        stamina_exhausted: bool,
        stamina_delay_left: f64,
    ) -> PackedInt64Array {
        let cols = cols.as_slice();
        if cols.len() < gs_sim_motor::cols::STRIDE {
            return PackedInt64Array::new();
        }
        let extra = Extra {
            crouch_held,
            timer_scale,
            stamina,
            stamina_exhausted,
            stamina_delay_left,
        };
        PackedInt64Array::from(&pack(cols, &extra)[..])
    }
}
