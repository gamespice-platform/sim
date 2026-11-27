//! `GSSimNetRest`: game-fatal-contact's `FcPlayerNet._pull_rest` for every player in one call a tick (P6d):
//! the owner's exact float32 copy (`_pull_exact`), `GSCombatNetSync.pull`'s health, armour and alive, the
//! challenge clamps, the plain copies (blind, app points, beacon, flashlight, protected) and, on the authority,
//! the identity's `always_relevant` and `interest_aim`. The same values the script writes, bit for bit
//! (gs-sim-snapshot's `net_rest`). What needs the game's own scripts stays with the caller: a player some client
//! owns gets its `push_exact` and carry mask from the script after this call.

use godot::prelude::*;
use gs_sim_snapshot::net_rest::{
    EXACT_FLOATS, challenge_value_wire, challenge_wire, exact_row, health_wire,
};

/// `GSCombatNetSync.HEALTH_BITS`.
const HEALTH_BITS: u32 = 11;

/// The rest of every player's net pull in one call.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimNetRest {
    /// [`NAMES`] as names, made on the first [method pull_rest].
    k: Vec<StringName>,
}

/// Property and method names, indexed by the `N_*` constants.
const NAMES: [&str; 36] = [
    "player",
    "identity",
    "controller",
    "state",
    "_f",
    "stamina",
    "stamina_exhausted",
    "stamina_delay_left",
    "health",
    "armour",
    "alive",
    "is_authoritative",
    "owner_peer_id",
    "net_exact",
    "net_health",
    "net_armour",
    "net_alive",
    "blinded",
    "net_blind",
    "challenge",
    "net_challenge",
    "challenge_value",
    "net_challenge_value",
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
const N_CONTROLLER: usize = 2;
const N_STATE: usize = 3;
const N_F: usize = 4;
const N_STAMINA: usize = 5;
const N_EXHAUSTED: usize = 6;
const N_DELAY: usize = 7;
const N_HEALTH_OBJ: usize = 8;
const N_ARMOUR: usize = 9;
const N_ALIVE: usize = 10;
const N_AUTHORITATIVE: usize = 11;
const N_OWNER: usize = 12;
const N_NET_EXACT: usize = 13;
const N_NET_HEALTH: usize = 14;
const N_NET_ARMOUR: usize = 15;
const N_NET_ALIVE: usize = 16;
/// Pairs of (player property, net property) copied unchanged, after the challenge pair.
const COPIES: [(usize, usize); 6] = [(17, 18), (23, 24), (25, 26), (27, 28), (29, 30), (31, 32)];
const N_CHALLENGE: usize = 19;
const N_NET_CHALLENGE: usize = 20;
const N_CHALLENGE_VALUE: usize = 21;
const N_NET_CHALLENGE_VALUE: usize = 22;
const N_BEACON: usize = 27;
const N_ALWAYS_RELEVANT: usize = 33;
const N_INTEREST_AIM: usize = 34;
const N_AIM_DIRECTION: usize = 35;

fn obj(v: Variant) -> Option<Gd<Object>> {
    v.try_to::<Gd<Object>>().ok()
}

fn num(v: &Variant) -> f64 {
    v.try_to::<f64>()
        .or_else(|_| v.try_to::<i64>().map(|i| i as f64))
        .unwrap_or(0.0)
}

fn int(v: &Variant) -> i64 {
    v.try_to::<i64>().unwrap_or(0)
}

fn truth(v: &Variant) -> bool {
    v.try_to::<bool>().unwrap_or(false)
}

#[godot_api]
impl GSSimNetRest {
    /// Floats in the exact copy (`FcPlayerNet.EXACT_FLOATS`).
    #[func]
    fn exact_floats(&self) -> i64 {
        EXACT_FLOATS as i64
    }

    /// `FcPlayerNet._pull_rest` for every net of [param nets], in order, except what needs the game's scripts.
    /// Returns, in order, `i` for each net `i` some client owns (not server-owned: the caller runs its
    /// `push_exact` and carry mask) and `-1 - i` for each net it could not read (no player, controller, state or
    /// columns: the caller runs the whole script pull for it, which writes nothing here first).
    /// [param interest_aim]: whether gs-net's identity has `interest_aim` (`FcPlayerNet._has_interest_aim`).
    #[func]
    fn pull_rest(&mut self, nets: Array<Gd<Object>>, interest_aim: bool) -> PackedInt32Array {
        if self.k.is_empty() {
            self.k = NAMES.iter().map(|n| StringName::from(*n)).collect();
        }
        let mut follow = PackedInt32Array::new();
        for (i, mut net) in nets.iter_shared().enumerate() {
            match self.pull_one(&mut net, interest_aim) {
                Some(true) => follow.push(i as i32),
                Some(false) => {}
                None => follow.push(-1 - i as i32),
            }
        }
        follow
    }
}

impl GSSimNetRest {
    /// One net's pull: Some(owned by a client), None (nothing written) when it cannot be read.
    fn pull_one(&self, net: &mut Gd<Object>, interest_aim: bool) -> Option<bool> {
        let k = &self.k;
        let player = obj(net.get(&k[N_PLAYER]))?;
        let state = obj(obj(player.get(&k[N_CONTROLLER]))?.get(&k[N_STATE]))?;
        let identity = obj(net.get(&k[N_IDENTITY]));
        let authoritative = identity
            .as_ref()
            .is_some_and(|id| truth(&id.get(&k[N_AUTHORITATIVE])));
        let server_owned = authoritative
            && identity
                .as_ref()
                .is_some_and(|id| int(&id.get(&k[N_OWNER])) == 0);

        // _pull_exact
        if server_owned {
            let empty = net
                .get(&k[N_NET_EXACT])
                .try_to::<PackedFloat32Array>()
                .is_ok_and(|a| a.is_empty());
            if !empty {
                net.set(&k[N_NET_EXACT], &PackedFloat32Array::new().to_variant());
            }
        } else {
            let f = state.get(&k[N_F]).try_to::<PackedFloat64Array>().ok()?;
            let row = exact_row(
                f.as_slice(),
                num(&state.get(&k[N_STAMINA])),
                truth(&state.get(&k[N_EXHAUSTED])),
                num(&state.get(&k[N_DELAY])),
            )?;
            net.set(
                &k[N_NET_EXACT],
                &PackedFloat32Array::from(&row[..]).to_variant(),
            );
        }

        // GSCombatNetSync.pull
        if let Some(health) = obj(player.get(&k[N_HEALTH_OBJ])) {
            let h = health_wire(num(&health.get(&k[N_HEALTH_OBJ])), HEALTH_BITS);
            let a = health_wire(num(&health.get(&k[N_ARMOUR])), HEALTH_BITS);
            net.set(&k[N_NET_HEALTH], &h.to_variant());
            net.set(&k[N_NET_ARMOUR], &a.to_variant());
            net.set(&k[N_NET_ALIVE], &health.get(&k[N_ALIVE]));
        }

        let (from, to) = COPIES[0];
        net.set(&k[to], &player.get(&k[from]));
        let challenge = challenge_wire(int(&player.get(&k[N_CHALLENGE])));
        net.set(&k[N_NET_CHALLENGE], &challenge.to_variant());
        let value = challenge_value_wire(int(&player.get(&k[N_CHALLENGE_VALUE])));
        net.set(&k[N_NET_CHALLENGE_VALUE], &value.to_variant());
        for &(from, to) in &COPIES[1..] {
            net.set(&k[to], &player.get(&k[from]));
        }

        if authoritative && let Some(mut id) = identity {
            id.set(&k[N_ALWAYS_RELEVANT], &player.get(&k[N_BEACON]));
            if interest_aim {
                let mut p = player.clone();
                let aim = p.call(&k[N_AIM_DIRECTION], &[]);
                id.set(&k[N_INTEREST_AIM], &aim);
            }
        }
        Some(!server_owned)
    }
}
