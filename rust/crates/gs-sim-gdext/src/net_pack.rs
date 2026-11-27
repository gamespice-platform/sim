//! `GSSimFpsNetPack`: gs-player-controller's `GSFpsNetSync` packers over a state's columns, one
//! call per pull (the script stays the fallback and the specification), and `pull` adds a carrier's sub-quantum
//! hold (Fatal Contact's `FcPlayerNet._hold_subquantum`). `unpack` is the receiving half (`GSFpsNetSync.push`'s
//! column writes), and `unpack_exact` adds an exact movement record (`push_exact`) in the same call. Stateless.

use godot::prelude::*;
use gs_sim_motor::net_pack::{
    EXACT_FLOATS_TIMED, Extra, Held, Wire, pack, pull, unpack, unpack_exact,
};

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

    /// [method pack] plus the hold: `[net_flags, net_timers, net_timers2, net_stamina, hold]`, where `hold` has
    /// bit 0 when the carrier should keep [param held_velocity], bit 1 [param held_yaw] and bit 2
    /// [param held_pitch]: the new value differs but writes the same codes. [param wire] is
    /// `[max_speed, velocity_bits, yaw_bits, pitch_bits]`; a width outside 1..62 holds nothing for that
    /// property. Empty when [param cols] is shorter than a state's or [param wire] than four.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn pull(
        &self,
        cols: PackedFloat64Array,
        crouch_held: bool,
        timer_scale: f64,
        stamina: f64,
        stamina_exhausted: bool,
        stamina_delay_left: f64,
        pitch: f64,
        held_velocity: Vector3,
        held_yaw: f64,
        held_pitch: f64,
        wire: PackedFloat64Array,
    ) -> PackedInt64Array {
        let cols = cols.as_slice();
        let wire = wire.as_slice();
        if cols.len() < gs_sim_motor::cols::STRIDE || wire.len() < 4 {
            return PackedInt64Array::new();
        }
        let extra = Extra {
            crouch_held,
            timer_scale,
            stamina,
            stamina_exhausted,
            stamina_delay_left,
        };
        let held = Held {
            velocity: [held_velocity.x, held_velocity.y, held_velocity.z],
            yaw: held_yaw,
            pitch: held_pitch,
        };
        let wire = Wire {
            speed: wire[0],
            velocity_bits: wire[1] as i64,
            yaw_bits: wire[2] as i64,
            pitch_bits: wire[3] as i64,
        };
        PackedInt64Array::from(&pull(cols, &extra, pitch, &held, &wire)[..])
    }

    /// The receiving half: [param cols] (`GSFpsState._f`) with the replicated position, velocity and yaw written
    /// and [param flags] unpacked (`GSFpsNetSync.unpack_flags`), then [param timers2] when it is not negative
    /// (`unpack_timers2`). `crouch_held` is not a column: the caller reads it from [param flags]. Empty when
    /// [param cols] is shorter than a state's.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn unpack(
        &self,
        cols: PackedFloat64Array,
        position: Vector3,
        velocity: Vector3,
        yaw: f64,
        flags: i64,
        timers2: i64,
        timer_scale: f64,
    ) -> PackedFloat64Array {
        let mut cols = cols;
        if cols.len() < gs_sim_motor::cols::STRIDE {
            return PackedFloat64Array::new();
        }
        unpack(
            cols.as_mut_slice(),
            [position.x, position.y, position.z],
            [velocity.x, velocity.y, velocity.z],
            yaw,
            flags,
            (timers2 >= 0).then_some(timers2),
            timer_scale,
        );
        cols
    }

    /// Longest exact record [method unpack_exact] takes (`GSFpsNetSync.EXACT_FLOATS_TIMED`). A library without
    /// this method takes only the fourteen-float one.
    #[func]
    fn exact_floats_timed(&self) -> i64 {
        EXACT_FLOATS_TIMED as i64
    }

    /// [method unpack], then [param exact] (an exact movement record, `GSFpsNetSync.EXACT_FLOATS` or
    /// `EXACT_FLOATS_TIMED` long) written over its columns (`GSFpsNetSync.push_exact`'s column writes, slide and
    /// wall-run time too for a timed record) when it has one of those lengths. One call for a
    /// receiver that gets both. Empty when [param cols] is shorter than a state's.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn unpack_exact(
        &self,
        cols: PackedFloat64Array,
        position: Vector3,
        velocity: Vector3,
        yaw: f64,
        flags: i64,
        timers2: i64,
        timer_scale: f64,
        exact: PackedFloat32Array,
    ) -> PackedFloat64Array {
        let mut cols = cols;
        if cols.len() < gs_sim_motor::cols::STRIDE {
            return PackedFloat64Array::new();
        }
        let slice = cols.as_mut_slice();
        unpack(
            slice,
            [position.x, position.y, position.z],
            [velocity.x, velocity.y, velocity.z],
            yaw,
            flags,
            (timers2 >= 0).then_some(timers2),
            timer_scale,
        );
        unpack_exact(slice, exact.as_slice());
        cols
    }
}
