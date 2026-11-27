//! gs-net's `GSNetNativeCore` build, transcribed: the per-peer views (acked, believed, pending),
//! which fields are owed, the part layout with the unchanged tail and the lead, interest (distance,
//! the map's visibility, thinning, the cap) and the budget's ordering. Same bytes as gs-net's C++
//! core, which gives the same bytes as its GDScript send path.
//!
//! Engine values are converted by the caller (`gs-sim-gdext`'s `GSSimNetCore`): a value this crate
//! cannot hold natively is a [`Val::Ext`] with the engine's serialised bytes, compared by them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::{Writer, to_int};

pub const ID_BITS: i64 = 24;
pub const MAX_PARTS: usize = 16;
pub const TRAILER_BITS: i64 = 8;
pub const COUNT_BYTE: usize = 4;
pub const TAIL_MARK: i64 = 0x0F;
pub const LEAD_FIELD_BITS: i64 = 12;
pub const LEAD_BITS: i64 = 24;
pub const AUD_OWNER: i64 = 1;
pub const AUD_OBSERVERS: i64 = 2;

/// `GSNetVar.Type`.
pub mod ty {
    pub const BOOL: i64 = 0;
    pub const UINT: i64 = 1;
    pub const INT: i64 = 2;
    pub const VARINT: i64 = 3;
    pub const FLOAT: i64 = 4;
    pub const FLOAT_RANGE: i64 = 5;
    pub const VEC3_POSITION: i64 = 6;
    pub const VEC3_VELOCITY: i64 = 7;
    pub const VEC3_RANGE: i64 = 8;
    pub const DIRECTION: i64 = 9;
    pub const QUATERNION: i64 = 10;
    pub const ANGLE: i64 = 11;
    pub const STRING: i64 = 12;
    pub const BYTES: i64 = 13;
    pub const CUSTOM: i64 = 14;
}

/// One `GSNetVar` as `GSNetBehaviour._native_spec` describes it (the C++ core's `Decl`).
#[derive(Clone, Debug, PartialEq)]
pub struct NetDecl {
    pub ty: i64,
    pub audience: i64,
    pub max_rate: f64,
    pub epsilon: f64,
    pub on_spawn_only: bool,
    pub bit_width: i64,
    pub min_value: f64,
    pub max_value: f64,
    pub extent: f64,
    pub position_bits: i64,
    pub max_speed: f64,
    pub velocity_bits: i64,
    pub rotation_bits: i64,
    pub slot: usize,
}

impl Default for NetDecl {
    fn default() -> NetDecl {
        NetDecl {
            ty: 0,
            audience: 0,
            max_rate: 0.0,
            epsilon: 0.0,
            on_spawn_only: false,
            bit_width: 32,
            min_value: -1.0,
            max_value: 1.0,
            extent: 2048.0,
            position_bits: 19,
            max_speed: 100.0,
            velocity_bits: 12,
            rotation_bits: 9,
            slot: 0,
        }
    }
}

fn int_of(v: f64) -> i64 {
    v as i32 as i64
}

impl NetDecl {
    /// `decl_from`: a row shorter than 14 is the defaults.
    pub fn from_row(s: &[f64]) -> NetDecl {
        if s.len() < 14 {
            return NetDecl::default();
        }
        NetDecl {
            ty: int_of(s[0]),
            audience: int_of(s[1]),
            max_rate: s[2],
            epsilon: s[3],
            on_spawn_only: s[4] != 0.0,
            bit_width: int_of(s[5]),
            min_value: s[6],
            max_value: s[7],
            extent: s[8],
            position_bits: int_of(s[9]),
            max_speed: s[10],
            velocity_bits: int_of(s[11]),
            rotation_bits: int_of(s[12]),
            slot: int_of(s[13]).max(0) as usize,
        }
    }

    /// STRING, BYTES and CUSTOM may align, so their GDScript encodings come in 8 paddings.
    pub fn may_align(&self) -> bool {
        matches!(self.ty, ty::STRING | ty::BYTES | ty::CUSTOM)
    }
}

/// A read value, as the C++ core's `Val`: the engine types it holds natively, else the variant
/// type and its serialised bytes.
#[derive(Clone, Debug, Default)]
pub enum Val {
    #[default]
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    V2([f32; 2]),
    V3([f32; 3]),
    Quat([f32; 4]),
    Ext(i64, Arc<Vec<u8>>),
}

/// `GSNetVar._values_equal`: the same type, and equal.
pub fn val_equal(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Nil, Val::Nil) => true,
        (Val::Bool(x), Val::Bool(y)) => x == y,
        (Val::Int(x), Val::Int(y)) => x == y,
        (Val::Float(x), Val::Float(y)) => x == y,
        (Val::V2(x), Val::V2(y)) => x == y,
        (Val::V3(x), Val::V3(y)) => x == y,
        (Val::Quat(x), Val::Quat(y)) => x == y,
        (Val::Ext(t, x), Val::Ext(u, y)) => t == u && (Arc::ptr_eq(x, y) || x == y),
        _ => false,
    }
}

#[inline]
fn len3(d: [f32; 3]) -> f32 {
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// `Vector3::distance_squared_to`, in f32.
#[inline]
pub fn dist2(from: [f32; 3], to: [f32; 3]) -> f32 {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// `GSNetVar.differs`.
pub fn differs(d: &NetDecl, old: &Val, cur: &Val) -> bool {
    if d.epsilon <= 0.0 {
        return !val_equal(old, cur);
    }
    match (old, cur) {
        (Val::Float(a), Val::Float(b)) => (a - b).abs() > d.epsilon,
        (Val::V3(a), Val::V3(b)) => len3([b[0] - a[0], b[1] - a[1], b[2] - a[2]]) as f64 > d.epsilon,
        (Val::V2(a), Val::V2(b)) => {
            let x = b[0] - a[0];
            let y = b[1] - a[1];
            (x * x + y * y).sqrt() as f64 > d.epsilon
        }
        (Val::Int(a), Val::Int(b)) => a.wrapping_sub(*b).wrapping_abs() > to_int(d.epsilon),
        _ => !val_equal(old, cur),
    }
}

/// GDScript's encoding of one value at each padding (`_native_encode`), with 8 zero bytes after.
#[derive(Clone, Debug, Default)]
pub struct Pre {
    pub pads: usize,
    pub data: Vec<Vec<u8>>,
    pub bits: Vec<i64>,
}

/// What one declaration writes this snapshot.
#[derive(Clone, Debug, Default)]
pub enum Enc {
    #[default]
    Nothing,
    Int(i64),
    Float(f64),
    V3([f32; 3]),
    Quat([f32; 4]),
    Bytes(Arc<Vec<u8>>),
    Pre(Arc<Pre>),
}

impl Writer {
    /// `BitWriter.write_run`: [param bits] bits of [param data] from [param from_bit].
    pub fn write_run(&mut self, data: &[u8], from_bit: i64, bits: i64) {
        let mut done = 0i64;
        while done < bits {
            let at = from_bit + done;
            let take = (bits - done).min(56);
            let first = (at >> 3) as usize;
            let shift = at & 7;
            let need = ((shift + take + 7) >> 3) as usize;
            let mut chunk = 0u64;
            for k in 0..need {
                chunk |= (data.get(first + k).copied().unwrap_or(0) as u64) << (8 * k);
            }
            self.write_bits((chunk >> shift) as i64, take);
            done += take;
        }
    }

    pub fn write_gamma(&mut self, value: i64) {
        let n = value + 1;
        let width = gamma_width(n);
        if width > 0 {
            self.write_bits(0, width);
        }
        for bit in (0..=width).rev() {
            self.write_bits((n >> bit) & 1, 1);
        }
    }

    pub fn byte_length(&self) -> usize {
        (self.bit_pos + 7) >> 3
    }
}

fn gamma_width(n: i64) -> i64 {
    let mut width = 0i64;
    while width < 62 && (n >> (width + 1)) != 0 {
        width += 1;
    }
    width
}

pub fn gamma_bits(value: i64) -> i64 {
    2 * gamma_width(value + 1) + 1
}

/// Sorted ids as (start, length) runs; a repeat is skipped.
pub fn unchanged_runs(mut ids: Vec<i64>) -> Vec<i64> {
    let mut out = Vec::new();
    if ids.is_empty() {
        return out;
    }
    ids.sort_unstable();
    let mut start = ids[0];
    let mut length = 1i64;
    for &id in &ids[1..] {
        if id == start + length {
            length += 1;
        } else if id >= start + length {
            out.push(start);
            out.push(length);
            start = id;
            length = 1;
        }
    }
    out.push(start);
    out.push(length);
    out
}

pub fn tail_bits(runs: &[i64]) -> i64 {
    let mut bits = 8 + gamma_bits((runs.len() >> 1) as i64) + TRAILER_BITS;
    let mut cursor = 0i64;
    for r in runs.chunks_exact(2) {
        bits += gamma_bits(r[0] - cursor) + gamma_bits(r[1] - 1);
        cursor = r[0] + r[1];
    }
    bits
}

pub fn write_tail(w: &mut Writer, runs: &[i64]) {
    w.write_bits(TAIL_MARK, 8);
    w.write_gamma((runs.len() >> 1) as i64);
    let mut cursor = 0i64;
    for r in runs.chunks_exact(2) {
        w.write_gamma(r[0] - cursor);
        w.write_gamma(r[1] - 1);
        cursor = r[0] + r[1];
    }
}

pub fn clamp_lead(v: i64) -> i64 {
    let span = 1i64 << (LEAD_FIELD_BITS - 1);
    v.clamp(-span, span - 1)
}

/// One declaration's value (`_write_value`).
pub fn write_value(w: &mut Writer, d: &NetDecl, e: &Enc) {
    let (i, f) = match e {
        Enc::Pre(pre) => {
            let p = if pre.pads == 8 { w.bit_pos & 7 } else { 0 };
            let data = pre.data.get(p).map(|v| v.as_slice()).unwrap_or(&[]);
            let bits = pre.bits.get(p).copied().unwrap_or(0);
            w.write_run(data, p as i64, bits);
            return;
        }
        Enc::Bytes(b) => {
            w.write_bytes(b);
            return;
        }
        Enc::Int(i) => (*i, 0.0),
        Enc::Float(f) => (0, *f),
        _ => (0, 0.0),
    };
    let v3 = if let Enc::V3(v) = e { *v } else { [0.0; 3] };
    let q = if let Enc::Quat(q) = e { *q } else { [0.0, 0.0, 0.0, 1.0] };
    match d.ty {
        ty::BOOL => w.write_bits((i != 0) as i64, 1),
        ty::UINT | ty::INT => w.write_bits(i, d.bit_width),
        ty::VARINT => w.write_svarint(i),
        ty::FLOAT => w.write_float32(f),
        ty::FLOAT_RANGE => w.write_float_range(f, d.min_value, d.max_value, d.bit_width),
        ty::VEC3_POSITION => w.write_vec3_range(v3, -d.extent, d.extent, d.position_bits),
        ty::VEC3_VELOCITY => w.write_vec3_range(v3, -d.max_speed, d.max_speed, d.velocity_bits),
        ty::VEC3_RANGE => w.write_vec3_range(v3, d.min_value, d.max_value, d.bit_width),
        ty::DIRECTION => w.write_direction(v3, d.bit_width),
        ty::QUATERNION => w.write_quaternion(q, d.rotation_bits),
        ty::ANGLE => w.write_angle(f, d.bit_width),
        _ => {}
    }
}

pub type Reads = Arc<Vec<Val>>;

#[derive(Clone, Debug)]
pub struct Entry {
    pub tick: i64,
    pub mask: u64,
    pub reads: Reads,
}

/// `GSNetBehaviour.PeerView`, properties as slots, dictionaries as bitmasks.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub acked_has: u64,
    pub believed_has: u64,
    pub relost: u64,
    pub sent_has: u64,
    pub acked: Vec<Val>,
    pub believed: Vec<Val>,
    pub last_sent: Vec<i64>,
    pub pending: VecDeque<Entry>,
    pub last_ack: i64,
    pub strict: bool,
}

#[inline]
fn bits_of(mask: u64) -> impl Iterator<Item = usize> {
    let mut m = mask;
    std::iter::from_fn(move || {
        if m == 0 {
            return None;
        }
        let s = m.trailing_zeros() as usize;
        m &= m - 1;
        Some(s)
    })
}

#[inline]
fn bit(s: usize) -> u64 {
    1u64.wrapping_shl(s as u32)
}

fn merge(into: &mut [Val], has: &mut u64, mask: u64, reads: &[Val]) {
    for s in bits_of(mask) {
        if let (Some(dst), Some(src)) = (into.get_mut(s), reads.get(s)) {
            *dst = src.clone();
        }
    }
    *has |= mask;
}

impl View {
    pub fn ensure(&mut self, n: usize) {
        if self.acked.len() < n {
            self.acked.resize(n, Val::Nil);
            self.believed.resize(n, Val::Nil);
            self.last_sent.resize(n, 0);
        }
    }

    pub fn is_fresh(&self) -> bool {
        self.pending.is_empty()
            && self.believed_has == 0
            && self.acked_has == 0
            && self.sent_has == 0
            && self.relost == 0
            && self.last_ack == 0
            && !self.strict
    }

    fn rebuild_believed(&mut self) {
        self.believed_has = self.acked_has;
        for s in bits_of(self.acked_has) {
            if s < self.believed.len() && s < self.acked.len() {
                self.believed[s] = self.acked[s].clone();
            }
        }
        for e in &self.pending {
            merge(&mut self.believed, &mut self.believed_has, e.mask, &e.reads);
        }
    }

    /// `GSNetBehaviour._sync_view`.
    pub fn sync(&mut self, acked_tick: i64, ring: &[i64]) {
        self.last_ack = acked_tick;
        if self.pending.is_empty() {
            return;
        }
        let mut resolved = 0;
        let mut dropped = false;
        while resolved < self.pending.len() {
            let e = self.pending[resolved].clone();
            if e.tick > acked_tick {
                break;
            }
            resolved += 1;
            let in_ring = !ring.is_empty() && e.tick >= 0 && ring[(e.tick as u64 % ring.len() as u64) as usize] == e.tick;
            if e.tick == acked_tick || !self.strict || in_ring {
                merge(&mut self.acked, &mut self.acked_has, e.mask, &e.reads);
            } else {
                dropped = true;
                self.relost |= e.mask;
                self.acked_has &= !e.mask;
            }
        }
        if resolved == 0 {
            return;
        }
        self.pending.drain(..resolved);
        if dropped {
            self.rebuild_believed();
        }
    }

    /// `GSNetBehaviour._trim_view`.
    pub fn trim(&mut self, oldest: i64) {
        while let Some(e) = self.pending.front() {
            if e.tick >= oldest {
                break;
            }
            let e = self.pending.pop_front().expect("front");
            if !self.strict {
                merge(&mut self.acked, &mut self.acked_has, e.mask, &e.reads);
            } else {
                self.relost |= e.mask;
                self.acked_has &= !e.mask;
            }
        }
        if self.strict {
            self.rebuild_believed();
        }
    }
}

/// One behaviour this snapshot (the C++ core's `BehRec`, less its engine half).
#[derive(Clone, Debug, Default)]
pub struct Beh {
    pub index: usize,
    pub decls: Vec<NetDecl>,
    pub nprops: usize,
    pub reads: Option<Reads>,
    pub enc: Vec<Enc>,
    pub est_owner: i64,
    pub est_other: i64,
    pub owner: i64,
}

impl Beh {
    /// Whether every declaration's slot is a read value (else this behaviour sends nothing).
    pub fn slots_ok(&self) -> bool {
        self.nprops <= 64 && self.decls.len() <= 64 && self.decls.iter().all(|d| d.slot < self.nprops)
    }
}

/// One identity this snapshot (`IdRec`).
#[derive(Clone, Debug)]
pub struct Ident {
    pub net_id: i64,
    pub owner: i64,
    pub shared: bool,
    pub behs: Vec<usize>,
    pub priority: f64,
    pub always: bool,
    pub linger_sec: f64,
    pub pos: [f32; 3],
    pub cluster: i32,
    pub radius: f64,
    pub score_limit: f64,
    pub far_dist: f64,
}

impl Default for Ident {
    fn default() -> Ident {
        Ident {
            net_id: 0,
            owner: 0,
            shared: false,
            behs: Vec::new(),
            priority: 1.0,
            always: false,
            linger_sec: 2.0,
            pos: [0.0; 3],
            cluster: -1,
            radius: 0.0,
            score_limit: 1.0,
            far_dist: 0.0,
        }
    }
}

/// `GSNetVisibility`'s BSP tree and decoded PVS rows.
#[derive(Clone, Debug, Default)]
pub struct VisTree {
    pub serial: i64,
    pub planes: Vec<f32>,
    pub nodes: Vec<i32>,
    pub leaf_cluster: Vec<i32>,
    pub head: i32,
    pub row_bytes: i64,
    pub clusters: i64,
    pub bounded: bool,
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub rows: Vec<u8>,
}

impl VisTree {
    /// `_vis_tree`'s checks: false for a malformed tree.
    pub fn valid(&self) -> bool {
        if self.clusters <= 0
            || self.row_bytes != (self.clusters + 7) >> 3
            || self.rows.len() as i64 != self.clusters * self.row_bytes
            || self.planes.len() % 4 != 0
            || self.nodes.len() % 3 != 0
            || self.nodes.is_empty()
        {
            return false;
        }
        let node_count = (self.nodes.len() / 3) as i64;
        let plane_count = (self.planes.len() / 4) as i64;
        let leaves = self.leaf_cluster.len() as i64;
        if self.head < 0 || self.head as i64 >= node_count || leaves == 0 {
            return false;
        }
        for n in self.nodes.chunks_exact(3) {
            if n[0] < 0 || n[0] as i64 >= plane_count {
                return false;
            }
            for &c in &n[1..] {
                let ok = if c >= 0 { (c as i64) < node_count } else { (-1 - c as i64) < leaves };
                if !ok {
                    return false;
                }
            }
        }
        self.leaf_cluster.iter().all(|&c| (c as i64) < self.clusters)
    }

    pub fn cluster_of(&self, p: [f32; 3]) -> i32 {
        if self.bounded
            && (p[0] < self.lo[0]
                || p[1] < self.lo[1]
                || p[2] < self.lo[2]
                || p[0] > self.hi[0]
                || p[1] > self.hi[1]
                || p[2] > self.hi[2])
        {
            return -1;
        }
        let mut node = self.head;
        let mut guard = self.nodes.len() / 3 + 1;
        while node >= 0 && guard > 0 {
            guard -= 1;
            let n = &self.nodes[node as usize * 3..node as usize * 3 + 3];
            let pl = &self.planes[n[0] as usize * 4..n[0] as usize * 4 + 4];
            let d = pl[0] as f64 * p[0] as f64 + pl[1] as f64 * p[1] as f64 + pl[2] as f64 * p[2] as f64 - pl[3] as f64;
            node = if d >= 0.0 { n[1] } else { n[2] };
        }
        if node >= 0 {
            return -1;
        }
        let leaf = (-1 - node as i64) as usize;
        self.leaf_cluster.get(leaf).copied().unwrap_or(-1)
    }

    pub fn sees(&self, from: i32, to: i32) -> bool {
        if from < 0 || to < 0 || from as i64 >= self.clusters || to as i64 >= self.clusters {
            return true;
        }
        let at = from as usize * self.row_bytes as usize + (to >> 3) as usize;
        self.rows.get(at).is_some_and(|b| b & (1u8 << (to & 7)) != 0)
    }
}

#[derive(Clone, Debug)]
pub struct InterestCfg {
    pub on: bool,
    pub radius: f64,
    pub radius_by_tag: Vec<(String, f64)>,
    pub interval_sec: f64,
    pub far_every: i64,
    pub far_distance: f64,
    pub far_by_tag: Vec<(String, f64)>,
    pub far_coherent: bool,
    pub far_tiers: i64,
    pub aim_cos: f64,
    pub ticks_per_snapshot: i64,
    pub cap: i64,
    pub send_budget: i64,
    pub starvation_ticks: i64,
    pub starvation_weight: f64,
    pub invalidation: i64,
    pub vis: Option<Arc<VisTree>>,
    pub vis_audible: f64,
}

impl Default for InterestCfg {
    fn default() -> InterestCfg {
        InterestCfg {
            on: false,
            radius: 200.0,
            radius_by_tag: Vec::new(),
            interval_sec: 0.25,
            far_every: 1,
            far_distance: 0.0,
            far_by_tag: Vec::new(),
            far_coherent: false,
            far_tiers: 1,
            aim_cos: 1.0,
            ticks_per_snapshot: 1,
            cap: 0,
            send_budget: 0,
            starvation_ticks: 10,
            starvation_weight: 1.0,
            invalidation: 0,
            vis: None,
            vis_audible: 0.0,
        }
    }
}

impl InterestCfg {
    pub fn thins(&self) -> bool {
        self.far_every > 1 && (self.far_distance > 0.0 || !self.far_by_tag.is_empty())
    }
}

/// `GSNetInterest`'s per-peer cache and lingering set.
#[derive(Clone, Debug, Default)]
pub struct PeerInterest {
    pub has: bool,
    pub at_ms: i64,
    pub order: Vec<i64>,
    pub ids: HashSet<i64>,
    pub linger: Vec<(i64, i64)>,
}

const HAS_ACC: u8 = 1;
const HAS_STARVED: u8 = 2;

/// A peer's `GSNetBudget.PeerBudget` between snapshots, keys in first-insertion order.
#[derive(Clone, Debug, Default)]
pub struct BudgetStore {
    slots: HashMap<i64, usize>,
    pub ids: Vec<i64>,
    acc: Vec<f64>,
    starved: Vec<i64>,
    has: Vec<u8>,
    pub acc_order: Vec<usize>,
    pub starved_order: Vec<usize>,
    pub loaded: bool,
}

impl BudgetStore {
    pub fn clear(&mut self) {
        self.slots.clear();
        self.ids.clear();
        self.acc.clear();
        self.starved.clear();
        self.has.clear();
        self.acc_order.clear();
        self.starved_order.clear();
    }

    pub fn slot(&mut self, id: i64) -> usize {
        let next = self.ids.len();
        let s = *self.slots.entry(id).or_insert(next);
        if s == next {
            self.ids.push(id);
            self.acc.push(0.0);
            self.starved.push(0);
            self.has.push(0);
        }
        s
    }

    pub fn get_acc(&self, s: usize) -> f64 {
        if self.has[s] & HAS_ACC != 0 { self.acc[s] } else { 0.0 }
    }

    pub fn get_starved(&self, s: usize) -> i64 {
        if self.has[s] & HAS_STARVED != 0 { self.starved[s] } else { 0 }
    }

    pub fn set_acc(&mut self, s: usize, v: f64) {
        if self.has[s] & HAS_ACC == 0 {
            self.has[s] |= HAS_ACC;
            self.acc_order.push(s);
        }
        self.acc[s] = v;
    }

    pub fn set_starved(&mut self, s: usize, v: i64) {
        if self.has[s] & HAS_STARVED == 0 {
            self.has[s] |= HAS_STARVED;
            self.starved_order.push(s);
        }
        self.starved[s] = v;
    }

    pub fn acc_at(&self, s: usize) -> f64 {
        self.acc[s]
    }

    pub fn starved_at(&self, s: usize) -> i64 {
        self.starved[s]
    }
}

/// One peer's views (by behaviour index), interest cache and budget.
#[derive(Clone, Debug, Default)]
pub struct Peer {
    pub id: i64,
    pub views: Vec<Option<Box<View>>>,
    pub interest: PeerInterest,
    pub budget: BudgetStore,
}

impl Peer {
    pub fn view(&mut self, index: usize) -> &mut View {
        if self.views.len() <= index {
            self.views.resize_with(index + 1, || None);
        }
        self.views[index].get_or_insert_with(Box::default)
    }

    pub fn existing_view(&self, index: usize) -> Option<&View> {
        self.views.get(index).and_then(|v| v.as_deref())
    }

    pub fn reset_view(&mut self, index: usize) {
        if let Some(v) = self.views.get_mut(index) {
            *v = None;
        }
    }
}

/// One peer's build: what `submit_peer` handed in, then what `_run_job` made.
#[derive(Clone, Debug)]
pub struct Job {
    pub peer_id: i64,
    pub relevant: Vec<usize>,
    pub acked_tick: i64,
    pub ring: Vec<i64>,
    pub strict: bool,
    pub compact: bool,
    pub send_lead: bool,
    pub remaining: i64,
    pub lead_x: i64,
    pub lead_y: i64,
    pub interest: bool,
    pub has_observer: bool,
    pub obs_pos: [f32; 3],
    pub obs_owner: i64,
    pub obs_aim: [f32; 3],
    pub cap_count: i64,
    pub payloads: Vec<Vec<u8>>,
    pub counts: Vec<i32>,
    pub starved: i64,
    pub spent: i64,
    pub split: bool,
    pub unchanged_listed: i64,
    pub unchanged_saved: i64,
    pub leads_sent: i64,
    pub oversize_id: i64,
    pub oversize_bytes: i64,
    /// Net ids whose budget entries the GDScript dictionaries reset to 0 (a job without a store).
    pub resets: Vec<i64>,
}

impl Default for Job {
    fn default() -> Job {
        Job {
            peer_id: 0,
            relevant: Vec::new(),
            acked_tick: 0,
            ring: Vec::new(),
            strict: false,
            compact: false,
            send_lead: false,
            remaining: 0,
            lead_x: 0,
            lead_y: 0,
            interest: false,
            has_observer: false,
            obs_pos: [0.0; 3],
            obs_owner: -1,
            obs_aim: [0.0; 3],
            cap_count: 0,
            payloads: Vec::new(),
            counts: Vec::new(),
            starved: 0,
            spent: 0,
            split: false,
            unchanged_listed: 0,
            unchanged_saved: 0,
            leads_sent: 0,
            oversize_id: -1,
            oversize_bytes: 0,
            resets: Vec::new(),
        }
    }
}

/// What every job of one snapshot reads.
pub struct Ctx<'a> {
    pub behs: &'a [Option<Beh>],
    pub ids: &'a [Option<Ident>],
    /// Every identity of this snapshot, for interest (slots into `ids`).
    pub all: &'a [usize],
    pub by_net: &'a HashMap<i64, usize>,
    pub icfg: &'a InterestCfg,
    pub tick: i64,
    pub now_ms: i64,
    pub mtu: i64,
    pub oldest_pending: i64,
}

impl Ctx<'_> {
    fn ident(&self, i: usize) -> &Ident {
        self.ids[i].as_ref().expect("a live identity")
    }

    fn beh(&self, i: usize) -> Option<&Beh> {
        self.behs.get(i).and_then(|b| b.as_ref())
    }

    /// `_collect`: the declarations (by index) this peer is owed.
    fn collect(&self, view: &mut View, rec: &Beh, peer_id: i64, until_acked: bool) -> u64 {
        let Some(reads) = rec.reads.as_ref() else {
            return 0;
        };
        let mut out = 0u64;
        for (i, d) in rec.decls.iter().enumerate() {
            if d.audience == AUD_OWNER {
                if peer_id != rec.owner {
                    continue;
                }
            } else if d.audience == AUD_OBSERVERS && peer_id == rec.owner {
                continue;
            }
            let s = d.slot;
            let b = bit(s);
            if d.max_rate > 0.0 {
                let interval = 1000.0 / d.max_rate;
                let last = if view.sent_has & b != 0 { view.last_sent[s] as f64 } else { -100000.0 };
                if self.now_ms as f64 - last < interval {
                    continue;
                }
            }
            let current = &reads[s];
            let dirty = if view.believed_has & b == 0 {
                true
            } else if d.epsilon <= 0.0 {
                !val_equal(&view.believed[s], current)
            } else {
                differs(d, &view.believed[s], current)
            };
            if d.on_spawn_only && !dirty {
                continue;
            }
            let mut owed = until_acked;
            let mut known = false;
            let mut unconfirmed = false;
            if !owed && view.relost & b != 0 {
                unconfirmed = view.acked_has & b == 0 || differs(d, &view.acked[s], current);
                known = true;
                if unconfirmed {
                    owed = true;
                } else {
                    view.relost &= !b;
                }
            }
            if !dirty {
                if !owed {
                    continue;
                }
                if !known {
                    unconfirmed = view.acked_has & b == 0 || differs(d, &view.acked[s], current);
                }
                if !unconfirmed {
                    continue;
                }
            }
            out |= 1u64 << i;
        }
        out
    }

    /// `_write_state`: the set, then the record of it.
    fn write_state(&self, w: &mut Writer, view: &mut View, rec: &Beh, mask: u64) {
        w.write_bits(mask.count_ones() as i64, 6);
        let mut slots = 0u64;
        let mut rated = 0u64;
        for i in bits_of(mask) {
            let d = &rec.decls[i];
            w.write_bits(i as i64, 6);
            write_value(w, d, &rec.enc[i]);
            slots |= bit(d.slot);
            if d.max_rate > 0.0 {
                rated |= bit(d.slot);
            }
        }
        if slots == 0 {
            return;
        }
        let Some(reads) = rec.reads.as_ref() else {
            return;
        };
        merge(&mut view.believed, &mut view.believed_has, slots, reads);
        for s in bits_of(rated) {
            view.last_sent[s] = self.now_ms;
        }
        view.sent_has |= rated;
        if self.tick <= 0 {
            merge(&mut view.acked, &mut view.acked_has, slots, reads);
            return;
        }
        view.pending.push_back(Entry {
            tick: self.tick,
            mask: slots,
            reads: reads.clone(),
        });
    }

    fn state_bits(&self, rec: &Beh, mask: u64, at: i64) -> i64 {
        let mut w = Writer::new();
        let pad = at & 7;
        if pad > 0 {
            w.write_bits(0, pad);
        }
        w.write_bits(mask.count_ones() as i64, 6);
        for i in bits_of(mask) {
            w.write_bits(i as i64, 6);
            write_value(&mut w, &rec.decls[i], &rec.enc[i]);
        }
        w.bit_length() as i64 - pad
    }

    fn begin_part(&self) -> Writer {
        let mut w = Writer::new();
        w.bytes.reserve(self.mtu.max(0) as usize + 16);
        w.write_bits(self.tick, 32);
        w.write_bits(0, 12);
        w
    }

    /// `_interest_job`: who this peer gets, ranked, and the budget's bookkeeping.
    fn interest_job(&self, peer: &mut Peer, job: &mut Job) {
        let c = self.icfg;
        let now = self.now_ms;
        let pi = &mut peer.interest;
        let mut relevant: Vec<usize> = Vec::with_capacity(self.all.len());
        let cached = c.interval_sec > 0.0 && pi.has && ((now - pi.at_ms) as f64 / 1000.0) < c.interval_sec;
        if cached {
            for &r in self.all {
                let id = self.ident(r);
                if pi.ids.contains(&id.net_id) || id.always || (job.has_observer && id.owner == job.obs_owner) {
                    relevant.push(r);
                }
            }
        } else {
            let vis = if job.has_observer { c.vis.as_deref() } else { None };
            let from = vis.map_or(-1, |v| v.cluster_of(job.obs_pos));
            let audible2 = c.vis_audible * c.vis_audible;
            for &r in self.all {
                let id = self.ident(r);
                if !job.has_observer || id.always || id.owner == job.obs_owner {
                    relevant.push(r);
                    continue;
                }
                let d2 = dist2(job.obs_pos, id.pos) as f64;
                if d2 <= id.radius * id.radius && (vis.is_none() || d2 <= audible2 || vis.is_some_and(|v| v.sees(from, id.cluster))) {
                    relevant.push(r);
                }
            }
            let mut ids: HashSet<i64> = HashSet::with_capacity(relevant.len() * 2);
            let mut order = Vec::with_capacity(relevant.len());
            for &r in &relevant {
                let n = self.ident(r).net_id;
                if ids.insert(n) {
                    order.push(n);
                }
            }
            let mut linger = pi.linger.clone();
            let mut in_linger: HashSet<i64> = linger.iter().map(|kv| kv.0).collect();
            for &n in &pi.order {
                if !ids.contains(&n) && !in_linger.contains(&n) {
                    linger.push((n, now));
                    in_linger.insert(n);
                }
            }
            let mut still = Vec::new();
            for kv in linger {
                if ids.contains(&kv.0) {
                    continue;
                }
                let Some(&r) = self.by_net.get(&kv.0) else {
                    continue;
                };
                let elapsed = (now - kv.1) as f64 / 1000.0;
                if elapsed < self.ident(r).linger_sec {
                    still.push(kv);
                    ids.insert(kv.0);
                    order.push(kv.0);
                    relevant.push(r);
                }
            }
            pi.has = true;
            pi.at_ms = now;
            pi.order = order;
            pi.ids = ids;
            pi.linger = still;
        }

        if job.has_observer && c.thins() {
            let mut index = self.tick / c.ticks_per_snapshot.max(1);
            if !c.far_coherent {
                index += peer.id;
            }
            let aimed = c.aim_cos < 1.0 && job.obs_aim != [0.0; 3];
            let mut kept = Vec::with_capacity(relevant.len());
            for r in relevant {
                let id = self.ident(r);
                let turn = index.wrapping_add(id.net_id);
                if id.owner == job.obs_owner || (turn.checked_rem(c.far_every) == Some(0) && c.far_tiers == 1) {
                    kept.push(r);
                    continue;
                }
                let far = id.far_dist;
                let mut limit = far * far;
                let d2 = dist2(job.obs_pos, id.pos) as f64;
                if far <= 0.0 || d2 <= limit {
                    kept.push(r);
                    continue;
                }
                let mut every = c.far_every;
                let mut band = 1;
                while band < c.far_tiers {
                    limit *= 4.0;
                    if d2 <= limit {
                        break;
                    }
                    every = every.wrapping_shl(1);
                    band += 1;
                }
                if every > c.far_every && aimed {
                    let rel = [id.pos[0] - job.obs_pos[0], id.pos[1] - job.obs_pos[1], id.pos[2] - job.obs_pos[2]];
                    let dot = job.obs_aim[0] * rel[0] + job.obs_aim[1] * rel[1] + job.obs_aim[2] * rel[2];
                    if dot as f64 >= c.aim_cos * d2.sqrt() {
                        every = c.far_every;
                    }
                }
                if turn.checked_rem(every) == Some(0) {
                    kept.push(r);
                }
            }
            relevant = kept;
        }

        let mut scores: Vec<f64> = relevant
            .iter()
            .map(|&r| {
                let id = self.ident(r);
                if !job.has_observer {
                    id.priority
                } else {
                    let distance = dist2(job.obs_pos, id.pos).sqrt() as f64;
                    let t = distance / id.score_limit;
                    let t = if t < 0.0 {
                        0.0
                    } else if t > 1.0 {
                        1.0
                    } else {
                        t
                    };
                    id.priority * (1.0 - t)
                }
            })
            .collect();

        job.cap_count = relevant.len() as i64;
        if c.cap > 0 && relevant.len() as i64 > c.cap {
            let mut keys: Vec<(f32, f32)> = relevant
                .iter()
                .enumerate()
                .map(|(i, &r)| (if self.ident(r).always { -1e9f32 } else { (-scores[i]) as f32 }, i as f32))
                .collect();
            keys.sort_by(|a, b| {
                use std::cmp::Ordering::*;
                if a.0 == b.0 {
                    a.1.partial_cmp(&b.1).unwrap_or(Equal)
                } else if a.0 < b.0 {
                    Less
                } else {
                    Greater
                }
            });
            let cap = c.cap as usize;
            let top: Vec<usize> = keys[..cap].iter().map(|k| relevant[k.1 as usize]).collect();
            let top_scores: Vec<f64> = keys[..cap].iter().map(|k| scores[k.1 as usize]).collect();
            relevant = top;
            scores = top_scores;
        }

        let count = relevant.len();
        let mut index_bits = 1u32;
        while (1i64 << index_bits) < count as i64 {
            index_bits += 1;
        }
        let low = (1i64 << index_bits) - 1;
        let store = &mut peer.budget;
        let mut keys: Vec<i64> = Vec::with_capacity(count);
        for (i, &r) in relevant.iter().enumerate() {
            let s = store.slot(self.ident(r).net_id);
            let waited = store.get_starved(s) + 1;
            store.set_starved(s, waited);
            let mut total = store.get_acc(s) + 0f64.max(scores[i]);
            if waited > c.starvation_ticks {
                let over = (waited - c.starvation_ticks) as f64;
                total += over * over * c.starvation_weight;
            }
            store.set_acc(s, total);
            let value = 0f64.max(total);
            let bits = value.to_bits() as i64;
            keys.push(((i64::MAX - bits) & !low) | i as i64);
        }
        keys.sort_unstable();
        relevant = keys.iter().map(|k| relevant[(k & low) as usize]).collect();

        if c.send_budget > 0 && relevant.len() as i64 > c.send_budget {
            relevant = relevant
                .into_iter()
                .enumerate()
                .filter(|&(i, r)| {
                    (i as i64) < c.send_budget || (job.has_observer && job.obs_owner > 0 && self.ident(r).owner == job.obs_owner)
                })
                .map(|(_, r)| r)
                .collect();
        }
        job.relevant = relevant;
    }

    /// `_run_job`: one peer's parts.
    pub fn run_job(&self, peer: &mut Peer, job: &mut Job) {
        if job.interest {
            self.interest_job(peer, job);
        }
        let peer_id = peer.id;
        let mut limit_bits = self.mtu * 8 - TRAILER_BITS;
        if job.send_lead {
            limit_bits -= tail_bits(&[]) + LEAD_BITS;
        }
        let mut parts = vec![self.begin_part()];
        let mut counts: Vec<i32> = vec![0];
        let mut written = 0i64;
        let mut remaining = job.remaining;
        let mut unchanged: Vec<i64> = Vec::new();
        let mut unchanged_header_bits = 0i64;
        let mut masks: Vec<u64> = Vec::new();
        let use_store = job.interest;
        let relevant = std::mem::take(&mut job.relevant);
        let reset = |peer: &mut Peer, job: &mut Job, net_id: i64| {
            if use_store {
                let s = peer.budget.slot(net_id);
                peer.budget.set_acc(s, 0.0);
                peer.budget.set_starved(s, 0);
            } else {
                job.resets.push(net_id);
            }
        };

        for &r in &relevant {
            let identity = self.ident(r);
            let predicted = identity.owner == peer_id && identity.shared;
            masks.clear();
            let mut anything = false;
            for &bi in &identity.behs {
                let Some(b) = self.beh(bi) else {
                    masks.push(0);
                    continue;
                };
                let v = peer.view(b.index);
                v.ensure(b.nprops);
                if job.strict {
                    v.strict = true;
                }
                if job.acked_tick > v.last_ack {
                    v.sync(job.acked_tick, &job.ring);
                }
                if v.pending.front().is_some_and(|e| e.tick < self.oldest_pending) {
                    v.trim(self.oldest_pending);
                }
                let m = self.collect(v, b, peer_id, predicted);
                masks.push(m);
                anything = anything || m != 0;
            }
            let nb = identity.behs.len() as i64;
            if job.compact && !anything {
                unchanged.push(identity.net_id);
                unchanged_header_bits += ID_BITS + 6 + 6 * nb;
                reset(peer, job, identity.net_id);
                continue;
            }
            let mut estimated = 0i64;
            for &bi in &identity.behs {
                if let Some(b) = self.beh(bi) {
                    estimated += if peer_id == b.owner { b.est_owner } else { b.est_other };
                }
            }
            estimated = (estimated + 7) >> 3;
            if written > 0 && estimated > remaining {
                job.starved += 1;
                continue;
            }
            let last = parts.len() - 1;
            if counts[last] > 0 && parts[last].bit_length() as i64 + estimated * 16 + 256 > limit_bits {
                let mut at = parts[last].bit_length() as i64 + ID_BITS + 6;
                for (k, &bi) in identity.behs.iter().enumerate() {
                    if let Some(b) = self.beh(bi) {
                        at += self.state_bits(b, masks[k], at);
                    }
                }
                if at > limit_bits {
                    if parts.len() >= MAX_PARTS {
                        job.starved += 1;
                        continue;
                    }
                    parts.push(self.begin_part());
                    counts.push(0);
                }
            }
            let last = parts.len() - 1;
            let w = &mut parts[last];
            let before = w.byte_length() as i64;
            w.write_bits((identity.net_id & ((1i64 << ID_BITS) - 1)) | ((nb & 0x3F) << ID_BITS), ID_BITS + 6);
            for (k, &bi) in identity.behs.iter().enumerate() {
                if let Some(b) = self.beh(bi) {
                    let v = peer.view(b.index);
                    self.write_state(w, v, b, masks[k]);
                }
            }
            if w.bit_length() as i64 > limit_bits && job.oversize_id < 0 {
                job.oversize_id = identity.net_id;
                job.oversize_bytes = (w.bit_length() as i64 + 7) >> 3;
            }
            let spent = w.byte_length() as i64 - before;
            remaining -= spent;
            job.spent += spent;
            reset(peer, job, identity.net_id);
            counts[last] += 1;
            written += 1;
        }
        job.relevant = relevant;

        if parts.len() > 1 && counts[counts.len() - 1] == 0 {
            parts.pop();
            counts.pop();
        }

        let mut tail_part: Option<usize> = None;
        let runs = unchanged_runs(unchanged.clone());
        if !runs.is_empty() {
            let bits = tail_bits(&runs);
            if parts[parts.len() - 1].bit_length() as i64 + bits <= limit_bits + TRAILER_BITS {
                tail_part = Some(parts.len() - 1);
            } else if parts.len() < MAX_PARTS {
                parts.push(self.begin_part());
                counts.push(0);
                tail_part = Some(parts.len() - 1);
            } else {
                job.starved += 1;
            }
            if tail_part.is_some() {
                job.unchanged_listed += unchanged.len() as i64;
                job.unchanged_saved += 0i64.max((unchanged_header_bits - bits) >> 3);
            }
        }

        let total = parts.len();
        job.split = total > 1;
        for (index, mut part) in parts.into_iter().enumerate() {
            let is_tail = tail_part == Some(index);
            if is_tail {
                write_tail(&mut part, &runs);
                part.write_bits(index as i64, 4);
                part.write_bits(total as i64 - 1, 4);
            } else if job.send_lead {
                write_tail(&mut part, &[]);
                part.write_bits(index as i64, 4);
                part.write_bits(total as i64 - 1, 4);
            } else if total > 1 {
                part.write_bits(index as i64, 4);
                part.write_bits(total as i64 - 1, 4);
            }
            if job.send_lead {
                part.write_bits(clamp_lead(job.lead_x), LEAD_FIELD_BITS);
                part.write_bits(clamp_lead(job.lead_y - self.tick), LEAD_FIELD_BITS);
                job.leads_sent += 1;
            }
            let mut payload = part.bytes;
            let count = counts[index];
            if COUNT_BYTE + 1 < payload.len() {
                payload[COUNT_BYTE] = (count & 0xFF) as u8;
                payload[COUNT_BYTE + 1] = (payload[COUNT_BYTE + 1] & 0xF0) | ((count >> 8) & 0x0F) as u8;
            }
            job.payloads.push(payload);
            job.counts.push(count + if is_tail { unchanged.len() as i32 } else { 0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_and_tail_bytes() {
        let mut w = Writer::new();
        w.write_gamma(0);
        w.write_gamma(4);
        // 0 -> "1"; 4 -> n=5=101b, width 2: "00" then 1,0,1.
        assert_eq!(w.bit_length(), 1 + 5);
        assert_eq!(w.bytes, vec![0b10_1001]);
        assert_eq!(gamma_bits(4), 5);
        assert_eq!(unchanged_runs(vec![7, 3, 4, 5, 5, 9]), vec![3, 3, 7, 1, 9, 1]);
        assert_eq!(tail_bits(&[]), 17);
        let runs = unchanged_runs(vec![2, 3]);
        let mut t = Writer::new();
        write_tail(&mut t, &runs);
        assert_eq!(t.bit_length() as i64 + TRAILER_BITS, tail_bits(&runs));
    }

    #[test]
    fn write_run_copies_bits() {
        let data = [0xABu8, 0xCD, 0xEF, 0x12, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut w = Writer::new();
        w.write_bits(1, 3);
        w.write_run(&data, 4, 20);
        let mut r = crate::Reader::new(&w.bytes);
        assert_eq!(r.read_bits(3), 1);
        assert_eq!(r.read_bits(20), (0x12EFCDAB >> 4) & 0xFFFFF);
    }

    fn decl(ty: i64, slot: usize) -> NetDecl {
        NetDecl {
            ty,
            bit_width: 8,
            slot,
            ..NetDecl::default()
        }
    }

    #[test]
    fn job_writes_delta_then_tail() {
        let beh = Beh {
            index: 0,
            decls: vec![decl(ty::UINT, 0), decl(ty::BOOL, 1)],
            nprops: 2,
            reads: Some(Arc::new(vec![Val::Int(42), Val::Bool(true)])),
            enc: vec![Enc::Int(42), Enc::Int(1)],
            est_owner: 64,
            est_other: 64,
            owner: 1,
        };
        let behs = vec![Some(beh)];
        let ids = vec![Some(Ident {
            net_id: 5,
            owner: 1,
            behs: vec![0],
            ..Ident::default()
        })];
        let by_net = HashMap::new();
        let icfg = InterestCfg::default();
        let ctx = Ctx {
            behs: &behs,
            ids: &ids,
            all: &[],
            by_net: &by_net,
            icfg: &icfg,
            tick: 100,
            now_ms: 1000,
            mtu: 1200,
            oldest_pending: 0,
        };
        let mut peer = Peer {
            id: 2,
            ..Peer::default()
        };
        let mut job = Job {
            peer_id: 2,
            relevant: vec![0],
            compact: true,
            remaining: 1000,
            ..Job::default()
        };
        ctx.run_job(&mut peer, &mut job);
        // tick:32, count:12, id:24|nb:6, count:6, (index:6, value) * 2.
        let mut r = crate::Reader::new(&job.payloads[0]);
        assert_eq!(r.read_bits(32), 100);
        assert_eq!(r.read_bits(12), 1);
        assert_eq!(r.read_bits(30), 5 | (1 << 24));
        assert_eq!(r.read_bits(6), 2);
        assert_eq!(r.read_bits(6), 0);
        assert_eq!(r.read_bits(8), 42);
        assert_eq!(r.read_bits(6), 1);
        assert_eq!(r.read_bits(1), 1);
        assert_eq!(job.resets, vec![5]);
        // Nothing changed and nothing acked: owed again only once dropped; the believed copy holds it.
        let mut again = Job {
            peer_id: 2,
            relevant: vec![0],
            compact: true,
            remaining: 1000,
            ..Job::default()
        };
        ctx.run_job(&mut peer, &mut again);
        assert_eq!(again.unchanged_listed, 1);
        assert_eq!(again.counts, vec![1]);
        let mut r = crate::Reader::new(&again.payloads[0]);
        assert_eq!(r.read_bits(32), 100);
        assert_eq!(r.read_bits(12), 0);
        assert_eq!(r.read_bits(8), TAIL_MARK);
    }
}
