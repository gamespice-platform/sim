//! `GSSimInterpolator`: the tracks and the per-frame draw of gs-net's `GSNetInterpolator`,
//! writing what the script's `apply` writes, value for value.

use std::collections::HashMap;

use godot::prelude::*;
use gs_sim_interp::{Config, Counts, Interp, Other, Value, Values};

/// A value the crate leaves opaque. A quaternion slerps and a colour lerps in the engine itself,
/// so they match the script's to the bit; everything else switches at the midpoint.
#[derive(Clone)]
struct Var(Variant);

/// Same type and same value, as the script's `==` compares a drawn dictionary.
impl PartialEq for Var {
    fn eq(&self, other: &Self) -> bool {
        self.0.get_type() == other.0.get_type() && self.0 == other.0
    }
}

impl Other for Var {
    fn same_kind(&self, other: &Self) -> bool {
        self.0.get_type() == other.0.get_type()
    }

    fn blend(&self, to: &Self, alpha: f64) -> Option<Self> {
        let method = match self.0.get_type() {
            VariantType::QUATERNION => "slerp",
            VariantType::COLOR => "lerp",
            _ => return None,
        };
        Some(Var(self
            .0
            .call(method, &[to.0.clone(), alpha.to_variant()])))
    }
}

/// One behaviour's part of a track's write plan, as the script's `_build_plan` makes it.
struct Part {
    id: InstanceId,
    size: usize,
    /// Interpolated properties in declaration order: interned key, name.
    props: Vec<(u32, StringName)>,
    /// Whether writes go through `_net_write_property` (overridden) rather than `set`.
    hook: bool,
}

#[derive(Default)]
struct Plan {
    parts: Vec<Part>,
    /// What the settled skip compares with: the last frame's draw, written in `frame`. Cleared by
    /// a push, whose snapshot hooks may have overwritten what was written.
    last: Option<Values<Var>>,
    frame: u64,
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimInterpolator {
    interp: Interp<Var, Plan>,
    keys: HashMap<StringName, u32>,
    names: Vec<StringName>,
    /// Script instance id -> whether it overrides `_net_write_property`.
    hooks: HashMap<i64, bool>,
    /// `set_settled_skip`: `apply_all` leaves an entity whose draw equals last frame's alone.
    settle: bool,
    /// `apply_all` calls so far, while `settle` is on.
    frame: u64,
    settled: i64,
    /// Behaviour instance id -> the copy that replaces its `_net_interpolated` (`bind_mirror`).
    mirrors: HashMap<InstanceId, Mirror>,
    mirrored: i64,
    /// `set_mirror_same_skip`: a mirror leaves the node alone when it already stands where the
    /// state says (on by default).
    #[init(val = true)]
    same_skip: bool,
}

/// `bind_mirror`: what a behaviour's `_net_interpolated` does, done here instead of a script call.
/// Each pair copies the behaviour's interpolated property onto a state object reached from `node`
/// through `chain`, then the state's `position` becomes the node's global position.
struct Mirror {
    node: InstanceId,
    chain: Vec<StringName>,
    /// Net property key and name, state property name.
    pairs: Vec<(u32, StringName, StringName)>,
    position: StringName,
}

fn to_value(v: &Variant) -> Value<Var> {
    match v.get_type() {
        VariantType::FLOAT => Value::Float(v.to::<f64>()),
        VariantType::VECTOR3 => {
            let p = v.to::<Vector3>();
            Value::Vec3([p.x, p.y, p.z])
        }
        VariantType::VECTOR2 => {
            let p = v.to::<Vector2>();
            Value::Vec2([p.x, p.y])
        }
        _ => Value::Other(Var(v.clone())),
    }
}

fn to_variant(v: &Value<Var>) -> Variant {
    match v {
        Value::Float(f) => f.to_variant(),
        Value::Vec3(p) => Vector3::new(p[0], p[1], p[2]).to_variant(),
        Value::Vec2(p) => Vector2::new(p[0], p[1]).to_variant(),
        Value::Other(o) => o.0.clone(),
    }
}

fn net_vars(behaviour: &Gd<Object>) -> Option<AnyArray> {
    behaviour.get("net_vars").try_to::<AnyArray>().ok()
}

impl GSSimInterpolator {
    fn key(&mut self, name: StringName) -> u32 {
        if let Some(k) = self.keys.get(&name) {
            return *k;
        }
        let k = self.names.len() as u32;
        self.names.push(name.clone());
        self.keys.insert(name, k);
        k
    }

    /// Whether `behaviour`'s script overrides `_net_write_property`: the method listed more
    /// than once, the script's own over its base's. A script that is not a GDScript is asked.
    fn hooked(&mut self, behaviour: &Gd<Object>) -> bool {
        let Some(script) = behaviour.get_script() else {
            return false;
        };
        let id = script.instance_id().to_i64();
        if let Some(h) = self.hooks.get(&id) {
            return *h;
        }
        let hook = !script.is_class("GDScript")
            || script
                .get_script_method_list()
                .iter_shared()
                .filter(|m| {
                    m.get("name")
                        .is_some_and(|n| n.to_string() == "_net_write_property")
                })
                .count()
                > 1;
        self.hooks.insert(id, hook);
        hook
    }

    fn build_plan(&mut self, behaviours: &[Gd<Object>]) -> Plan {
        let mut parts = Vec::with_capacity(behaviours.len());
        for behaviour in behaviours {
            let vars = net_vars(behaviour);
            let mut props = Vec::new();
            if let Some(vars) = &vars {
                for declaration in vars.iter_shared() {
                    let Ok(declaration) = declaration.try_to::<Gd<Object>>() else {
                        continue;
                    };
                    if declaration.get("interpolate").booleanize() {
                        let name = declaration.get("property").to::<StringName>();
                        props.push((self.key(name.clone()), name));
                    }
                }
            }
            parts.push(Part {
                id: behaviour.instance_id(),
                size: vars.map_or(0, |v| v.len()),
                props,
                hook: self.hooked(behaviour),
            });
        }
        Plan {
            parts,
            ..Plan::default()
        }
    }
}

impl GSSimInterpolator {
    /// `apply` for one entity, its config worked out by the caller. With `frame`, a draw equal to
    /// the one written the frame before (no push since) writes nothing and calls no hook.
    #[allow(clippy::too_many_arguments)]
    fn apply_one(
        &mut self,
        net_id: i64,
        behaviours: &AnyArray,
        tick: &Variant,
        render: f64,
        frame_elapsed: f64,
        cfg: &Config,
        frame: Option<u64>,
    ) -> bool {
        let Some(values) = self.interp.draw(net_id, render, frame_elapsed, cfg) else {
            return false;
        };
        let list: Vec<Gd<Object>> = behaviours
            .iter_shared()
            .filter_map(|b| b.try_to::<Gd<Object>>().ok())
            .collect();
        // Out of the track while hooks run, back in after.
        let mut plan = self
            .interp
            .plan_mut(net_id)
            .map(std::mem::take)
            .unwrap_or_default();
        if !plan_fits(&plan, &list) {
            plan = self.build_plan(&list);
        } else if let (Some(f), Some(last)) = (frame, &plan.last)
            && plan.frame + 1 == f
            && same_draw(last, &values)
        {
            plan.frame = f;
            self.settled += 1;
            if let Some(slot) = self.interp.plan_mut(net_id) {
                *slot = plan;
            }
            return true;
        }
        for (part, behaviour) in plan.parts.iter().zip(list.iter()) {
            let mut behaviour = behaviour.clone();
            let mut wrote = false;
            for (key, name) in &part.props {
                let Some((_, value)) = values.iter().find(|(k, _)| k == key) else {
                    continue;
                };
                let value = to_variant(value);
                if part.hook {
                    behaviour.call("_net_write_property", &[name.to_variant(), value]);
                } else {
                    behaviour.set(name, &value);
                }
                wrote = true;
            }
            if !wrote {
                continue;
            }
            if let Some(mirror) = self.mirrors.get(&part.id) {
                // The drawn value is what `set` stored; a hooked write may have changed it, so read back.
                let drawn = |key: u32| {
                    (!part.hook)
                        .then(|| values.iter().find(|(k, _)| *k == key))
                        .flatten()
                        .map(|(_, v)| to_variant(v))
                };
                mirror.run(&behaviour, drawn, self.same_skip);
                self.mirrored += 1;
            } else {
                behaviour.call("_net_interpolated", std::slice::from_ref(tick));
            }
        }
        plan.last = frame.map(|_| values);
        plan.frame = frame.unwrap_or(0);
        if let Some(slot) = self.interp.plan_mut(net_id) {
            *slot = plan;
        }
        true
    }

    /// `GSSimSnapshotBatch`'s push: decoded values in wire order, no dictionary between.
    pub(crate) fn push_list(&mut self, net_id: i64, tick: i64, values: Vec<(StringName, Variant)>) {
        let mut out: Values<Var> = Vec::with_capacity(values.len());
        for (name, v) in values {
            out.push((self.key(name), to_value(&v)));
        }
        self.interp.push(net_id, tick, out);
        if let Some(plan) = self.interp.plan_mut(net_id) {
            plan.last = None;
        }
    }

    /// Shared by the `push` func and `GSSimSnapshotBatch`.
    pub(crate) fn push_values(&mut self, net_id: i64, tick: i64, values: &VarDictionary) {
        let mut out: Values<Var> = Vec::with_capacity(values.len());
        for (k, v) in values.iter_shared() {
            // A dictionary finds a `String` key by the `StringName` the script looks up with.
            let name = match k.get_type() {
                VariantType::STRING_NAME => k.to::<StringName>(),
                VariantType::STRING => StringName::from(&k.to::<GString>()),
                _ => continue,
            };
            out.push((self.key(name), to_value(&v)));
        }
        self.interp.push(net_id, tick, out);
        if let Some(plan) = self.interp.plan_mut(net_id) {
            plan.last = None;
        }
    }
}

impl Mirror {
    /// FC's `_net_interpolated`, step for step: nothing without the node, the pairs in order,
    /// then `node.global_position = state.position`.
    ///
    /// With `same_skip`, a position equal to the node's own global position is not written again:
    /// a standing player's position repeats, and each write re-sends the move through every node
    /// under the player. The comparison reads the node itself, not a remembered last write, so a
    /// teleport, respawn or reparent done by anything else is never mistaken for "unchanged".
    fn run(&self, behaviour: &Gd<Object>, drawn: impl Fn(u32) -> Option<Variant>, same_skip: bool) {
        let Ok(mut node) = Gd::<Node3D>::try_from_instance_id(self.node) else {
            return;
        };
        let mut state: Gd<Object> = node.clone().upcast();
        for link in &self.chain {
            let Ok(next) = state.get(link).try_to::<Gd<Object>>() else {
                return;
            };
            state = next;
        }
        let mut position = None;
        for (key, net, field) in &self.pairs {
            let value = drawn(*key).unwrap_or_else(|| behaviour.get(net));
            state.set(field, &value);
            if *field == self.position {
                position = Some(value);
            }
        }
        // The state's typed property may have converted what was set; only a Vector3 passes as is.
        let position = match position {
            Some(v) if v.get_type() == VariantType::VECTOR3 => v,
            _ => state.get(&self.position),
        };
        if let Ok(p) = position.try_to::<Vector3>() {
            if same_skip && node.get_global_position() == p {
                return;
            }
            node.set_global_position(p);
        }
    }
}

/// The script's dictionary `==`: the same properties with the same values, in any order (a
/// merged retransmit reorders a sample's keys).
fn same_draw(a: &Values<Var>, b: &Values<Var>) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(k, v)| b.iter().any(|(bk, bv)| bk == k && bv == v))
}

fn plan_fits(plan: &Plan, behaviours: &[Gd<Object>]) -> bool {
    plan.parts.len() == behaviours.len()
        && plan.parts.iter().zip(behaviours).all(|(part, b)| {
            part.id == b.instance_id() && net_vars(b).map_or(0, |v| v.len()) == part.size
        })
}

#[godot_api]
impl GSSimInterpolator {
    /// `GSNetInterpolator.push`'s track half: a snapshot's values for an entity.
    #[func]
    fn push(&mut self, net_id: i64, tick: i64, values: VarDictionary) {
        self.push_values(net_id, tick, &values);
    }

    /// `GSNetInterpolator.push_repeat_batch`'s library half: for each id a sample at `tick`
    /// with its newest sample's values. Returns the ids it could not repeat (no track, or one
    /// already at `tick` or later), which the script pushes the values of.
    #[func]
    fn push_repeat(&mut self, net_ids: PackedInt64Array, tick: i64) -> PackedInt64Array {
        let mut missed = Vec::new();
        for &id in net_ids.as_slice() {
            if self.interp.push_repeat(id, tick) {
                if let Some(plan) = self.interp.plan_mut(id) {
                    plan.last = None;
                }
            } else {
                missed.push(id);
            }
        }
        PackedInt64Array::from(missed.as_slice())
    }

    /// `GSNetInterpolator.apply` past its predicted check: samples at `render` (the script's
    /// `server_tick - delay_ticks()`), eases a resume, and writes each behaviour's interpolated
    /// properties, then calls its `_net_interpolated(int(server_tick))`. Config is passed per
    /// call, read as the script reads it. Returns whether anything was sampled.
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn apply(
        &mut self,
        net_id: i64,
        behaviours: AnyArray,
        server_tick: f64,
        render: f64,
        frame_elapsed: f64,
        max_ticks: f64,
        ease_max: f64,
        ease_sec: f64,
    ) -> bool {
        let cfg = Config {
            max_ticks,
            ease_max,
            ease_sec,
        };
        let tick = (server_tick as i64).to_variant();
        self.apply_one(
            net_id,
            &behaviours,
            &tick,
            render,
            frame_elapsed,
            &cfg,
            None,
        )
    }

    /// `GSNetInterpolator.apply_frame` in one call: `apply` for each identity in `identities`,
    /// in order, skipping a null one and one whose `is_predicted()` is true, as the script's loop
    /// does. Returns how many were sampled.
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn apply_all(
        &mut self,
        identities: AnyArray,
        server_tick: f64,
        render: f64,
        frame_elapsed: f64,
        max_ticks: f64,
        ease_max: f64,
        ease_sec: f64,
    ) -> i64 {
        let cfg = Config {
            max_ticks,
            ease_max,
            ease_sec,
        };
        let tick = (server_tick as i64).to_variant();
        let net_id_name = StringName::from("net_id");
        let behaviours_name = StringName::from("behaviours");
        let mut sampled = 0;
        let frame = if self.settle {
            self.frame += 1;
            Some(self.frame)
        } else {
            None
        };
        for identity in identities.iter_shared() {
            let Ok(mut identity) = identity.try_to::<Gd<Object>>() else {
                continue;
            };
            if identity.call("is_predicted", &[]).booleanize() {
                continue;
            }
            let net_id = identity.get(&net_id_name).to::<i64>();
            let Ok(behaviours) = identity.get(&behaviours_name).try_to::<AnyArray>() else {
                continue;
            };
            if self.apply_one(
                net_id,
                &behaviours,
                &tick,
                render,
                frame_elapsed,
                &cfg,
                frame,
            ) {
                sampled += 1;
            }
        }
        sampled
    }

    /// `GSNetInterpolator`'s settled skip, off by default: in `apply_all`, an entity whose draw
    /// equals what was written the frame before, with no push since, is left alone (no writes,
    /// no `_net_interpolated`). For games whose hook only copies the interpolated values.
    #[func]
    fn set_settled_skip(&mut self, on: bool) {
        self.settle = on;
    }

    /// On by default: a bound mirror skips `node.global_position = position` when the node's
    /// global position already equals it (exact compare, read from the node). Off: always set,
    /// as the script's `_net_interpolated` did before.
    #[func]
    fn set_mirror_same_skip(&mut self, on: bool) {
        self.same_skip = on;
    }

    #[func]
    fn mirror_same_skip(&self) -> bool {
        self.same_skip
    }

    /// Entities `apply_all` left alone as settled since the last `reset_counts`.
    #[func]
    fn settled_count(&self) -> i64 {
        self.settled
    }

    /// `GSNetInterpolator.value_at`: `property` drawn at `render` between the bracketing
    /// samples, the newest past the end, null when unknown or older than the buffer, or with
    /// `cut` > 0 past a teleport. Counts nothing.
    #[func]
    fn value_at(&self, net_id: i64, property: StringName, render: f64, cut: f64) -> Variant {
        let Some(&key) = self.keys.get(&property) else {
            return Variant::nil();
        };
        self.interp
            .value_at(net_id, key, render, cut)
            .map_or_else(Variant::nil, |v| to_variant(&v))
    }

    /// `GSNetInterpolator.value_between`: `property` between the samples at exactly
    /// `older_tick` and `newer_tick`, at `render` (clamped). Counts nothing.
    #[func]
    fn value_between(
        &self,
        net_id: i64,
        property: StringName,
        older_tick: i64,
        newer_tick: i64,
        render: f64,
        cut: f64,
    ) -> Variant {
        let Some(&key) = self.keys.get(&property) else {
            return Variant::nil();
        };
        self.interp
            .value_between(net_id, key, older_tick, newer_tick, render, cut)
            .map_or_else(Variant::nil, |v| to_variant(&v))
    }

    /// `GSNetInterpTrack.samples` for `net_id`, oldest first: `{tick, values}` dictionaries, the
    /// values keyed by property name in the order the track holds them. Empty without a track.
    #[func]
    fn samples(&self, net_id: i64) -> VarArray {
        let mut out = VarArray::new();
        if let Some(track) = self.interp.track(net_id) {
            track.for_each_sample(|tick, values| {
                let mut dict = VarDictionary::new();
                for (k, v) in values {
                    dict.set(&self.names[*k as usize], &to_variant(v));
                }
                let mut sample = VarDictionary::new();
                sample.set("tick", tick);
                sample.set("values", &dict);
                out.push(&sample.to_variant());
            });
        }
        out
    }

    /// Samples held for `net_id` (0 when there is no track).
    #[func]
    fn track_len(&self, net_id: i64) -> i64 {
        self.interp.track(net_id).map_or(0, |t| t.len() as i64)
    }

    /// The newest sample's tick for `net_id`, -1 when it has none.
    #[func]
    fn newest_tick(&self, net_id: i64) -> i64 {
        self.interp
            .track(net_id)
            .and_then(|t| t.newest_tick())
            .unwrap_or(-1)
    }

    /// `GSNetInterpolator.sample` at `render`, counted as it counts: for a suite.
    #[func]
    fn sample(&mut self, net_id: i64, render: f64, max_ticks: f64) -> VarDictionary {
        let cfg = Config {
            max_ticks,
            ease_max: 0.0,
            ease_sec: 1.0,
        };
        let mut out = VarDictionary::new();
        for (k, v) in self.interp.sample(net_id, render, &cfg) {
            out.set(&self.names[k as usize], &to_variant(&v));
        }
        out
    }

    /// Replaces `behaviour`'s `_net_interpolated` with a copy done in `apply`/`apply_all`, for a
    /// hook that only copies interpolated values: after a frame's writes, each `pairs` entry
    /// (behaviour property -> state property) is copied onto the object reached from `node`
    /// through the `chain` properties, then `node.global_position` is set to the state's
    /// `position` property. Nothing happens once `node` is freed. Rebinding replaces.
    #[func]
    fn bind_mirror(
        &mut self,
        behaviour: Gd<Object>,
        node: Gd<Node3D>,
        chain: PackedStringArray,
        pairs: VarDictionary,
        position: StringName,
    ) {
        let mut list = Vec::with_capacity(pairs.len());
        for (k, v) in pairs.iter_shared() {
            let net = k.to::<StringName>();
            list.push((self.key(net.clone()), net, v.to::<StringName>()));
        }
        let mirror = Mirror {
            node: node.instance_id(),
            chain: chain.as_slice().iter().map(StringName::from).collect(),
            pairs: list,
            position,
        };
        self.mirrors.insert(behaviour.instance_id(), mirror);
    }

    /// Back to `_net_interpolated` for `behaviour`.
    #[func]
    fn unbind_mirror(&mut self, behaviour: Gd<Object>) {
        self.mirrors.remove(&behaviour.instance_id());
    }

    /// Behaviours bound with `bind_mirror`.
    #[func]
    fn mirror_count(&self) -> i64 {
        self.mirrors.len() as i64
    }

    /// Mirrors run (in place of `_net_interpolated`) since the last `reset_counts`.
    #[func]
    fn mirrored_count(&self) -> i64 {
        self.mirrored
    }

    #[func]
    fn forget(&mut self, net_id: i64) {
        self.interp.forget(net_id);
    }

    #[func]
    fn clear(&mut self) {
        self.interp.clear();
        self.mirrors.clear();
    }

    #[func]
    fn prune(&mut self, live_ids: PackedInt64Array) {
        self.interp.prune(live_ids.as_slice());
        self.mirrors
            .retain(|id, _| Gd::<Object>::try_from_instance_id(*id).is_ok());
    }

    #[func]
    fn track_count(&self) -> i64 {
        self.interp.track_count() as i64
    }

    #[func]
    fn sample_count(&self) -> i64 {
        self.interp.counts.samples
    }

    #[func]
    fn stall_count(&self) -> i64 {
        self.interp.counts.stalls
    }

    #[func]
    fn extrapolation_count(&self) -> i64 {
        self.interp.counts.extrapolations
    }

    #[func]
    fn reset_counts(&mut self) {
        self.interp.counts = Counts::default();
        self.settled = 0;
        self.mirrored = 0;
    }
}
