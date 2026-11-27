//! `GSSimFpsNetPullBatch`: every player's net pull in one call a tick: `GSSimFpsNetPack.pull` (movement and
//! the sub-quantum hold) and `GSSimFpsWeaponNet.pull` (weapon integers and the held weapon's wire number) over
//! flat rows (gs-sim-motor's `net_pull_batch`). Same integers as those per-player calls, bit for bit. Holds the
//! weapon wire table a game hands it with `set_wire_ids`.

use std::collections::HashMap;

use godot::prelude::*;
use gs_sim_motor::net_pull_batch::{MOVE_STRIDE, OUT_STRIDE, WEAPON_STRIDE, pull_all};
use gs_sim_motor::weapon_net::weapon_code;

/// Every carrier's replicated movement and weapon integers in one call.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimFpsNetPullBatch {
    wire: HashMap<StringName, i64>,
}

#[godot_api]
impl GSSimFpsNetPullBatch {
    /// Floats in one player's movement row: the state's columns (`GSFpsState._f`), then `[crouch_held, stamina,
    /// stamina_exhausted, stamina_delay_left, pitch, held_velocity x, y, z, held_yaw, held_pitch, max_speed,
    /// velocity_bits, yaw_bits, pitch_bits]` (booleans 0 or 1; see `GSSimFpsNetPack.pull`).
    #[func]
    fn move_stride(&self) -> i64 {
        MOVE_STRIDE as i64
    }

    /// Floats in one player's weapon row: `[current_slot, has_slot, fire_mode_index, magazine, reserve,
    /// fire_seq, fire_kind, charge_shown, windup]` (see `GSSimFpsWeaponNet.pull`).
    #[func]
    fn weapon_stride(&self) -> i64 {
        WEAPON_STRIDE as i64
    }

    /// Integers in one player's output row (see [method pull]).
    #[func]
    fn out_stride(&self) -> i64 {
        OUT_STRIDE as i64
    }

    /// The weapon wire table: [param ids]'s index is an id's wire number. Replaces the last one.
    #[func]
    fn set_wire_ids(&mut self, ids: Array<StringName>) {
        self.wire.clear();
        for (i, id) in ids.iter_shared().enumerate() {
            self.wire.insert(id, i as i64);
        }
    }

    /// One row of [method out_stride] integers per player, in order: `[net_flags, net_timers2, hold, net_slot,
    /// net_fire_mode, net_magazine, net_reserve, net_fire_seq, net_fire_kind, net_charge, net_weapon]`, for the
    /// players that [param moves], [param weapons] and [param weapon_ids] (each held def's id, `&""` for none)
    /// all have whole rows for. `hold` is `GSSimFpsNetPack.pull`'s.
    #[func]
    fn pull(
        &self,
        moves: PackedFloat64Array,
        weapons: PackedFloat64Array,
        weapon_ids: Array<StringName>,
        timer_scale: f64,
    ) -> PackedInt64Array {
        let codes: Vec<i64> = weapon_ids
            .iter_shared()
            .map(|id| weapon_code(id.is_empty(), self.wire.get(&id).copied()))
            .collect();
        PackedInt64Array::from(
            &pull_all(moves.as_slice(), weapons.as_slice(), timer_scale, &codes)[..],
        )
    }
}
