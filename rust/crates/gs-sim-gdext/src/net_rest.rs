//! `GSSimNetRest`: game-fatal-contact's `FcPlayerNet._pull_rest` for every player in one call a tick (P6d):
//! the owner's exact float32 copy (`_pull_exact`), `GSCombatNetSync.pull`'s health, armour and alive, the
//! challenge clamps, the plain copies (blind, app points, beacon, flashlight, protected) and, on the authority,
//! the identity's `always_relevant` and `interest_aim`. The same values the script writes, bit for bit
//! (gs-sim-snapshot's `net_rest`). What needs the game's own scripts stays with the caller: a player some client
//! owns gets its `push_exact` and carry mask from the script after this call.
//!
//! Cheaper than the script only by crossing less: the exact copy comes from the batch's move rows (no state
//! reads), and a field is written only when its value differs from what this object last wrote there. That
//! holds while nothing else writes the fields: the caller says [method forget] for a net its script pulled.

use std::collections::HashMap;

use godot::prelude::*;
use gs_sim_snapshot::net_rest::{
    EXACT_FLOATS, EXACT_FLOATS_TIMED, challenge_value_wire, challenge_wire, exact_from_moves,
    health_wire,
};

/// `GSCombatNetSync.HEALTH_BITS`.
const HEALTH_BITS: u32 = 11;

/// Property and method names, indexed by the `N_*` constants.
const NAMES: [&str; 30] = [
    "player",
    "identity",
    "health",
    "armour",
    "alive",
    "is_authoritative",
    "owner_peer_id",
    "net_exact",
    "net_health",
    "net_armour",
    "net_alive",
    "challenge",
    "net_challenge",
    "challenge_value",
    "net_challenge_value",
    "blinded",
    "net_blind",
    "app_points_on",
    "net_app_points_on",
    "app_points",
    "net_app_points",
    "beacon",
    "net_beacon",
    "flashlight",
    "net_flashlight",
    "protected_view",
    "net_protected",
    "always_relevant",
    "interest_aim",
    "aim_direction",
];
const N_PLAYER: usize = 0;
const N_IDENTITY: usize = 1;
const N_HEALTH: usize = 2;
const N_ARMOUR: usize = 3;
const N_ALIVE: usize = 4;
const N_AUTHORITATIVE: usize = 5;
const N_OWNER: usize = 6;
const N_NET_EXACT: usize = 7;
const N_NET_HEALTH: usize = 8;
const N_NET_ARMOUR: usize = 9;
const N_NET_ALIVE: usize = 10;
const N_CHALLENGE: usize = 11;
const N_NET_CHALLENGE: usize = 12;
const N_CHALLENGE_VALUE: usize = 13;
const N_NET_CHALLENGE_VALUE: usize = 14;
/// (player property, net property), copied unchanged in the script's order.
const COPIES: [(usize, usize); 6] = [(15, 16), (17, 18), (19, 20), (21, 22), (23, 24), (25, 26)];
const N_BEACON: usize = 21;
const N_ALWAYS_RELEVANT: usize = 27;
const N_INTEREST_AIM: usize = 28;
const N_AIM_DIRECTION: usize = 29;

/// Cache slots: what this object last wrote to a field.
const S_EXACT_EMPTY: usize = 0;
const S_HEALTH: usize = 1;
const S_ARMOUR: usize = 2;
const S_ALIVE: usize = 3;
const S_CHALLENGE: usize = 4;
const S_CHALLENGE_VALUE: usize = 5;
const S_COPIES: usize = 6;
const S_ALWAYS_RELEVANT: usize = S_COPIES + COPIES.len();
const SLOTS: usize = S_ALWAYS_RELEVANT + 1;

type Written = [Option<Variant>; SLOTS];

/// The rest of every player's net pull in one call.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimNetRest {
    /// [`NAMES`] as names, made on the first [method pull_rest_rows].
    k: Vec<StringName>,
    /// Per net (instance id): what was last written to each field, and the call that wrote it.
    written: HashMap<i64, (u64, Written)>,
    /// Calls to [method pull_rest_rows] so far.
    calls: u64,
    /// Whether the exact copy is the sixteen-float timed record ([method set_exact_floats]); fourteen otherwise.
    timed: bool,
}

fn obj(v: &Variant) -> Option<Gd<Object>> {
    if v.get_type() == VariantType::OBJECT {
        v.try_to::<Gd<Object>>().ok()
    } else {
        None
    }
}

fn num(v: &Variant) -> f64 {
    match v.get_type() {
        VariantType::FLOAT => v.to::<f64>(),
        VariantType::INT => v.to::<i64>() as f64,
        _ => 0.0,
    }
}

fn int(v: &Variant) -> i64 {
    match v.get_type() {
        VariantType::INT => v.to::<i64>(),
        _ => 0,
    }
}

fn truth(v: &Variant) -> bool {
    v.get_type() == VariantType::BOOL && v.to::<bool>()
}

/// Writes [param value] to [param name] on [param target] unless the slot says it is already there.
fn put(target: &mut Gd<Object>, name: &StringName, slot: &mut Option<Variant>, value: Variant) {
    if slot.as_ref() != Some(&value) {
        target.set(name, &value);
        *slot = Some(value);
    }
}

#[godot_api]
impl GSSimNetRest {
    /// Floats in the exact copy this object writes (`FcPlayerNet.EXACT_FLOATS`): fourteen until
    /// [method set_exact_floats] says sixteen.
    #[func]
    fn exact_floats(&self) -> i64 {
        self.exact_len() as i64
    }

    /// Makes the exact copy [param n] floats: fourteen (`GSFpsNetSync.EXACT_FLOATS`) or sixteen
    /// (`EXACT_FLOATS_TIMED`, slide and wall-run time last). False, and nothing changed, for any other length.
    #[func]
    fn set_exact_floats(&mut self, n: i64) -> bool {
        match usize::try_from(n) {
            Ok(EXACT_FLOATS) => self.timed = false,
            Ok(EXACT_FLOATS_TIMED) => self.timed = true,
            _ => return false,
        }
        true
    }

    /// `FcPlayerNet._pull_rest` for every net of [param nets], in order, except what needs the game's scripts.
    /// Row `i` of [param moves] ([param columns] state columns then [param extra] values, `FcNetPullBatch._gather`'s
    /// rows) is net `i`'s state as the batch read it this tick. Returns, in order, `i` for each net `i` some client
    /// owns (the caller runs its `push_exact` and carry mask) and `-1 - i` for each net it could not read (no player
    /// or row: the caller runs the whole script pull for it, which writes nothing here first).
    /// [param interest_aim]: whether gs-net's identity has `interest_aim` (`FcPlayerNet._has_interest_aim`).
    #[func]
    fn pull_rest_rows(
        &mut self,
        nets: Array<Gd<Object>>,
        interest_aim: bool,
        moves: PackedFloat64Array,
        columns: i64,
        extra: i64,
    ) -> PackedInt32Array {
        if self.k.is_empty() {
            self.k = NAMES.iter().map(|n| StringName::from(*n)).collect();
        }
        self.calls += 1;
        let (columns, extra) = (columns.max(0) as usize, extra.max(0) as usize);
        let moves = moves.as_slice();
        let mut follow = PackedInt32Array::new();
        for (i, mut net) in nets.iter_shared().enumerate() {
            let id = net.instance_id().to_i64();
            let mut entry = match self.written.remove(&id) {
                // Only the last call's values are trusted: a net skipped in between was pulled by its script.
                Some((call, w)) if call + 1 == self.calls => w,
                _ => std::array::from_fn(|_| None),
            };
            let exact = exact_from_moves(moves, i, columns, extra);
            match self.pull_one(&mut net, exact, &mut entry, interest_aim) {
                Some(owned) => {
                    if owned {
                        follow.push(i as i32);
                    }
                    self.written.insert(id, (self.calls, entry));
                }
                None => follow.push(-1 - i as i32),
            }
        }
        if self.written.len() > nets.len() * 2 + 8 {
            let calls = self.calls;
            self.written.retain(|_, (call, _)| *call == calls);
        }
        follow
    }

    /// Drops what this object believes is written on [param net]: something else (its script) wrote the fields.
    #[func]
    fn forget(&mut self, net: Gd<Object>) {
        self.written.remove(&net.instance_id().to_i64());
    }
}

impl GSSimNetRest {
    fn exact_len(&self) -> usize {
        if self.timed {
            EXACT_FLOATS_TIMED
        } else {
            EXACT_FLOATS
        }
    }

    /// One net's pull: Some(owned by a client), None (nothing written) when it cannot be read.
    fn pull_one(
        &self,
        net: &mut Gd<Object>,
        exact: Option<[f32; EXACT_FLOATS_TIMED]>,
        w: &mut Written,
        interest_aim: bool,
    ) -> Option<bool> {
        let k = &self.k;
        let player = obj(&net.get(&k[N_PLAYER]))?;
        let identity = obj(&net.get(&k[N_IDENTITY]));
        let authoritative = identity
            .as_ref()
            .is_some_and(|id| truth(&id.get(&k[N_AUTHORITATIVE])));
        let server_owned = authoritative
            && identity
                .as_ref()
                .is_some_and(|id| int(&id.get(&k[N_OWNER])) == 0);

        // _pull_exact
        if server_owned {
            if w[S_EXACT_EMPTY].is_none() {
                let empty = net
                    .get(&k[N_NET_EXACT])
                    .try_to::<PackedFloat32Array>()
                    .is_ok_and(|a| a.is_empty());
                if !empty {
                    net.set(&k[N_NET_EXACT], &PackedFloat32Array::new().to_variant());
                }
                w[S_EXACT_EMPTY] = Some(true.to_variant());
            }
        } else {
            let row = exact?;
            net.set(
                &k[N_NET_EXACT],
                &PackedFloat32Array::from(&row[..self.exact_len()]).to_variant(),
            );
            w[S_EXACT_EMPTY] = None;
        }

        // GSCombatNetSync.pull
        if let Some(health) = obj(&player.get(&k[N_HEALTH])) {
            let h = health_wire(num(&health.get(&k[N_HEALTH])), HEALTH_BITS);
            let a = health_wire(num(&health.get(&k[N_ARMOUR])), HEALTH_BITS);
            put(net, &k[N_NET_HEALTH], &mut w[S_HEALTH], h.to_variant());
            put(net, &k[N_NET_ARMOUR], &mut w[S_ARMOUR], a.to_variant());
            put(
                net,
                &k[N_NET_ALIVE],
                &mut w[S_ALIVE],
                health.get(&k[N_ALIVE]),
            );
        }

        let (from, to) = COPIES[0];
        put(net, &k[to], &mut w[S_COPIES], player.get(&k[from]));
        let challenge = challenge_wire(int(&player.get(&k[N_CHALLENGE])));
        put(
            net,
            &k[N_NET_CHALLENGE],
            &mut w[S_CHALLENGE],
            challenge.to_variant(),
        );
        let value = challenge_value_wire(int(&player.get(&k[N_CHALLENGE_VALUE])));
        put(
            net,
            &k[N_NET_CHALLENGE_VALUE],
            &mut w[S_CHALLENGE_VALUE],
            value.to_variant(),
        );
        let mut beacon = Variant::nil();
        for (n, &(from, to)) in COPIES.iter().enumerate().skip(1) {
            let v = player.get(&k[from]);
            if from == N_BEACON {
                beacon = v.clone();
            }
            put(net, &k[to], &mut w[S_COPIES + n], v);
        }

        if authoritative && let Some(mut id) = identity {
            put(
                &mut id,
                &k[N_ALWAYS_RELEVANT],
                &mut w[S_ALWAYS_RELEVANT],
                beacon,
            );
            if interest_aim {
                let mut p = player.clone();
                let aim = p.call(&k[N_AIM_DIRECTION], &[]);
                id.set(&k[N_INTEREST_AIM], &aim);
            }
        }
        Some(!server_owned)
    }
}
