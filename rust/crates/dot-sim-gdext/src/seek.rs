//! `DotSimHunterSeek`: game-prop-hunt's hunter senses for a tick, every hunter in one call.
//!
//! The game writes the map's pieces once a map (`set_pieces`), their broken flags as they change,
//! each hunter's search places when the script moved them (`set_search`), binds a
//! [`DotSimSightRay`] to its world (`set_sight`), then each tick hands the props as they stand
//! (`set_props`) and asks `sense` for all hunters: the prey each has in sight, the places it
//! crosses off and the doubt rows it adds, the script's to the bit and in its order
//! (`dot_sim_seek`; the game's `headless_native_seek` and its `seek_verify` hold the two together).
//! Every line of sight is the script's `_clear_between` ray, asked natively.

use crate::sight::{DotSimSightRay, SightQuery};
use dot_sim_math::Vec3;
use dot_sim_seek::{Hunter, Piece, Prop, Rules, Search, Seek, Sensed, Taunt};
use godot::prelude::*;

/// Prop flag bits in `set_props`.
const ALIVE: i32 = 1;
const WATCHING: i32 = 2;
const MARKED: i32 = 4;
const DISGUISED: i32 = 8;
const TILTED: i32 = 16;
/// Hunter flag bits in `sense`.
const CAREFUL: i32 = 1;
const SENSE: i32 = 2;
const OBSERVE: i32 = 4;

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimHunterSeek {
    seek: Seek,
    props: Vec<Prop>,
    hunters: Vec<Hunter>,
    taunts: Vec<Taunt>,
    out: Vec<Sensed>,
    sight: Option<Gd<DotSimSightRay>>,
    query: SightQuery,
    /// Rays asked since the last `rays` read.
    rays: i64,
}

#[godot_api]
impl DotSimHunterSeek {
    /// Holds the map's pieces: each one's middle, feet, kind code and kin near it (less itself;
    /// shorter than the pieces is allowed, the rest are counted), all whole.
    #[func]
    fn set_pieces(
        &mut self,
        middles: PackedVector3Array,
        feet: PackedVector3Array,
        kinds: PackedInt32Array,
        kin_near: PackedInt32Array,
    ) {
        let (m, f, k) = (middles.as_slice(), feet.as_slice(), kinds.as_slice());
        if m.len() != f.len() || m.len() != k.len() {
            godot_error!(
                "DotSimHunterSeek.set_pieces: {} middles, {} feet, {} kinds",
                m.len(),
                f.len(),
                k.len()
            );
            return;
        }
        let pieces = (0..m.len())
            .map(|i| Piece {
                middle: v3(m[i]),
                feet: v3(f[i]),
                kind: k[i],
                broken: false,
            })
            .collect();
        self.seek.set_pieces(pieces, kin_near.as_slice().to_vec());
    }

    /// Piece [param index] broken or whole; an index outside the table is an error.
    #[func]
    fn set_broken(&mut self, index: i32, broken: bool) {
        if !usize::try_from(index).is_ok_and(|i| self.seek.set_broken(i, broken)) {
            godot_error!(
                "DotSimHunterSeek.set_broken: index {index} outside {} pieces",
                self.seek.pieces.len()
            );
        }
    }

    /// The ray every line of sight asks (bound by the game to its world, mask and all).
    #[func]
    fn set_sight(&mut self, ray: Option<Gd<DotSimSightRay>>) {
        self.sight = ray;
        self.query = SightQuery::default();
    }

    /// A new round: every hunter's places and cleared list forgotten.
    #[func]
    fn reset(&mut self) {
        self.seek.minds.clear();
    }

    /// Hunter [param id] forgotten (its state was dropped).
    #[func]
    fn forget(&mut self, id: GString) {
        self.seek.minds.remove(&id.to_string());
    }

    /// Hunter [param id]'s search places as the script holds them now (cleared list kept).
    #[func]
    fn set_search(
        &mut self,
        id: GString,
        at: PackedVector3Array,
        seen_at: PackedFloat64Array,
        recheck: f64,
        look_reach: f64,
    ) {
        if at.len() != seen_at.len() {
            godot_error!(
                "DotSimHunterSeek.set_search: {} places, {} seen times",
                at.len(),
                seen_at.len()
            );
            return;
        }
        self.seek.mind(&id.to_string()).search = Search {
            at: at.as_slice().iter().map(|v| v3(*v)).collect(),
            seen_at: seen_at.as_slice().to_vec(),
            recheck,
            look_reach,
        };
    }

    /// The seen times of hunter [param id]'s places as held here (a check against the script's).
    #[func]
    fn seen_of(&self, id: GString) -> PackedFloat64Array {
        match self.seek.minds.get(&id.to_string()) {
            Some(m) => PackedFloat64Array::from(m.search.seen_at.as_slice()),
            None => PackedFloat64Array::new(),
        }
    }

    /// The props as `players_on(PROPS)` lists them this tick: position, velocity, hull height,
    /// flag bits (alive 1, watching 2, marked 4, disguised 8, tilted 16), kind code and id.
    #[func]
    fn set_props(
        &mut self,
        at: PackedVector3Array,
        velocity: PackedVector3Array,
        hull: PackedFloat64Array,
        flags: PackedInt32Array,
        kinds: PackedInt32Array,
        ids: PackedStringArray,
    ) {
        let n = at.len();
        if [
            velocity.len(),
            hull.len(),
            flags.len(),
            kinds.len(),
            ids.len(),
        ]
        .iter()
        .any(|&l| l != n)
        {
            godot_error!("DotSimHunterSeek.set_props: columns of different lengths");
            self.props.clear();
            return;
        }
        let (a, v, h, f, k) = (
            at.as_slice(),
            velocity.as_slice(),
            hull.as_slice(),
            flags.as_slice(),
            kinds.as_slice(),
        );
        let ids = ids.as_slice();
        self.props.resize_with(n, Prop::default);
        for (i, p) in self.props.iter_mut().enumerate() {
            p.id.clear();
            p.id.push_str(&ids[i].to_string());
            p.at = v3(a[i]);
            p.velocity = v3(v[i]);
            p.hull = h[i];
            p.alive = f[i] & ALIVE != 0;
            p.watching = f[i] & WATCHING != 0;
            p.marked = f[i] & MARKED != 0;
            p.disguised = f[i] & DISGUISED != 0;
            p.tilted = f[i] & TILTED != 0;
            p.kind = k[i];
        }
    }

    /// Every hunter's senses this tick. [param rules]: now, step, base, fov, sight, memory,
    /// map knowledge, aggression, round. Per hunter: id, eye, forward, flag bits (careful 1,
    /// sense 2, observe 4), the props index it found (-1), when, and the key of a test that ends
    /// this tick ("" none). Returns `[ints, floats]`: per hunter prey, sensed, the count and
    /// indices of the places crossed off, the count of doubt rows and each row's key (piece
    /// index, or -1 - prop index) and capped flag; the floats are the rows' additions in order.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn sense(
        &mut self,
        rules: PackedFloat64Array,
        taunt_at: PackedVector3Array,
        taunt_t: PackedFloat64Array,
        ids: PackedStringArray,
        eye: PackedVector3Array,
        forward: PackedVector3Array,
        flags: PackedInt32Array,
        found: PackedInt32Array,
        found_at: PackedFloat64Array,
        clearing: PackedStringArray,
    ) -> VarArray {
        let r = rules.as_slice();
        let n = ids.len();
        if r.len() != 9
            || taunt_at.len() != taunt_t.len()
            || [
                eye.len(),
                forward.len(),
                flags.len(),
                found.len(),
                found_at.len(),
                clearing.len(),
            ]
            .iter()
            .any(|&l| l != n)
        {
            godot_error!("DotSimHunterSeek.sense: rules need 9 values and columns of one length");
            return VarArray::new();
        }
        let rules = Rules {
            now: r[0],
            step: r[1],
            base: r[2],
            fov: r[3],
            sight: r[4],
            memory: r[5],
            map_knowledge: r[6],
            aggression: r[7],
            round: r[8] as i64,
        };
        self.taunts.clear();
        for (a, t) in taunt_at.as_slice().iter().zip(taunt_t.as_slice()) {
            self.taunts.push(Taunt { at: v3(*a), t: *t });
        }
        let (ids, e, fw, fl, fo, fa, cl) = (
            ids.as_slice(),
            eye.as_slice(),
            forward.as_slice(),
            flags.as_slice(),
            found.as_slice(),
            found_at.as_slice(),
            clearing.as_slice(),
        );
        self.hunters.resize_with(n, Hunter::default);
        for (i, h) in self.hunters.iter_mut().enumerate() {
            h.id.clear();
            h.id.push_str(&ids[i].to_string());
            h.eye = v3(e[i]);
            h.forward = v3(fw[i]);
            h.careful = fl[i] & CAREFUL != 0;
            h.sense = fl[i] & SENSE != 0;
            h.observe = fl[i] & OBSERVE != 0;
            h.found = fo[i];
            h.found_at = fa[i];
            h.clearing = if cl[i].is_empty() {
                None
            } else {
                Some(cl[i].to_string())
            };
        }

        let conf = self.sight.as_ref().map(|s| s.bind().conf());
        let query = &mut self.query;
        let rays = &mut self.rays;
        let mut los = |a: Vec3, b: Vec3| -> bool {
            *rays += 1;
            match &conf {
                Some(c) => c.clear_to(query, gv(a), gv(b)),
                None => true,
            }
        };
        self.seek.sense(
            &rules,
            &self.props,
            &self.taunts,
            &self.hunters,
            &mut los,
            &mut self.out,
        );

        let mut ints = Vec::with_capacity(n * 4);
        let mut floats = Vec::new();
        for s in &self.out {
            ints.push(s.prey);
            ints.push(i32::from(s.sensed));
            ints.push(s.crossed.len() as i32);
            ints.extend_from_slice(&s.crossed);
            ints.push(s.doubt.len() as i32);
            for row in &s.doubt {
                ints.push(row.key);
                ints.push(i32::from(row.capped));
                floats.push(row.delta);
            }
        }
        let mut out = VarArray::new();
        out.push(&PackedInt32Array::from(ints.as_slice()).to_variant());
        out.push(&PackedFloat64Array::from(floats.as_slice()).to_variant());
        out
    }

    /// Rays asked since the last read (the suites check the native path cast them).
    #[func]
    fn rays(&mut self) -> i64 {
        std::mem::take(&mut self.rays)
    }
}

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn gv(v: Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}
