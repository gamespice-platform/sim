//! `GSSimBotDrops`: GSBotWorld's drop table packed from a drop set's snapshot in one native pass
//! (GSBotArms.drops_of, then GSBotWorld.drop_table), the script's columns to the bit. The per-weapon
//! numbers (slot, rating, rounds when full, infinite) come from a script callable asked once per
//! weapon id in the snapshot; the per-drop reads and the ammunition fraction are here.

use godot::builtin::{AnyArray, AnyDictionary};
use godot::prelude::*;
use gs_sim_brain::pickup::{STRIDE, ammo_fraction};
use std::collections::HashMap;

/// Columns of one drop table: ids, positions, and per drop slot, value, ammo.
type Columns = (PackedInt64Array, PackedVector3Array, PackedFloat64Array);

/// What a `def_row` answer holds: slot, value, full, infinite.
const DEF_STRIDE: usize = 4;

/// Stateless: [`Self::pack`] is static.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBotDrops {}

/// `int(v)` as GDScript takes a whole number of rounds; `None` for a type the pack does not read
/// (the script then packs, and says what it makes of it).
fn int_of(v: &Variant) -> Option<i64> {
    match v.get_type() {
        VariantType::INT => v.try_to::<i64>().ok(),
        VariantType::FLOAT => v.try_to::<f64>().ok().map(|f| f as i64),
        VariantType::BOOL => v.try_to::<bool>().ok().map(i64::from),
        _ => None,
    }
}

/// `int(state.get(key, 0))`.
fn count(state: &AnyDictionary, key: &Variant) -> Option<i64> {
    state.get(key).map_or(Some(0), |v| int_of(&v))
}

/// The pack, or `None` where the script would read something this does not (it then packs itself).
fn pack_columns(snapshot: &AnyArray, def_row: &Callable) -> Option<Columns> {
    let (k_id, k_state, k_at) = ("id".to_variant(), "state".to_variant(), "at".to_variant());
    let (k_mag, k_res) = ("magazine".to_variant(), "reserve".to_variant());
    let mut defs: HashMap<String, Option<[f64; DEF_STRIDE]>> = HashMap::new();
    let n = snapshot.len();
    let mut ids = PackedInt64Array::new();
    let mut at = PackedVector3Array::new();
    let mut rows = PackedFloat64Array::new();
    ids.resize(n);
    at.resize(n);
    rows.resize(n * STRIDE);
    let mut kept = 0;
    for d in snapshot.iter_shared() {
        let d = d.try_to::<AnyDictionary>().ok()?;
        let state = d.get(&k_state)?.try_to::<AnyDictionary>().ok()?;
        let weapon = state.get(&k_id)?;
        if !matches!(
            weapon.get_type(),
            VariantType::STRING | VariantType::STRING_NAME
        ) {
            return None;
        }
        let key = weapon.to_string();
        let def = match defs.get(&key) {
            Some(def) => *def,
            None => {
                let got = def_row.call(&[weapon]);
                let def = if got.is_nil() {
                    None
                } else {
                    let row = got.try_to::<PackedFloat64Array>().ok()?;
                    match row.as_slice() {
                        [] => None,
                        &[slot, value, full, infinite] => Some([slot, value, full, infinite]),
                        _ => return None,
                    }
                };
                defs.insert(key, def);
                def
            }
        };
        let Some([slot, value, full, infinite]) = def else {
            continue;
        };
        let id = d.get(&k_id)?;
        if id.get_type() != VariantType::INT {
            return None;
        }
        let origin = d.get(&k_at)?.try_to::<Transform3D>().ok()?.origin;
        let ammo = ammo_fraction(
            infinite != 0.0,
            full as i64,
            count(&state, &k_mag)?,
            count(&state, &k_res)?,
        );
        ids.as_mut_slice()[kept] = id.try_to::<i64>().ok()?;
        at.as_mut_slice()[kept] = origin;
        rows.as_mut_slice()[kept * STRIDE..(kept + 1) * STRIDE]
            .copy_from_slice(&[slot, value, ammo]);
        kept += 1;
    }
    ids.resize(kept);
    at.resize(kept);
    rows.resize(kept * STRIDE);
    Some((ids, at, rows))
}

#[godot_api]
impl GSSimBotDrops {
    /// GSBotWorld.drop_table's `[ids, at, rows]` (rows: slot, value, ammo per drop) for `snapshot`, a
    /// GSWeaponDrops snapshot (`{id, state: {id, magazine, reserve}, at: Transform3D}` per drop), or
    /// an empty array when an entry is not of that shape (the script then packs). `def_row(weapon_id)`
    /// answers `[slot, value, magazine + reserve_cap, infinite]` (empty or null: no such weapon,
    /// skipped) and is asked once per weapon id. Safe on a worker thread when `def_row` is.
    #[func]
    fn pack(snapshot: AnyArray, def_row: Callable) -> VarArray {
        match pack_columns(&snapshot, &def_row) {
            Some((ids, at, rows)) => {
                varray![&ids.to_variant(), &at.to_variant(), &rows.to_variant()]
            }
            None => VarArray::new(),
        }
    }
}
