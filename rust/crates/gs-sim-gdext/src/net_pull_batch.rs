//! `GSSimFpsNetPullBatch`: every player's net pull in one call a tick: `GSSimFpsNetPack.pull` (movement and
//! the sub-quantum hold) and `GSSimFpsWeaponNet.pull` (weapon integers and the held weapon's wire number) over
//! flat rows (gs-sim-motor's `net_pull_batch`). Same integers as those per-player calls, bit for bit. Holds the
//! weapon wire table a game hands it with `set_wire_ids`. `pull_apply` also writes the answers onto the players'
//! net objects, so a server's whole pull is one call with no per-field script.

use std::collections::HashMap;

use godot::prelude::*;
use gs_sim_motor::cols::STRIDE;
use gs_sim_motor::net_pull_batch::{
    APPLY_EXTRA, Applied, MOVE_EXTRA, OUT_STRIDE, WEAPON_STRIDE, applied, players, pull_all,
};
use gs_sim_motor::weapon_net::weapon_code;

/// Every carrier's replicated movement and weapon integers in one call.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimFpsNetPullBatch {
    wire: HashMap<StringName, i64>,
    /// [`FIELDS`] as names, made on the first [method pull_apply].
    names: Vec<StringName>,
}

/// The net object's properties [method pull_apply] writes, in [`GSSimFpsNetPullBatch::write`]'s order.
const FIELDS: [&str; 19] = [
    "net_position",
    "net_velocity",
    "net_yaw",
    "net_pitch",
    "net_crouch",
    "net_flags",
    "net_modifiers",
    "net_timers2",
    "net_slot",
    "net_fitted",
    "net_fire_mode",
    "net_magazine",
    "net_reserve",
    "net_reloading",
    "net_switching",
    "net_fire_seq",
    "net_fire_kind",
    "net_charge",
    "net_weapon",
];

#[godot_api]
impl GSSimFpsNetPullBatch {
    /// The fewest state columns (`GSFpsState._f`) a movement row may carry; [method pull] pulls nobody below it.
    #[func]
    fn min_columns(&self) -> i64 {
        STRIDE as i64
    }

    /// Floats in one player's movement row after its state columns: `[crouch_held, stamina, stamina_exhausted,
    /// stamina_delay_left, pitch, held_velocity x, y, z, held_yaw, held_pitch, max_speed, velocity_bits, yaw_bits,
    /// pitch_bits]` (booleans 0 or 1; see `GSSimFpsNetPack.pull`).
    #[func]
    fn move_extra(&self) -> i64 {
        MOVE_EXTRA as i64
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
    /// players that [param moves] (rows of [param columns] state columns then [method move_extra]), [param weapons]
    /// and [param weapon_ids] (each held def's id, `&""` for none) all have whole rows for. `hold` is
    /// `GSSimFpsNetPack.pull`'s.
    #[func]
    fn pull(
        &self,
        moves: PackedFloat64Array,
        columns: i64,
        weapons: PackedFloat64Array,
        weapon_ids: Array<StringName>,
        timer_scale: f64,
    ) -> PackedInt64Array {
        let codes: Vec<i64> = weapon_ids
            .iter_shared()
            .map(|id| weapon_code(id.is_empty(), self.wire.get(&id).copied()))
            .collect();
        PackedInt64Array::from(
            &pull_all(
                moves.as_slice(),
                columns.max(0) as usize,
                weapons.as_slice(),
                timer_scale,
                &codes,
            )[..],
        )
    }

    /// Floats per player in [method pull_apply]'s [param extras]: `[crouch_fraction, modifiers, fitted, reloading,
    /// switching]` (the packed modifiers and fitted code as whole numbers, booleans 0 or 1).
    #[func]
    fn apply_extra(&self) -> i64 {
        APPLY_EXTRA as i64
    }

    /// [method pull], then writes each row's answers onto the net object of [param nets] in the same order, as the
    /// game's script apply does: `net_position`, `net_velocity`, `net_yaw` and `net_pitch` (the last three only
    /// where the sub-quantum hold lets them change), `net_crouch`, `net_flags`, `net_modifiers`, `net_timers2` and
    /// the weapon fields (`net_slot`, `net_fitted`, `net_fire_mode`, `net_magazine`, `net_reserve`,
    /// `net_reloading`, `net_switching`, `net_fire_seq`, `net_fire_kind`, `net_charge`, `net_weapon`). Returns how
    /// many players it wrote; on rows that do not cover every net (or [param extras] short of
    /// [method apply_extra] per net) it writes nobody, reports an error and returns -1.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn pull_apply(
        &mut self,
        nets: Array<Gd<Object>>,
        moves: PackedFloat64Array,
        columns: i64,
        weapons: PackedFloat64Array,
        weapon_ids: Array<StringName>,
        timer_scale: f64,
        extras: PackedFloat64Array,
    ) -> i64 {
        let columns = columns.max(0) as usize;
        let codes: Vec<i64> = weapon_ids
            .iter_shared()
            .map(|id| weapon_code(id.is_empty(), self.wire.get(&id).copied()))
            .collect();
        let n = nets.len();
        let (m, w, x) = (moves.as_slice(), weapons.as_slice(), extras.as_slice());
        if players(m, columns, w, &codes) != n || codes.len() != n || x.len() != n * APPLY_EXTRA {
            godot_error!(
                "GSSimFpsNetPullBatch.pull_apply: rows for {} players, {} weapon ids, {} extras do not match {} nets",
                players(m, columns, w, &codes),
                codes.len(),
                x.len(),
                n
            );
            return -1;
        }
        if self.names.is_empty() {
            self.names = FIELDS.iter().map(|f| StringName::from(*f)).collect();
        }
        let out = pull_all(m, columns, w, timer_scale, &codes);
        let stride = columns + MOVE_EXTRA;
        for (i, mut net) in nets.iter_shared().enumerate() {
            let row = &out[i * OUT_STRIDE..(i + 1) * OUT_STRIDE];
            let a = applied(
                &m[i * stride..(i + 1) * stride],
                columns,
                &x[i * APPLY_EXTRA..(i + 1) * APPLY_EXTRA],
                row,
            );
            self.write(&mut net, &a, row);
        }
        n as i64
    }
}

impl GSSimFpsNetPullBatch {
    /// One player's fields, with the variant types the script's typed properties hold.
    fn write(&self, net: &mut Gd<Object>, a: &Applied, row: &[i64]) {
        let k = &self.names;
        let v3 = |p: [f32; 3]| Vector3::new(p[0], p[1], p[2]).to_variant();
        net.set(&k[0], &v3(a.position));
        if let Some(v) = a.velocity {
            net.set(&k[1], &v3(v));
        }
        if let Some(y) = a.yaw {
            net.set(&k[2], &y.to_variant());
        }
        if let Some(p) = a.pitch {
            net.set(&k[3], &p.to_variant());
        }
        net.set(&k[4], &a.crouch.to_variant());
        net.set(&k[5], &row[0].to_variant());
        net.set(&k[6], &a.modifiers.to_variant());
        net.set(&k[7], &row[1].to_variant());
        net.set(&k[8], &row[3].to_variant());
        net.set(&k[9], &a.fitted.to_variant());
        net.set(&k[10], &row[4].to_variant());
        net.set(&k[11], &row[5].to_variant());
        net.set(&k[12], &row[6].to_variant());
        net.set(&k[13], &a.reloading.to_variant());
        net.set(&k[14], &a.switching.to_variant());
        net.set(&k[15], &row[7].to_variant());
        net.set(&k[16], &row[8].to_variant());
        net.set(&k[17], &row[9].to_variant());
        net.set(&k[18], &row[10].to_variant());
    }
}
