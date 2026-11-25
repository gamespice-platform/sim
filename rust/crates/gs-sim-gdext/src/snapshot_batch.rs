//! `GSSimSnapshotBatch`: a whole snapshot's entities decoded and applied in one call, for the
//! remote entities whose per-entity work is data only (no change handlers, no predicted state).
//!
//! What `GSNetManager._apply_entity` does for such an entity, step for step: each behaviour's
//! state decoded by its codec, written to its properties and baseline, its `_net_state_applied`
//! hook called, the interpolated values handed to the track and to `GSSimInterpolator`. An
//! entity the batch does not hold, or anything it cannot do the script's way (a script-read
//! declaration, a truncated or mismatched body), is left to the script, with the reader back at
//! that entity's start and nothing of it applied: the script's errors and logs stay the script's.

use godot::classes::Object;
use godot::prelude::*;
use gs_sim_snapshot::{Decl, Decoded, Reader, Stop, Value, decode_state};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::interp::GSSimInterpolator;
use crate::snapshot::{GSSimSnapshotCodec, to_variant};

/// One behaviour of a held entity.
struct Part {
    object: Gd<Object>,
    decls: Vec<Decl>,
    /// Property per declaration index.
    props: Vec<StringName>,
    /// Whether the declaration's value goes to the interpolator.
    interpolated: Vec<bool>,
    baseline: VarDictionary,
    read_now: VarDictionary,
    /// Whether `_net_state_applied` is overridden (the base one does nothing).
    hooked: bool,
}

struct Entry {
    parts: Vec<Part>,
    /// The interpolator's script track for this entity: `push(tick, values)`.
    track: Gd<Object>,
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimSnapshotBatch {
    entries: RefCell<HashMap<i64, Rc<Entry>>>,
    /// Decode scratch, all parts of the entity in turn: `(part, index, value)`.
    scratch: RefCell<Vec<(usize, usize, Value)>>,
}

#[godot_api]
impl GSSimSnapshotBatch {
    /// Holds entity `net_id`: one `[behaviour, codec, props, interpolated, hooked]` array per
    /// behaviour in `parts` (`behaviour` a `GSNetBehaviour`; `codec` its `GSSimSnapshotCodec`, or
    /// null for one with no declarations; `props` an `Array` of property names by declaration
    /// index; `interpolated` a `PackedByteArray`, 1 for an interpolated declaration; `hooked`
    /// whether it overrides `_net_state_applied`), and the entity's script track. Returns
    /// false, holding nothing, if anything is not as described.
    #[func]
    fn register(&self, net_id: i64, parts: VarArray, track: Gd<Object>) -> bool {
        let mut held = Vec::with_capacity(parts.len());
        for row in parts.iter_shared() {
            let Ok(row) = row.try_to::<VarArray>() else {
                return false;
            };
            if row.len() != 5 {
                return false;
            }
            let Ok(object) = row.at(0).try_to::<Gd<Object>>() else {
                return false;
            };
            let decls = match row.at(1).try_to::<Gd<GSSimSnapshotCodec>>() {
                Ok(codec) => codec.bind().decls().to_vec(),
                Err(_) => Vec::new(),
            };
            let (Ok(names), Ok(flags)) = (
                row.at(2).try_to::<VarArray>(),
                row.at(3).try_to::<PackedByteArray>(),
            ) else {
                return false;
            };
            if names.len() != decls.len() || flags.len() != decls.len() {
                return false;
            }
            let (Ok(baseline), Ok(read_now)) = (
                object.get("_baseline").try_to::<VarDictionary>(),
                object.get("_read_now").try_to::<VarDictionary>(),
            ) else {
                return false;
            };
            held.push(Part {
                object,
                decls,
                props: names
                    .iter_shared()
                    .map(|v| v.try_to::<StringName>().unwrap_or_default())
                    .collect(),
                interpolated: flags.as_slice().iter().map(|&b| b != 0).collect(),
                baseline,
                read_now,
                hooked: row.at(4).try_to::<bool>().unwrap_or(false),
            });
        }
        self.entries
            .borrow_mut()
            .insert(net_id, Rc::new(Entry { parts: held, track }));
        true
    }

    #[func]
    fn has(&self, net_id: i64) -> bool {
        self.entries.borrow().contains_key(&net_id)
    }

    #[func]
    fn forget(&self, net_id: i64) {
        self.entries.borrow_mut().remove(&net_id);
    }

    #[func]
    fn clear(&self) {
        self.entries.borrow_mut().clear();
    }

    #[func]
    fn size(&self) -> i64 {
        self.entries.borrow().len() as i64
    }

    /// Applies up to `args[2]` entities from bit `args[0]` of `bytes` (ending at bit `args[1]`,
    /// negative: the buffer's end), at tick `args[4]`; each is a net id (`args[3]` bits), a behaviour count (6) and the
    /// behaviours' states. Stops before the first entity it cannot do. Returns
    /// `[bit position, entities left]`; the script applies the entity at that position, if any
    /// are left, and calls again.
    #[func]
    fn apply(
        &self,
        bytes: PackedByteArray,
        args: PackedInt64Array,
        mut interp: Gd<GSSimInterpolator>,
    ) -> PackedInt64Array {
        let &[bit_pos, end_bits, count, id_bits, tick] = args.as_slice() else {
            return PackedInt64Array::new();
        };
        let data = bytes.as_slice();
        let mut r = Reader::at(data, bit_pos, end_bits);
        let mut left = count;
        let mut decoded = Decoded::default();
        let mut flat = std::mem::take(&mut *self.scratch.borrow_mut());
        let tick_v = tick.to_variant();

        while left > 0 {
            let start = r.bit_pos;
            let net_id = r.read_bits(id_bits);
            let n = r.read_bits(6) as usize;
            let entry = match self.entries.borrow().get(&net_id) {
                Some(e) if !r.exhausted && n <= e.parts.len() => Rc::clone(e),
                _ => {
                    r.bit_pos = start;
                    break;
                }
            };

            flat.clear();
            let mut ends = [0usize; 64];
            let mut ok = true;
            for (p, part) in entry.parts.iter().take(n).enumerate() {
                let stop = decode_state(&part.decls, &mut r, -1, &mut decoded);
                if stop != Stop::Done || r.exhausted {
                    ok = false;
                    break;
                }
                flat.extend(decoded.entries.drain(..).map(|(i, v)| (p, i, v)));
                ends[p] = flat.len();
            }
            if !ok {
                r.bit_pos = start;
                break;
            }

            let mut values = VarDictionary::new();
            let mut at = 0;
            for (p, part) in entry.parts.iter().take(n).enumerate() {
                if !part.object.is_instance_valid() {
                    at = ends[p];
                    continue;
                }
                let mut object = part.object.clone();
                let mut read_now = part.read_now.clone();
                let mut baseline = part.baseline.clone();
                read_now.clear();
                for (_, index, value) in &flat[at..ends[p]] {
                    let variant = to_variant(value);
                    let prop = &part.props[*index];
                    object.set(prop, &variant);
                    baseline.set(prop, &variant);
                    read_now.set(prop, true);
                }
                at = ends[p];
                object.set("_read_tick", &tick_v);
                if part.hooked {
                    object.call("_net_state_applied", std::slice::from_ref(&tick_v));
                }
                for (index, prop) in part.props.iter().enumerate() {
                    if part.interpolated[index]
                        && let Some(v) = baseline.get(prop)
                    {
                        values.set(prop, &v);
                    }
                }
            }
            if !values.is_empty() {
                entry
                    .track
                    .clone()
                    .call("push", &[tick_v.clone(), values.to_variant()]);
                interp.bind_mut().push_values(net_id, tick, &values);
            }
            left -= 1;
        }

        *self.scratch.borrow_mut() = flat;
        let mut out = PackedInt64Array::new();
        out.push(r.bit_pos);
        out.push(left);
        out
    }
}
