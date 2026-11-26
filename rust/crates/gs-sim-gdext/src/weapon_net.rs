//! `GSSimFpsWeaponNet`: fc-weapons' `AwWeaponNet.pull` with gs-weapon's `GSWeaponNetSync.pull` under it, and
//! Fatal Contact's `FcContent.weapon_index`, in one call per pull (the scripts stay the fallback and the
//! specification). Holds the weapon wire table a game hands it with `set_wire_ids`.

use std::collections::HashMap;

use godot::prelude::*;
use gs_sim_motor::weapon_net::{WeaponIn, pull, weapon_code};

/// A carrier's replicated weapon integers from its arsenal and rig numbers.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimFpsWeaponNet {
    wire: HashMap<StringName, i64>,
}

#[godot_api]
impl GSSimFpsWeaponNet {
    /// The weapon wire table: [param ids]'s index is an id's wire number. Replaces the last one.
    #[func]
    fn set_wire_ids(&mut self, ids: Array<StringName>) {
        self.wire.clear();
        for (i, id) in ids.iter_shared().enumerate() {
            self.wire.insert(id, i as i64);
        }
    }

    /// [param id]'s weapon code: 0 for `&""`, else its index in the wire table plus one (0 when absent).
    #[func]
    fn weapon_code(&self, id: StringName) -> i64 {
        weapon_code(id.is_empty(), self.wire.get(&id).copied())
    }

    /// `[net_slot, net_fire_mode, net_magazine, net_reserve, net_fire_seq, net_fire_kind, net_charge,
    /// net_weapon]`. A negative [param windup] keeps the rig's [param charge_shown]; otherwise the charge is
    /// the windup's draw, at least 1. [param weapon_id] is the held def's id, `&""` for none.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn pull(
        &self,
        current_slot: i64,
        has_slot: bool,
        fire_mode_index: i64,
        magazine: i64,
        reserve: i64,
        fire_seq: i64,
        fire_kind: i64,
        charge_shown: f64,
        windup: f64,
        weapon_id: StringName,
    ) -> PackedInt64Array {
        let w = WeaponIn {
            current_slot,
            has_slot,
            fire_mode_index,
            magazine,
            reserve,
            fire_seq,
            fire_kind,
            charge_shown,
            windup,
        };
        let [a, b, c, d, e, f, g] = pull(&w);
        PackedInt64Array::from(&[a, b, c, d, e, f, g, self.weapon_code(weapon_id)][..])
    }
}
