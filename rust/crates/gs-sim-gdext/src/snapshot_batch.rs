//! `GSSimSnapshotBatch`: a whole snapshot's entities decoded and applied in one call, for the
//! remote entities whose per-entity work is data only (no change handlers, no predicted state).
//!
//! What `GSNetManager._apply_entity` does for such an entity, step for step: each behaviour's
//! state decoded by its codec, written to its properties and baseline, its `_net_state_applied`
//! hook called, the interpolated values handed to the track and to `GSSimInterpolator`. An
//! entity the batch does not hold, or anything it cannot do the script's way (a script-read
//! declaration, a truncated or mismatched body), is left to the script, with the reader back at
//! that entity's start and nothing of it applied: the script's errors and logs stay the script's.
//!
//! An entry is the identity it was registered from: once that identity is unregistered or any of
//! its behaviours freed (a game that drops an entity and registers a new one under the same net
//! id without `GSNetInterpolator.forget`), the entry is dropped at its next snapshot and the
//! entity left to the script, which reads the registry's identity and offers it to the batch again.
//!
//! With `set_skip_unchanged_hooks(true)` a behaviour's hook runs only when a value it decoded
//! differs from its baseline (type and value), as `GSNetBehaviour._finish_read` does with
//! `GSNetConfig.skip_unchanged_state_applied`; `last_changed` lists the entities that moved.

use godot::classes::Object;
use godot::prelude::*;
use gs_sim_snapshot::{Decl, Decoded, Reader, Stop, Value, decode_state};
use std::cell::{Cell, RefCell};
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
    /// Whether the hook runs on every snapshot even when nothing it decoded changed.
    always: bool,
}

struct Entry {
    parts: Vec<Part>,
    /// The interpolator's script track for this entity (`push(tick, values)`); none when the
    /// interpolator keeps its samples natively only.
    track: Option<Gd<Object>>,
    /// The `GSNetIdentity` the behaviours were registered from (their `identity`), if they had one.
    identity: Option<Gd<Object>>,
}

impl Entry {
    /// Whether the entry still is the entity the registry holds under its net id: every
    /// behaviour alive and the identity alive and registered. The script reads the registry's
    /// identity each snapshot; a stale entry would decode into behaviours nobody draws.
    fn live(&self) -> bool {
        self.parts.iter().all(|p| p.object.is_instance_valid())
            && self.identity.as_ref().is_none_or(|identity| {
                identity.is_instance_valid() && identity.get("_registered").booleanize()
            })
    }
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimSnapshotBatch {
    entries: RefCell<HashMap<i64, Rc<Entry>>>,
    /// Decode scratch, all parts of the entity in turn: `(part, index, value)`.
    scratch: RefCell<Vec<(usize, usize, Value)>>,
    /// Skip a hook whose behaviour decoded no changed value (`set_skip_unchanged_hooks`).
    skip_unchanged: Cell<bool>,
    /// The entities of the last `apply` with at least one changed value, in wire order.
    changed: RefCell<Vec<i64>>,
    /// Hooks not called because nothing changed, since the batch was made.
    hooks_skipped: Cell<i64>,
}

#[godot_api]
impl GSSimSnapshotBatch {
    /// Holds entity `net_id`: one `[behaviour, codec, props, interpolated, hooked, always]` array
    /// per behaviour in `parts` (`behaviour` a `GSNetBehaviour`; `codec` its `GSSimSnapshotCodec`, or
    /// null for one with no declarations; `props` an `Array` of property names by declaration
    /// index; `interpolated` a `PackedByteArray`, 1 for an interpolated declaration; `hooked`
    /// whether it overrides `_net_state_applied`; `always`, optional (false), whether its hook
    /// runs on unchanged snapshots too), and the entity's script track (null when
    /// `GSSimInterpolator` holds the samples alone). Returns
    /// false, holding nothing, if anything is not as described.
    #[func]
    fn register(&self, net_id: i64, parts: VarArray, track: Option<Gd<Object>>) -> bool {
        let mut held = Vec::with_capacity(parts.len());
        for row in parts.iter_shared() {
            let Ok(row) = row.try_to::<VarArray>() else {
                return false;
            };
            if row.len() != 5 && row.len() != 6 {
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
                always: row.len() == 6 && row.at(5).try_to::<bool>().unwrap_or(false),
            });
        }
        let identity = held
            .first()
            .and_then(|p| p.object.get("identity").try_to::<Gd<Object>>().ok());
        self.entries.borrow_mut().insert(
            net_id,
            Rc::new(Entry {
                parts: held,
                track,
                identity,
            }),
        );
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

    /// Off (the default): every hooked behaviour's `_net_state_applied` runs on each snapshot.
    /// On: only one that decoded a value differing from its baseline, or registered `always`.
    #[func]
    fn set_skip_unchanged_hooks(&self, on: bool) {
        self.skip_unchanged.set(on);
    }

    /// The net ids of the last `apply` whose decoded values changed anything, in wire order.
    #[func]
    fn last_changed(&self) -> PackedInt64Array {
        PackedInt64Array::from(self.changed.borrow().as_slice())
    }

    /// How many hook calls `set_skip_unchanged_hooks` has saved since the batch was made.
    #[func]
    fn hooks_skipped(&self) -> i64 {
        self.hooks_skipped.get()
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
        let skip_unchanged = self.skip_unchanged.get();
        let mut changed_ids = std::mem::take(&mut *self.changed.borrow_mut());
        changed_ids.clear();
        let mut skipped = 0i64;

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
            if !entry.live() {
                // Unregistered or freed since it was held: the script applies the registry's
                // identity, and holds that one.
                self.entries.borrow_mut().remove(&net_id);
                r.bit_pos = start;
                break;
            }

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

            // The interpolated values in the script dictionary's order: parts, then declarations,
            // a name seen again keeping its first place and taking the later value.
            let mut values: Vec<(StringName, Variant)> = Vec::new();
            let mut at = 0;
            let mut entity_changed = false;
            for (p, part) in entry.parts.iter().take(n).enumerate() {
                if !part.object.is_instance_valid() {
                    at = ends[p];
                    continue;
                }
                let mut object = part.object.clone();
                let mut read_now = part.read_now.clone();
                let mut baseline = part.baseline.clone();
                read_now.clear();
                let mut part_changed = false;
                for (_, index, value) in &flat[at..ends[p]] {
                    let variant = to_variant(value);
                    let prop = &part.props[*index];
                    // What GSNetBehaviour._apply_read counts as a change: absent, another type, or unequal.
                    part_changed = part_changed
                        || baseline.get(prop).is_none_or(|old| {
                            old.get_type() != variant.get_type() || old != variant
                        });
                    object.set(prop, &variant);
                    baseline.set(prop, &variant);
                    read_now.set(prop, true);
                }
                at = ends[p];
                object.set("_read_tick", &tick_v);
                entity_changed |= part_changed;
                if part.hooked {
                    if !skip_unchanged || part.always || part_changed {
                        object.call("_net_state_applied", std::slice::from_ref(&tick_v));
                    } else {
                        skipped += 1;
                    }
                }
                for (index, prop) in part.props.iter().enumerate() {
                    if part.interpolated[index]
                        && let Some(v) = baseline.get(prop)
                    {
                        match values.iter_mut().find(|(k, _)| k == prop) {
                            Some(slot) => slot.1 = v,
                            None => values.push((prop.clone(), v)),
                        }
                    }
                }
            }
            if !values.is_empty() {
                if let Some(track) = &entry.track {
                    let mut dict = VarDictionary::new();
                    for (k, v) in &values {
                        dict.set(k, v);
                    }
                    track
                        .clone()
                        .call("push", &[tick_v.clone(), dict.to_variant()]);
                }
                interp.bind_mut().push_list(net_id, tick, values);
            }
            if entity_changed {
                changed_ids.push(net_id);
            }
            left -= 1;
        }

        *self.scratch.borrow_mut() = flat;
        *self.changed.borrow_mut() = changed_ids;
        self.hooks_skipped.set(self.hooks_skipped.get() + skipped);
        let mut out = PackedInt64Array::new();
        out.push(r.bit_pos);
        out.push(left);
        out
    }
}
