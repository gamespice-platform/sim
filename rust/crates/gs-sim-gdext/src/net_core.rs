//! `GSSimNetCore`: gs-net's `GSNetNativeCore` (the C++ snapshot core, ABI 5) as a gs-sim class,
//! method for method, so a `GSNetManager` given one builds byte-identical snapshots.
//!
//! The engine half lives here: reading behaviours and identities on the main thread (as the
//! C++ core's `_read` and `_identity_for`), the submit info, budgets and results. The build
//! itself (`Ctx::run_job`: dirty fields, deltas, interest, budget, parts) is the pure port in
//! `gs_sim_snapshot::net_core`, run per peer on scoped threads when `workers` asks for them.
//!
//! Values the core does not hold natively (strings, arrays, dictionaries) are kept as their
//! `var_to_bytes` serialisation, so two such values are equal when their bytes are.

use godot::classes::Object;
use godot::global::{bytes_to_var, var_to_bytes};
use godot::prelude::*;
use gs_sim_snapshot::net_core::{
    Beh, Ctx, Enc, Ident, InterestCfg, Job, NetDecl, Peer, PeerInterest, Pre, Val, VisTree, ty,
    val_equal, write_value,
};
use gs_sim_snapshot::{Writer, to_int};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[path = "net_core_batch.rs"]
mod batch;

const ABI: i64 = 5;
/// `GSNetVar.MAX_STRING_BYTES`.
const MAX_STRING_BYTES: usize = 1024;

/// The engine half of one behaviour (`BehRec`'s object id, properties and read state).
struct BehRec {
    obj_id: i64,
    props: Vec<StringName>,
    decl_count: i64,
    direct: bool,
    est_count: i64,
    est_for_owner: i64,
    seen: i64,
}

struct IdMeta {
    obj_id: i64,
    seen: i64,
}

/// One submitted peer: its build, the peer it builds for (out of the map until built), and the
/// GDScript dictionaries a job without a budget store resets entries of.
struct Pending {
    job: Job,
    peer: Option<Box<Peer>>,
    done: bool,
    dicts: Option<(VarDictionary, VarDictionary)>,
}

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct GSSimNetCore {
    base: Base<RefCounted>,
    behs: Vec<Option<Beh>>,
    brecs: Vec<Option<BehRec>>,
    beh_by_obj: HashMap<i64, usize>,
    free_behs: Vec<usize>,
    ids: Vec<Option<Ident>>,
    id_meta: Vec<Option<IdMeta>>,
    id_by_obj: HashMap<i64, usize>,
    free_ids: Vec<usize>,
    peers: HashMap<i64, Box<Peer>>,
    icfg: InterestCfg,
    all: Vec<usize>,
    by_net: HashMap<i64, usize>,
    serial: i64,
    snapshots: i64,
    tick: i64,
    now_ms: i64,
    mtu: i64,
    oldest_pending: i64,
    workers: i64,
    authority_shared: i64,
    read_mode: i64,
    open: bool,
    jobs: Vec<Pending>,
    taken: usize,
    read_usec: i64,
    build_usec: i64,
    wait_usec: i64,
    custom_reads: i64,
    read_mismatches: i64,
    unknown_relevant: i64,
    mismatch_logged: bool,
}

#[godot_api]
impl IRefCounted for GSSimNetCore {
    fn init(base: Base<RefCounted>) -> Self {
        GSSimNetCore {
            base,
            behs: Vec::new(),
            brecs: Vec::new(),
            beh_by_obj: HashMap::new(),
            free_behs: Vec::new(),
            ids: Vec::new(),
            id_meta: Vec::new(),
            id_by_obj: HashMap::new(),
            free_ids: Vec::new(),
            peers: HashMap::new(),
            icfg: InterestCfg::default(),
            all: Vec::new(),
            by_net: HashMap::new(),
            serial: 0,
            snapshots: 0,
            tick: 0,
            now_ms: 0,
            mtu: 1200,
            oldest_pending: 0,
            workers: 0,
            authority_shared: 1,
            read_mode: 1,
            open: false,
            jobs: Vec::new(),
            taken: 0,
            read_usec: 0,
            build_usec: 0,
            wait_usec: 0,
            custom_reads: 0,
            read_mismatches: 0,
            unknown_relevant: 0,
            mismatch_logged: false,
        }
    }
}

fn usec(since: Instant) -> i64 {
    since.elapsed().as_micros() as i64
}

fn is(v: &Variant, t: VariantType) -> bool {
    v.get_type() == t
}

/// `double(v)` for the numbers the C++ core reads.
fn num_f(v: &Variant) -> f64 {
    match v.get_type() {
        VariantType::FLOAT => v.try_to::<f64>().unwrap_or(0.0),
        VariantType::INT => v.try_to::<i64>().unwrap_or(0) as f64,
        VariantType::BOOL => f64::from(u8::from(v.booleanize())),
        _ => 0.0,
    }
}

/// `int64_t(v)`.
fn num_i(v: &Variant) -> i64 {
    match v.get_type() {
        VariantType::INT => v.try_to::<i64>().unwrap_or(0),
        VariantType::FLOAT => to_int(v.try_to::<f64>().unwrap_or(0.0)),
        VariantType::BOOL => i64::from(v.booleanize()),
        _ => 0,
    }
}

fn items(v: &Variant) -> Vec<Variant> {
    match v.try_to::<AnyArray>() {
        Ok(a) => (0..a.len()).map(|i| a.at(i)).collect(),
        Err(_) => Vec::new(),
    }
}

/// A packed or plain array of numbers as `PackedInt64Array(v)` gives them.
fn ints(v: &Variant) -> Vec<i64> {
    if let Ok(p) = v.try_to::<PackedInt64Array>() {
        return p.as_slice().to_vec();
    }
    if let Ok(p) = v.try_to::<PackedInt32Array>() {
        return p.as_slice().iter().map(|&x| i64::from(x)).collect();
    }
    items(v).iter().map(num_i).collect()
}

fn floats(v: &Variant) -> Vec<f64> {
    if let Ok(p) = v.try_to::<PackedFloat64Array>() {
        return p.as_slice().to_vec();
    }
    if let Ok(p) = v.try_to::<PackedFloat32Array>() {
        return p.as_slice().iter().map(|&x| f64::from(x)).collect();
    }
    items(v).iter().map(num_f).collect()
}

fn dict(v: &Variant) -> Option<VarDictionary> {
    if let Ok(d) = v.try_to::<VarDictionary>() {
        return Some(d);
    }
    v.try_to::<AnyDictionary>()
        .ok()
        .and_then(|d| d.try_cast_var_dictionary().ok())
}

fn object(v: &Variant) -> Option<Gd<Object>> {
    if !is(v, VariantType::OBJECT) {
        return None;
    }
    v.try_to::<Gd<Object>>().ok()
}

fn alive(id: i64) -> bool {
    InstanceId::try_from_i64(id).is_some_and(|i| Gd::<Object>::try_from_instance_id(i).is_ok())
}

fn string_name(v: &Variant) -> StringName {
    if let Ok(s) = v.try_to::<StringName>() {
        return s;
    }
    StringName::from(&v.stringify())
}

fn v3(v: &Variant) -> [f32; 3] {
    let x = v.try_to::<Vector3>().unwrap_or_default();
    [x.x, x.y, x.z]
}

/// `make_val`.
fn make_val(v: &Variant) -> Val {
    match v.get_type() {
        VariantType::NIL => Val::Nil,
        VariantType::BOOL => Val::Bool(v.booleanize()),
        VariantType::INT => Val::Int(v.try_to::<i64>().unwrap_or(0)),
        VariantType::FLOAT => Val::Float(v.try_to::<f64>().unwrap_or(0.0)),
        VariantType::VECTOR2 => {
            let x = v.try_to::<Vector2>().unwrap_or_default();
            Val::V2([x.x, x.y])
        }
        VariantType::VECTOR3 => Val::V3(v3(v)),
        VariantType::QUATERNION => {
            let q = v.try_to::<Quaternion>().unwrap_or_default();
            Val::Quat([q.x, q.y, q.z, q.w])
        }
        t => Val::Ext(
            t.ord() as i64,
            Arc::new(var_to_bytes(v).as_slice().to_vec()),
        ),
    }
}

/// `to_variant`.
fn to_variant(a: &Val) -> Variant {
    match a {
        Val::Nil => Variant::nil(),
        Val::Bool(b) => b.to_variant(),
        Val::Int(i) => i.to_variant(),
        Val::Float(f) => f.to_variant(),
        Val::V2(v) => Vector2::new(v[0], v[1]).to_variant(),
        Val::V3(v) => Vector3::new(v[0], v[1], v[2]).to_variant(),
        Val::Quat(q) => Quaternion::new(q[0], q[1], q[2], q[3]).to_variant(),
        Val::Ext(_, bytes) => bytes_to_var(&PackedByteArray::from(bytes.as_slice())),
    }
}

/// `GSNetVar._truncated_string`'s size: at most the cap, never inside a UTF-8 sequence.
fn truncated_string_size(b: &[u8]) -> usize {
    if b.len() <= MAX_STRING_BYTES {
        return b.len();
    }
    let mut n = MAX_STRING_BYTES;
    while n > 0 && (b[n - 1] & 0xC0) == 0x80 {
        n -= 1;
    }
    n
}

/// `native_enc`: the value as `GSNetVar.write` converts it, or None for one only GDScript can.
fn native_enc(d: &NetDecl, v: &Variant) -> Option<Enc> {
    let vt = v.get_type();
    match d.ty {
        ty::BOOL => match vt {
            VariantType::BOOL => Some(Enc::Int(i64::from(v.booleanize()))),
            VariantType::INT => Some(Enc::Int(i64::from(v.try_to::<i64>().unwrap_or(0) != 0))),
            VariantType::FLOAT => {
                Some(Enc::Int(i64::from(v.try_to::<f64>().unwrap_or(0.0) != 0.0)))
            }
            _ => None,
        },
        ty::UINT | ty::INT | ty::VARINT => match vt {
            VariantType::INT | VariantType::FLOAT | VariantType::BOOL => Some(Enc::Int(num_i(v))),
            _ => None,
        },
        ty::FLOAT | ty::FLOAT_RANGE | ty::ANGLE => match vt {
            VariantType::INT | VariantType::FLOAT | VariantType::BOOL => Some(Enc::Float(num_f(v))),
            _ => None,
        },
        ty::VEC3_POSITION | ty::VEC3_VELOCITY | ty::VEC3_RANGE | ty::DIRECTION => {
            (vt == VariantType::VECTOR3).then(|| Enc::V3(v3(v)))
        }
        ty::QUATERNION => (vt == VariantType::QUATERNION).then(|| {
            let q = v.try_to::<Quaternion>().unwrap_or_default();
            Enc::Quat([q.x, q.y, q.z, q.w])
        }),
        ty::STRING => {
            let s = if vt == VariantType::STRING {
                v.try_to::<GString>().unwrap_or_default().to_string()
            } else {
                v.stringify().to_string()
            };
            let b = s.as_bytes();
            Some(Enc::Bytes(Arc::new(b[..truncated_string_size(b)].to_vec())))
        }
        ty::BYTES => {
            if vt != VariantType::PACKED_BYTE_ARRAY {
                return None;
            }
            let b = v.try_to::<PackedByteArray>().unwrap_or_default();
            Some(Enc::Bytes(Arc::new(b.as_slice().to_vec())))
        }
        _ => None,
    }
}

/// `read_ring`: the ring's size at `at`, then that many ticks to the end.
fn read_ring(info: &[i64], at: usize) -> Option<Vec<i64>> {
    let n = *info.get(at)?;
    if n < 0 || info.len() as i64 != at as i64 + 1 + n {
        return None;
    }
    Some(info[at + 1..].to_vec())
}

fn job_from(info: &[i64]) -> Job {
    Job {
        peer_id: info[0],
        acked_tick: info[1],
        strict: info[2] != 0,
        compact: info[3] != 0,
        remaining: info[4],
        send_lead: info[5] != 0,
        lead_x: info[6],
        lead_y: info[7],
        ..Job::default()
    }
}

/// Every job, over up to `workers` scoped threads (serial on the web, which has none).
fn run_all(ctx: &Ctx, work: &mut [(&mut Job, &mut Peer)], workers: usize) {
    let threads = workers.min(work.len());
    #[cfg(not(target_family = "wasm"))]
    if threads > 1 {
        // Interleaved, so neighbouring (similar) peers spread over the threads.
        let mut lanes: Vec<Vec<&mut (&mut Job, &mut Peer)>> =
            (0..threads).map(|_| Vec::new()).collect();
        for (i, w) in work.iter_mut().enumerate() {
            lanes[i % threads].push(w);
        }
        std::thread::scope(|s| {
            for lane in lanes {
                s.spawn(move || {
                    for w in lane {
                        ctx.run_job(w.1, w.0);
                    }
                });
            }
        });
        return;
    }
    let _ = threads;
    for (job, peer) in work.iter_mut() {
        ctx.run_job(peer, job);
    }
}

impl GSSimNetCore {
    fn reset_views(&mut self, index: usize) {
        for peer in self.peers.values_mut() {
            peer.reset_view(index);
        }
    }

    /// `_adopt`.
    fn adopt(&mut self, bi: usize, obj: &mut Gd<Object>) {
        let spec = items(&obj.call("_native_spec", &[]));
        let rec = self.brecs[bi].as_mut().expect("behaviour record");
        let beh = self.behs[bi].as_mut().expect("behaviour");
        rec.props.clear();
        beh.decls.clear();
        rec.decl_count = -1;
        rec.direct = spec.len() >= 4 && spec[3].booleanize();
        if spec.len() >= 3 {
            rec.props = items(&spec[0]).iter().map(string_name).collect();
            beh.decls = items(&spec[1])
                .iter()
                .map(|row| NetDecl::from_row(&floats(row)))
                .collect();
            rec.decl_count = num_i(&spec[2]);
        }
        beh.nprops = rec.props.len();
        beh.reads = None;
        self.reset_views(bi);
        let me = self.to_gd().to_variant();
        obj.set("_native_core", &me);
    }

    /// `_direct_values`.
    fn direct_values(&mut self, bi: usize, obj: &mut Gd<Object>, owner: i64) -> Vec<Variant> {
        let props = self.brecs[bi]
            .as_ref()
            .expect("behaviour record")
            .props
            .clone();
        let mut out: Vec<Variant> = props.iter().map(|p| obj.get(p)).collect();
        let held = obj.get("_native_core");
        let count = if object(&held).is_some() {
            items(&obj.get("net_vars")).len() as i64
        } else {
            -1
        };
        let rec = self.brecs[bi].as_mut().expect("behaviour record");
        let beh = self.behs[bi].as_mut().expect("behaviour");
        if rec.est_count != count || rec.est_for_owner != owner {
            rec.est_count = count;
            rec.est_for_owner = owner;
            beh.est_owner = num_i(&obj.call("estimated_bits", &[owner.to_variant()]));
            beh.est_other = num_i(&obj.call("estimated_bits", &[(owner + 1).to_variant()]));
        }
        out.push(beh.est_owner.to_variant());
        out.push(beh.est_other.to_variant());
        out.push(count.to_variant());
        out
    }

    fn mismatch(&mut self, what: &str, obj: &Gd<Object>) {
        self.read_mismatches += 1;
        if !self.mismatch_logged {
            self.mismatch_logged = true;
            godot_error!(
                "GSSimNetCore: direct {} differs from GDScript's for {} (native_reads verify; GDScript's sent)",
                what,
                obj.get_class()
            );
        }
    }

    /// `_values`.
    fn values(&mut self, bi: usize, obj: &mut Gd<Object>, owner: i64) -> Vec<Variant> {
        let direct = self.brecs[bi].as_ref().is_some_and(|r| r.direct);
        if self.read_mode == 0 || !direct {
            return items(&obj.call("_native_values", &[]));
        }
        let d = self.direct_values(bi, obj, owner);
        if self.read_mode == 1 {
            return d;
        }
        let s = items(&obj.call("_native_values", &[]));
        if d.len() != s.len() || d.iter().zip(&s).any(|(a, b)| a != b) {
            self.mismatch("values", obj);
            return s;
        }
        d
    }

    /// `_position_of`.
    fn position_of(&mut self, obj: &mut Gd<Object>) -> [f32; 3] {
        if self.read_mode == 0 || !obj.get("_position_frozen").booleanize() {
            return v3(&obj.call("world_position", &[]));
        }
        let pos = v3(&obj.get("_frozen_position"));
        if self.read_mode == 2 {
            let scripted = v3(&obj.call("world_position", &[]));
            if pos.map(f32::to_bits) != scripted.map(f32::to_bits) {
                self.mismatch("position", obj);
                return scripted;
            }
        }
        pos
    }

    /// `_read`.
    fn read(&mut self, bi: usize, obj: &mut Gd<Object>, owner: i64) {
        let mut values = self.values(bi, obj, owner);
        let mut n = self.brecs[bi]
            .as_ref()
            .expect("behaviour record")
            .props
            .len();
        let decl_count = self.brecs[bi]
            .as_ref()
            .expect("behaviour record")
            .decl_count;
        if values.len() != n + 3 || num_i(&values[n + 2]) != decl_count {
            self.adopt(bi, obj);
            values = self.values(bi, obj, owner);
            n = self.brecs[bi]
                .as_ref()
                .expect("behaviour record")
                .props
                .len();
            values.resize(n + 3, Variant::nil());
        }
        let reads: Vec<Val> = values[..n].iter().map(make_val).collect();
        let beh = self.behs[bi].as_mut().expect("behaviour");
        beh.est_owner = num_i(&values[n]);
        beh.est_other = num_i(&values[n + 1]);
        let same = beh.reads.as_ref().is_some_and(|old| {
            old.len() == n && old.iter().zip(&reads).all(|(a, b)| val_equal(a, b))
        });
        if !same {
            beh.reads = Some(Arc::new(reads));
        }
        let decls = beh.decls.clone();
        let mut enc = Vec::with_capacity(decls.len());
        for (i, d) in decls.iter().enumerate() {
            let value = values.get(d.slot).cloned().unwrap_or_default();
            if let Some(e) = native_enc(d, &value) {
                enc.push(e);
                continue;
            }
            self.custom_reads += 1;
            let pads = if d.may_align() { 8 } else { 1 };
            let out = items(&obj.call(
                "_native_encode",
                &[(i as i64).to_variant(), value, (pads as i64).to_variant()],
            ));
            let mut pre = Pre {
                pads,
                data: vec![Vec::new(); pads],
                bits: vec![0; pads],
            };
            for p in 0..pads {
                if 2 * p + 1 >= out.len() {
                    break;
                }
                let mut bytes = out[2 * p]
                    .try_to::<PackedByteArray>()
                    .unwrap_or_default()
                    .as_slice()
                    .to_vec();
                bytes.resize(bytes.len() + 8, 0);
                pre.data[p] = bytes;
                pre.bits[p] = num_i(&out[2 * p + 1]);
            }
            enc.push(Enc::Pre(Arc::new(pre)));
        }
        self.behs[bi].as_mut().expect("behaviour").enc = enc;
    }

    /// `_behaviour_for`.
    fn behaviour_for(&mut self, obj: &mut Gd<Object>, owner: i64) -> usize {
        let id = obj.instance_id().to_i64();
        let bi = match self.beh_by_obj.get(&id) {
            Some(&bi) => bi,
            None => {
                let bi = self.free_behs.pop().unwrap_or_else(|| {
                    self.behs.push(None);
                    self.brecs.push(None);
                    self.behs.len() - 1
                });
                self.behs[bi] = Some(Beh {
                    index: bi,
                    ..Beh::default()
                });
                self.brecs[bi] = Some(BehRec {
                    obj_id: id,
                    props: Vec::new(),
                    decl_count: -1,
                    direct: false,
                    est_count: i64::MIN,
                    est_for_owner: i64::MIN,
                    seen: 0,
                });
                self.beh_by_obj.insert(id, bi);
                self.adopt(bi, obj);
                bi
            }
        };
        let rec = self.brecs[bi].as_mut().expect("behaviour record");
        if rec.seen != self.serial {
            rec.seen = self.serial;
            self.read(bi, obj, owner);
        }
        bi
    }

    /// `_identity_for`.
    fn identity_for(&mut self, v: &Variant) -> Option<usize> {
        let mut obj = object(v)?;
        let id = obj.instance_id().to_i64();
        let slot = match self.id_by_obj.get(&id) {
            Some(&s) => s,
            None => {
                let s = self.free_ids.pop().unwrap_or_else(|| {
                    self.ids.push(None);
                    self.id_meta.push(None);
                    self.ids.len() - 1
                });
                self.ids[s] = Some(Ident::default());
                self.id_meta[s] = Some(IdMeta {
                    obj_id: id,
                    seen: 0,
                });
                self.id_by_obj.insert(id, s);
                s
            }
        };
        let meta = self.id_meta[slot].as_mut().expect("identity meta");
        if meta.seen == self.serial {
            return Some(slot);
        }
        meta.seen = self.serial;
        let mut rec = std::mem::take(self.ids[slot].as_mut().expect("identity"));
        rec.net_id = num_i(&obj.get("net_id"));
        rec.owner = num_i(&obj.get("owner_peer_id"));
        rec.shared = num_i(&obj.get("authority")) == self.authority_shared;
        if self.icfg.on {
            rec.priority = num_f(&obj.get("priority"));
            rec.always = obj.get("always_relevant").booleanize();
            rec.linger_sec = num_f(&obj.get("interest_linger_sec"));
            rec.pos = self.position_of(&mut obj);
            rec.cluster = self.icfg.vis.as_ref().map_or(-1, |t| t.cluster_of(rec.pos));
            let tags: Vec<String> = obj
                .get("interest_tags")
                .try_to::<PackedStringArray>()
                .map(|t| t.as_slice().iter().map(|s| s.to_string()).collect())
                .unwrap_or_default();
            rec.radius = self.icfg.radius;
            rec.score_limit = self.icfg.radius.max(1.0);
            rec.far_dist = self.icfg.far_distance;
            let mut found_radius = false;
            let mut found_far = false;
            for tag in &tags {
                if found_radius && found_far {
                    break;
                }
                if !found_radius
                    && let Some((_, r)) = self.icfg.radius_by_tag.iter().find(|(k, _)| k == tag)
                {
                    rec.radius = *r;
                    rec.score_limit = r.max(1.0);
                    found_radius = true;
                }
                if !found_far
                    && let Some((_, f)) = self.icfg.far_by_tag.iter().find(|(k, _)| k == tag)
                {
                    rec.far_dist = *f;
                    found_far = true;
                }
            }
        }
        rec.behs.clear();
        for b in items(&obj.get("behaviours")) {
            let Some(mut bo) = object(&b) else {
                continue;
            };
            let bi = self.behaviour_for(&mut bo, rec.owner);
            self.behs[bi].as_mut().expect("behaviour").owner = rec.owner;
            rec.behs.push(bi);
        }
        self.ids[slot] = Some(rec);
        Some(slot)
    }

    /// `_known_identity`.
    fn known_identity(&self, v: &Variant) -> Option<usize> {
        let obj = object(v)?;
        let s = *self.id_by_obj.get(&obj.instance_id().to_i64())?;
        (self.id_meta[s].as_ref()?.seen == self.serial).then_some(s)
    }

    fn drop_behaviour(&mut self, bi: usize) {
        self.reset_views(bi);
        if let Some(rec) = self.brecs[bi].take() {
            self.beh_by_obj.remove(&rec.obj_id);
        }
        self.behs[bi] = None;
        self.free_behs.push(bi);
    }

    /// `_sweep`.
    fn sweep(&mut self) {
        let dead: Vec<usize> = (0..self.brecs.len())
            .filter(|&i| {
                self.brecs[i]
                    .as_ref()
                    .is_some_and(|r| r.seen != self.serial && !alive(r.obj_id))
            })
            .collect();
        for bi in dead {
            self.drop_behaviour(bi);
        }
        for s in 0..self.id_meta.len() {
            let gone = self.id_meta[s]
                .as_ref()
                .is_some_and(|m| m.seen != self.serial && !alive(m.obj_id));
            if gone {
                let m = self.id_meta[s].take().expect("identity meta");
                self.id_by_obj.remove(&m.obj_id);
                self.ids[s] = None;
                self.free_ids.push(s);
            }
        }
    }

    /// The peer for a new job, out of the map; one an unbuilt job holds is built first.
    fn take_peer(&mut self, peer_id: i64) -> Box<Peer> {
        if !self.peers.contains_key(&peer_id)
            && self
                .jobs
                .iter()
                .any(|p| p.peer.as_ref().is_some_and(|q| q.id == peer_id))
        {
            self.run_pending();
        }
        self.peers.remove(&peer_id).unwrap_or_else(|| {
            Box::new(Peer {
                id: peer_id,
                ..Peer::default()
            })
        })
    }

    fn queue(&mut self, job: Job, peer: Box<Peer>, dicts: Option<(VarDictionary, VarDictionary)>) {
        self.jobs.push(Pending {
            job,
            peer: Some(peer),
            done: false,
            dicts,
        });
        if self.workers == 0 {
            self.run_pending();
        }
    }

    /// Builds every submitted, unbuilt job, then hands their peers back and applies their resets.
    fn run_pending(&mut self) {
        let mut jobs = std::mem::take(&mut self.jobs);
        let started = Instant::now();
        {
            let ctx = Ctx {
                behs: &self.behs,
                ids: &self.ids,
                all: &self.all,
                by_net: &self.by_net,
                icfg: &self.icfg,
                tick: self.tick,
                now_ms: self.now_ms,
                mtu: self.mtu,
                oldest_pending: self.oldest_pending,
            };
            let mut work: Vec<(&mut Job, &mut Peer)> = jobs
                .iter_mut()
                .filter(|p| !p.done)
                .filter_map(|p| {
                    let Pending { job, peer, .. } = p;
                    Some((job, peer.as_deref_mut()?))
                })
                .collect();
            run_all(&ctx, &mut work, self.workers.max(0) as usize);
        }
        self.build_usec += usec(started);
        for p in jobs.iter_mut().filter(|p| !p.done) {
            p.done = true;
            if let Some(peer) = p.peer.take() {
                self.peers.insert(peer.id, peer);
            }
            if let Some((acc, starved)) = p.dicts.as_mut() {
                for &id in &p.job.resets {
                    acc.set(id, 0.0);
                    starved.set(id, 0);
                }
            }
        }
        self.jobs = jobs;
    }

    /// `_result`.
    fn result(job: &Job) -> VarArray {
        let mut payloads = VarArray::new();
        for p in &job.payloads {
            payloads.push(&PackedByteArray::from(p.as_slice()).to_variant());
        }
        let mut r = VarArray::new();
        r.push(&job.peer_id.to_variant());
        r.push(&payloads.to_variant());
        r.push(&PackedInt32Array::from(job.counts.as_slice()).to_variant());
        r.push(&job.starved.to_variant());
        r.push(&job.spent.to_variant());
        r.push(&job.split.to_variant());
        r.push(&job.unchanged_listed.to_variant());
        r.push(&job.unchanged_saved.to_variant());
        r.push(&job.leads_sent.to_variant());
        r.push(&job.oversize_id.to_variant());
        r.push(&job.oversize_bytes.to_variant());
        r.push(&job.cap_count.to_variant());
        r
    }

    fn load_budget(peer: &mut Peer, acc: &VarDictionary, starved: &VarDictionary) -> bool {
        let store = &mut peer.budget;
        store.clear();
        store.loaded = true;
        let mut ok = true;
        let (keys, values) = (acc.keys_array(), acc.values_array());
        for i in 0..keys.len() {
            let k = keys.at(i);
            if !is(&k, VariantType::INT) {
                ok = false;
                continue;
            }
            let s = store.slot(num_i(&k));
            store.set_acc(s, num_f(&values.at(i)));
        }
        let (keys, values) = (starved.keys_array(), starved.values_array());
        for i in 0..keys.len() {
            let k = keys.at(i);
            if !is(&k, VariantType::INT) {
                ok = false;
                continue;
            }
            let s = store.slot(num_i(&k));
            store.set_starved(s, num_i(&values.at(i)));
        }
        ok
    }

    /// `_vis_tree`: [param kept] when its serial is the one already copied.
    fn vis_tree(d: &VarDictionary, kept: &Option<Arc<VisTree>>) -> Option<Arc<VisTree>> {
        let get = |k: &str| d.get(k).unwrap_or_default();
        let serial = num_i(&get("serial"));
        if let Some(k) = kept
            && k.serial == serial
        {
            return Some(k.clone());
        }
        let bounds = get("bounds").try_to::<Aabb>().unwrap_or_default();
        let int32s = |k: &str| {
            ints(&get(k))
                .iter()
                .map(|&x| x as i32)
                .collect::<Vec<i32>>()
        };
        let g = VisTree {
            serial,
            planes: floats(&get("planes")).iter().map(|&x| x as f32).collect(),
            nodes: int32s("nodes"),
            leaf_cluster: int32s("leaf_cluster"),
            head: num_i(&get("head")) as i32,
            row_bytes: num_i(&get("row_bytes")),
            clusters: num_i(&get("clusters")),
            bounded: bounds.has_volume(),
            lo: [bounds.position.x, bounds.position.y, bounds.position.z],
            hi: {
                let e = bounds.position + bounds.size;
                [e.x, e.y, e.z]
            },
            rows: get("rows")
                .try_to::<PackedByteArray>()
                .unwrap_or_default()
                .as_slice()
                .to_vec(),
        };
        if !g.valid() {
            godot_error!("GSSimNetCore.set_interest: a malformed vis tree, ignored.");
            return None;
        }
        Some(Arc::new(g))
    }

    fn rec_index(&self, behaviour: &Variant) -> Option<usize> {
        let obj = object(behaviour)?;
        self.beh_by_obj.get(&obj.instance_id().to_i64()).copied()
    }

    /// `_view_dict`.
    fn view_dict(&self, bi: usize, peer_id: i64) -> Option<VarDictionary> {
        let rec = self.brecs[bi].as_ref()?;
        let view = self.peers.get(&peer_id)?.existing_view(bi)?;
        if view.is_fresh() {
            return None;
        }
        let prop = |s: usize| rec.props.get(s).cloned().unwrap_or_default();
        let values = |vals: &[Val], has: u64| {
            let mut d = VarDictionary::new();
            for s in (0..64).filter(|s| has & (1u64 << s) != 0) {
                if let Some(v) = vals.get(s) {
                    d.set(&prop(s).to_variant(), &to_variant(v));
                }
            }
            d
        };
        let mut out = VarDictionary::new();
        out.set("acked", &values(&view.acked, view.acked_has).to_variant());
        out.set(
            "believed",
            &values(&view.believed, view.believed_has).to_variant(),
        );
        let mut entries = VarArray::new();
        for e in &view.pending {
            let mut entry = VarArray::new();
            entry.push(&e.tick.to_variant());
            entry.push(&values(&e.reads, e.mask).to_variant());
            entries.push(&entry.to_variant());
        }
        out.set("pending", &entries.to_variant());
        let mut sent = VarDictionary::new();
        for s in (0..64).filter(|s| view.sent_has & (1u64 << s) != 0) {
            sent.set(
                &prop(s).to_variant(),
                &view.last_sent.get(s).copied().unwrap_or(0).to_variant(),
            );
        }
        out.set("last_sent_ms", &sent.to_variant());
        out.set("last_ack", &view.last_ack.to_variant());
        let mut relost = VarDictionary::new();
        for s in (0..64).filter(|s| view.relost & (1u64 << s) != 0) {
            relost.set(&prop(s).to_variant(), &true.to_variant());
        }
        out.set("relost", &relost.to_variant());
        out.set("strict", &view.strict.to_variant());
        Some(out)
    }
}

#[godot_api]
impl GSSimNetCore {
    #[func]
    fn abi_version() -> i64 {
        ABI
    }

    #[func]
    fn visibility_supported() -> bool {
        true
    }

    #[func]
    fn begin_snapshot(&mut self, identities: Variant, params: Variant) {
        if self.open {
            self.finish_snapshot();
        }
        let started = Instant::now();
        let p = ints(&params);
        let at = |i: usize, d: i64| p.get(i).copied().unwrap_or(d);
        self.serial += 1;
        self.snapshots += 1;
        self.tick = at(0, 0);
        self.now_ms = at(1, 0);
        self.mtu = at(2, 1200);
        self.oldest_pending = at(3, 0);
        let workers = at(4, 0);
        self.authority_shared = at(5, 1);
        let mut read_mode = at(6, 1);
        if !(0..=2).contains(&read_mode) {
            godot_error!(
                "GSSimNetCore: read mode {} is not 0, 1 or 2; GDScript reads used",
                read_mode
            );
            read_mode = 0;
        }
        self.read_mode = read_mode;
        self.all.clear();
        self.by_net.clear();
        for identity in items(&identities) {
            let slot = self.identity_for(&identity);
            if self.icfg.on
                && let Some(s) = slot
            {
                self.all.push(s);
                let net_id = self.ids[s].as_ref().expect("identity").net_id;
                self.by_net.insert(net_id, s);
            }
        }
        self.sweep();
        self.workers = workers.clamp(0, 64);
        self.jobs.clear();
        self.taken = 0;
        self.open = true;
        self.build_usec = 0;
        self.wait_usec = 0;
        self.read_usec = usec(started);
    }

    #[func]
    fn submit_peer(
        &mut self,
        relevant: Variant,
        info: Variant,
        accumulators: Variant,
        starved_ticks: Variant,
    ) {
        if !self.open {
            godot_error!("GSSimNetCore.submit_peer outside begin_snapshot/finish_snapshot.");
            return;
        }
        let info = ints(&info);
        if info.len() < 9 {
            godot_error!("GSSimNetCore.submit_peer: info needs the acked ring size (ABI 4).");
            return;
        }
        let mut job = job_from(&info);
        let Some(ring) = read_ring(&info, 8) else {
            godot_error!("GSSimNetCore.submit_peer: the acked ring's size does not match info.");
            return;
        };
        job.ring = ring;
        for r in items(&relevant) {
            match self.known_identity(&r) {
                Some(s) => job.relevant.push(s),
                None => self.unknown_relevant += 1,
            }
        }
        let dicts = dict(&accumulators).zip(dict(&starved_ticks));
        let peer = self.take_peer(job.peer_id);
        self.queue(job, peer, dicts);
    }

    #[func]
    fn submit_peer_interest(
        &mut self,
        observer: Variant,
        info: Variant,
        accumulators: Variant,
        starved_ticks: Variant,
    ) {
        let info = ints(&info);
        self.submit_interest(&observer, &info, &accumulators, &starved_ticks);
    }

    #[func]
    fn set_interest(&mut self, cfg: Variant) {
        if self.open {
            godot_error!("GSSimNetCore.set_interest inside a snapshot.");
            return;
        }
        let cfg = dict(&cfg).unwrap_or_default();
        let get = |k: &str| cfg.get(k);
        let f = |k: &str, d: f64| get(k).map_or(d, |v| num_f(&v));
        let i = |k: &str, d: i64| get(k).map_or(d, |v| num_i(&v));
        let mut c = InterestCfg {
            on: get("on").is_some_and(|v| v.booleanize()),
            ..InterestCfg::default()
        };
        if c.on {
            c.radius = f("radius", 200.0);
            let pairs = |k: &str| -> Vec<(String, f64)> {
                let Some(d) = get(k).as_ref().and_then(dict) else {
                    return Vec::new();
                };
                let (keys, values) = (d.keys_array(), d.values_array());
                (0..keys.len())
                    .filter_map(|j| {
                        let key = keys.at(j);
                        (is(&key, VariantType::STRING) || is(&key, VariantType::STRING_NAME))
                            .then(|| (key.stringify().to_string(), num_f(&values.at(j))))
                    })
                    .collect()
            };
            c.radius_by_tag = pairs("radius_by_tag");
            c.far_by_tag = pairs("far_by_tag");
            c.interval_sec = f("interval", 0.25);
            c.far_every = i("far_every", 1);
            c.far_distance = f("far_distance", 0.0);
            c.far_coherent = get("far_coherent").is_some_and(|v| v.booleanize());
            c.far_tiers = i("far_tiers", 1);
            let cone = f("aim_cone_deg", 0.0);
            c.aim_cos = if cone > 0.0 {
                (cone * (std::f64::consts::PI / 180.0)).cos()
            } else {
                1.0
            };
            c.ticks_per_snapshot = i("ticks_per_snapshot", 1).max(1);
            c.cap = i("cap", 0);
            c.send_budget = i("send_budget", 0);
            c.starvation_ticks = i("starvation_ticks", 10);
            c.starvation_weight = f("starvation_weight", 1.0);
            c.invalidation = i("invalidation", 0);
            if let Some(vis) = get("vis").as_ref().and_then(dict)
                && !vis.is_empty()
            {
                c.vis = Self::vis_tree(&vis, &self.icfg.vis);
                c.vis_audible = f("vis_audible", 0.0);
            }
            if c.invalidation != self.icfg.invalidation {
                for p in self.peers.values_mut() {
                    p.interest = PeerInterest::default();
                }
            }
        } else {
            c.invalidation = self.icfg.invalidation;
        }
        self.icfg = c;
    }

    #[func]
    fn expire_interest(&mut self) {
        if self.open {
            godot_error!("GSSimNetCore.expire_interest inside a snapshot.");
            return;
        }
        for p in self.peers.values_mut() {
            p.interest.at_ms = 0;
        }
    }

    #[func]
    fn take_ready(&mut self) -> VarArray {
        let mut out = VarArray::new();
        if !self.open {
            godot_error!("GSSimNetCore.take_ready outside begin_snapshot/finish_snapshot.");
            return out;
        }
        let started = Instant::now();
        self.run_pending();
        while self.taken < self.jobs.len() && self.jobs[self.taken].done {
            out.push(&Self::result(&self.jobs[self.taken].job).to_variant());
            self.taken += 1;
        }
        self.wait_usec += usec(started);
        out
    }

    #[func]
    fn finish_snapshot(&mut self) -> VarArray {
        let mut out = VarArray::new();
        if !self.open {
            return out;
        }
        let started = Instant::now();
        self.run_pending();
        for p in &self.jobs[self.taken..] {
            out.push(&Self::result(&p.job).to_variant());
        }
        self.jobs.clear();
        self.taken = 0;
        self.wait_usec += usec(started);
        self.open = false;
        out
    }

    #[func]
    fn forget_peer(&mut self, peer_id: i64) {
        if self.open {
            godot_error!("GSSimNetCore.forget_peer inside a snapshot.");
            return;
        }
        self.peers.remove(&peer_id);
    }

    #[func]
    fn export_budget(&mut self, peer_id: i64, accumulators: Variant, starved_ticks: Variant) {
        if self.open {
            godot_error!(
                "GSSimNetCore.export_budget inside a snapshot: the build threads are writing it."
            );
            return;
        }
        let Some(peer) = self.peers.get(&peer_id).filter(|p| p.budget.loaded) else {
            godot_error!(
                "GSSimNetCore.export_budget: that peer's budget was never loaded here (forgotten, or another core holds it)."
            );
            return;
        };
        let (Some(mut acc), Some(mut starved)) = (dict(&accumulators), dict(&starved_ticks)) else {
            godot_error!(
                "GSSimNetCore.export_budget: accumulators and starved_ticks must be Dictionaries."
            );
            return;
        };
        let store = &peer.budget;
        acc.clear();
        for &s in &store.acc_order {
            acc.set(store.ids[s], store.acc_at(s));
        }
        starved.clear();
        for &s in &store.starved_order {
            starved.set(store.ids[s], store.starved_at(s));
        }
    }

    #[func]
    fn forget_view(&mut self, behaviour: Variant, peer_id: i64) {
        if self.open {
            godot_error!("GSSimNetCore.forget_view inside a snapshot.");
            return;
        }
        let Some(bi) = self.rec_index(&behaviour) else {
            return;
        };
        for (id, p) in self.peers.iter_mut() {
            if peer_id < 0 || *id == peer_id {
                p.reset_view(bi);
            }
        }
    }

    #[func]
    fn view_snapshot(&self, behaviour: Variant, peer_id: i64) -> VarDictionary {
        self.rec_index(&behaviour)
            .and_then(|bi| self.view_dict(bi, peer_id))
            .unwrap_or_default()
    }

    #[func]
    fn view_state(&self, behaviour: Variant, peer_id: i64) -> VarDictionary {
        let mut out = VarDictionary::new();
        let Some(v) = self
            .rec_index(&behaviour)
            .and_then(|bi| self.peers.get(&peer_id)?.existing_view(bi))
        else {
            return out;
        };
        out.set("acked", i64::from(v.acked_has.count_ones()));
        out.set("believed", i64::from(v.believed_has.count_ones()));
        out.set("pending", v.pending.len() as i64);
        out.set("last_ack", v.last_ack);
        out.set("strict", v.strict);
        out
    }

    #[func]
    fn encode_value(&self, spec: Variant, value: Variant, pad: i64) -> VarArray {
        let mut out = VarArray::new();
        let d = NetDecl::from_row(&floats(&spec));
        let Some(e) = native_enc(&d, &value) else {
            return out;
        };
        let mut w = Writer::new();
        let p = pad & 7;
        if p > 0 {
            w.write_bits(0, p);
        }
        write_value(&mut w, &d, &e);
        out.push(&PackedByteArray::from(w.bytes.as_slice()).to_variant());
        out.push(&(w.bit_length() as i64 - p).to_variant());
        out
    }

    #[func]
    fn describe(&self) -> VarDictionary {
        let views: i64 = self
            .peers
            .values()
            .map(|p| p.views.iter().filter(|v| v.is_some()).count() as i64)
            .sum();
        let mut out = VarDictionary::new();
        out.set("abi", ABI);
        out.set("behaviours", self.beh_by_obj.len() as i64);
        out.set("identities", self.id_by_obj.len() as i64);
        out.set("peers", self.peers.len() as i64);
        out.set("views", views);
        out.set("workers", self.workers);
        out.set("snapshots", self.snapshots);
        out.set("read_usec", self.read_usec);
        out.set("build_usec", self.build_usec);
        out.set("wait_usec", self.wait_usec);
        out.set("gdscript_encodes", self.custom_reads);
        out.set("read_mode", self.read_mode);
        out.set("read_mismatches", self.read_mismatches);
        out.set("unknown_relevant", self.unknown_relevant);
        out.set("interest", self.icfg.on);
        out
    }
}
