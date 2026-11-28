//! `GSSimNetCore`'s interest submits: one peer at a time (`submit_peer_interest`, the C++ core's
//! method) or every peer of a snapshot in one call (`submit_peers_interest`), which queues the
//! same jobs in the same order, so the snapshots' bytes are the same.

use super::{GSSimNetCore, dict, job_from, num_i, object, read_ring, v3};
use godot::prelude::*;

impl GSSimNetCore {
    /// One peer's interest submit: `info` as `submit_peer_interest` takes it.
    pub(super) fn submit_interest(
        &mut self,
        observer: &Variant,
        info: &[i64],
        accumulators: &Variant,
        starved_ticks: &Variant,
    ) {
        if !self.open {
            godot_error!(
                "GSSimNetCore.submit_peer_interest outside begin_snapshot/finish_snapshot."
            );
            return;
        }
        if !self.icfg.on {
            godot_error!("GSSimNetCore.submit_peer_interest without set_interest.");
            return;
        }
        if info.len() < 10 {
            godot_error!(
                "GSSimNetCore.submit_peer_interest: info needs the budget load flag and acked ring size (ABI 4)."
            );
            return;
        }
        let mut job = job_from(info);
        let Some(ring) = read_ring(info, 9) else {
            godot_error!(
                "GSSimNetCore.submit_peer_interest: the acked ring's size does not match info."
            );
            return;
        };
        job.ring = ring;
        let mut peer = self.take_peer(job.peer_id);
        if info[8] != 0 {
            let acc = dict(accumulators).unwrap_or_default();
            let starved = dict(starved_ticks).unwrap_or_default();
            if !Self::load_budget(&mut peer, &acc, &starved) {
                godot_error!(
                    "GSSimNetCore.submit_peer_interest: a budget key is not an int net_id; that peer's budget starts empty."
                );
            }
        } else if !peer.budget.loaded {
            godot_error!(
                "GSSimNetCore.submit_peer_interest: load = 0 for a peer whose budget was never loaded (forgotten?); loading it now from dictionaries that may be stale."
            );
            let acc = dict(accumulators).unwrap_or_default();
            let starved = dict(starved_ticks).unwrap_or_default();
            Self::load_budget(&mut peer, &acc, &starved);
        }
        job.interest = true;
        if let Some(mut obs) = object(observer) {
            job.has_observer = true;
            job.obs_pos = v3(&obs.call("world_position", &[]));
            job.obs_owner = num_i(&obs.get("owner_peer_id"));
            job.obs_aim = v3(&obs.get("interest_aim"));
        }
        self.queue(job, peer, None);
    }
}

#[godot_api(secondary)]
impl GSSimNetCore {
    /// Every peer's [`Self::submit_peer_interest`] of this snapshot in one call (gs-net's
    /// `GSNetConfig.native_submit_batch`): peer `i` is `observers[i]`, `accumulators[i]`,
    /// `starved_ticks[i]` and the next run of `infos`, its info's length and then its info.
    /// The jobs are queued in this order, as the same calls one by one would queue them.
    /// Answers the peers queued; a short array or a run past the end of `infos` stops there.
    #[func]
    fn submit_peers_interest(
        &mut self,
        observers: VarArray,
        infos: PackedInt64Array,
        accumulators: VarArray,
        starved_ticks: VarArray,
    ) -> i64 {
        let n = observers.len();
        if accumulators.len() != n || starved_ticks.len() != n {
            godot_error!(
                "GSSimNetCore.submit_peers_interest: observers, accumulators and starved_ticks differ in length."
            );
            return 0;
        }
        let infos = infos.as_slice();
        let mut at = 0usize;
        for i in 0..n {
            let len = infos.get(at).copied().unwrap_or(-1);
            let end = usize::try_from(len).ok().map(|l| at + 1 + l);
            let Some(info) = end.and_then(|e| infos.get(at + 1..e)) else {
                godot_error!(
                    "GSSimNetCore.submit_peers_interest: infos ends inside peer {i}'s run."
                );
                return i as i64;
            };
            at = end.unwrap_or(at);
            self.submit_interest(
                &observers.at(i),
                info,
                &accumulators.at(i),
                &starved_ticks.at(i),
            );
        }
        n as i64
    }
}
