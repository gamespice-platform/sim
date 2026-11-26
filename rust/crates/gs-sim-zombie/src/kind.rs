//! A common's mind between its five kinds (`ZzCommonBrain._decide`): idle, alert, investigate, chase and
//! lure, as an enum core. The script computes the conditions the current state reads (a bitmask, from
//! [`needs`]: it asks the horde only for what the state looks at), this answers the next state and the
//! action to run, and the script runs the side effects (the halt, the face, the search, the chase).
//!
//! Two states hand a result back before they finish, because the next step depends on what a call did:
//! [`after_search`] (investigate) and [`after_chase`] (chase).

/// The states, `go(&"idle")` and so on.
pub const IDLE: i64 = 0;
pub const ALERT: i64 = 1;
pub const INVESTIGATE: i64 = 2;
pub const CHASE: i64 = 3;
pub const LURE: i64 = 4;

/// A condition the script has computed (bit set: true).
/// `lure != Vector3.INF`.
pub const C_LURE: i64 = 1;
/// `forced != &""`.
pub const C_FORCED: i64 = 2;
/// `_woken_at != INF and now() >= _woken_at`.
pub const C_WOKEN: i64 = 4;
/// `npc.has_target() and has_reacted()`.
pub const C_REACTED: i64 = 8;
/// `LIGHT_DAMAGE` or `HEAR_COMBAT` in the tick's conditions.
pub const C_HEARD: i64 = 16;
/// `in_state_for() >= _alert_for`.
pub const C_ALERT_DONE: i64 = 32;
/// `quarry() != &""`.
pub const C_QUARRY: i64 = 64;
/// `_alert_target != &""`.
pub const C_ALERT_TARGET: i64 = 128;
/// `npc.has_target() and distance_to(npc.target_id) < 3.0`.
pub const C_TARGET_NEAR: i64 = 256;
/// `flat_distance(npc.position(), lure) > ring`.
pub const C_OUT_OF_RING: i64 = 512;

/// The actions. What the script runs in the state it is in (in chase and lure it sets the alert sight first, every tick):
/// idle: `_idle(b)` always, then the begin below if there is one.
pub const A_NONE: i64 = 0;
/// Idle: `_begin_alert(_alert_target, _alert_noise, 0.0)` after `_woken_at = INF`.
pub const A_BEGIN_WOKEN: i64 = 1;
/// Idle: `_begin_alert(forced, Vector3.INF, 0.1)`.
pub const A_BEGIN_FORCED: i64 = 2;
/// Idle: `_begin_alert(npc.target_id, Vector3.INF, 0.0)`.
pub const A_BEGIN_TARGET: i64 = 3;
/// Idle: `_begin_alert(&"", heard or its own position, 0.0)`.
pub const A_BEGIN_HEARD: i64 = 4;
/// Alert: halt, rage, face the target or the noise (then maybe leave).
pub const A_ALERT: i64 = 5;
/// Alert leaves for the chase and sets `forced = _alert_target` first.
pub const A_ALERT_FORCE: i64 = 6;
/// Investigate: run `search(..)`, then [`after_search`].
pub const A_SEARCH: i64 = 7;
/// Chase: sight, `_chase(b)`, then [`after_chase`].
pub const A_CHASE: i64 = 8;
/// Lure: go to the lure, run anim, `_unstick`.
pub const A_LURE_GO: i64 = 9;
/// Lure: halt, face the lure, rage.
pub const A_LURE_HOLD: i64 = 10;

/// The conditions [`step`] reads in `state`, so the script asks only those.
pub fn needs(state: i64) -> i64 {
    match state {
        IDLE => C_FORCED | C_WOKEN | C_REACTED | C_HEARD,
        ALERT => C_ALERT_DONE | C_QUARRY | C_ALERT_TARGET | C_FORCED,
        INVESTIGATE => C_QUARRY,
        LURE => C_LURE | C_FORCED | C_TARGET_NEAR | C_QUARRY | C_OUT_OF_RING,
        _ => 0,
    }
}

/// The lure outranks everything but a survivor in its face: the state to `go` to before the match, or
/// -1 to stay.
pub fn pre(state: i64, conditions: i64) -> i64 {
    if conditions & C_LURE != 0 && state != LURE && conditions & C_FORCED == 0 {
        return LURE;
    }
    -1
}

/// One step of `state` under `conditions`: `[next, action]` packed as `next | action << 8`; `next`
/// equal to `state` is no change of state.
pub fn step(state: i64, c: i64) -> i64 {
    let (next, act) = match state {
        IDLE => {
            if c & C_WOKEN != 0 {
                (ALERT, A_BEGIN_WOKEN)
            } else if c & C_FORCED != 0 {
                (ALERT, A_BEGIN_FORCED)
            } else if c & C_REACTED != 0 {
                (ALERT, A_BEGIN_TARGET)
            } else if c & C_HEARD != 0 {
                (ALERT, A_BEGIN_HEARD)
            } else {
                (IDLE, A_NONE)
            }
        }
        ALERT => {
            if c & C_ALERT_DONE == 0 {
                (ALERT, A_ALERT)
            } else if c & (C_QUARRY | C_ALERT_TARGET) != 0 {
                let force = c & C_FORCED == 0 && c & C_ALERT_TARGET != 0 && c & C_QUARRY == 0;
                (CHASE, if force { A_ALERT_FORCE } else { A_ALERT })
            } else {
                (INVESTIGATE, A_ALERT)
            }
        }
        INVESTIGATE => {
            if c & C_QUARRY != 0 {
                (CHASE, A_NONE)
            } else {
                (INVESTIGATE, A_SEARCH)
            }
        }
        CHASE => (CHASE, A_CHASE),
        LURE => {
            if c & C_LURE == 0 {
                (if c & C_QUARRY != 0 { CHASE } else { IDLE }, A_NONE)
            } else if c & (C_FORCED | C_TARGET_NEAR) != 0 {
                (CHASE, A_NONE)
            } else if c & C_OUT_OF_RING != 0 {
                (LURE, A_LURE_GO)
            } else {
                (LURE, A_LURE_HOLD)
            }
        }
        _ => (state, A_NONE),
    };
    next | act << 8
}

/// Investigate after `search(..)`: still running keeps the state (and the script runs the run/idle
/// animation and `_unstick`); otherwise home is set, back to idle, mill about. `[next, 1 if done]`
/// packed `next | done << 8`.
pub fn after_search(running: bool) -> i64 {
    if running { INVESTIGATE } else { IDLE | 1 << 8 }
}

/// Chase after `_chase(b)`: still chasing keeps the state; lost it and it remembers: investigate; else
/// idle (home set, shamble): `next | (1 if idle by shamble) << 8`.
pub fn after_chase(ok: bool, remembers: bool) -> i64 {
    if ok {
        CHASE
    } else if remembers {
        INVESTIGATE
    } else {
        IDLE | 1 << 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(state: i64, c: i64) -> (i64, i64) {
        let r = step(state, c);
        (r & 255, r >> 8)
    }

    #[test]
    fn a_lure_outranks_all_but_a_forced_chase() {
        assert_eq!(pre(IDLE, C_LURE), LURE);
        assert_eq!(pre(CHASE, C_LURE), LURE);
        assert_eq!(pre(LURE, C_LURE), -1);
        assert_eq!(pre(IDLE, C_LURE | C_FORCED), -1);
        assert_eq!(pre(IDLE, 0), -1);
    }

    #[test]
    fn idle_wakes_in_the_scripts_order() {
        assert_eq!(s(IDLE, 0), (IDLE, A_NONE));
        assert_eq!(s(IDLE, C_HEARD), (ALERT, A_BEGIN_HEARD));
        assert_eq!(s(IDLE, C_HEARD | C_REACTED), (ALERT, A_BEGIN_TARGET));
        assert_eq!(
            s(IDLE, C_HEARD | C_REACTED | C_FORCED),
            (ALERT, A_BEGIN_FORCED)
        );
        assert_eq!(s(IDLE, C_FORCED | C_WOKEN), (ALERT, A_BEGIN_WOKEN));
    }

    #[test]
    fn alert_waits_then_chases_or_investigates() {
        assert_eq!(s(ALERT, C_QUARRY), (ALERT, A_ALERT));
        assert_eq!(s(ALERT, C_ALERT_DONE), (INVESTIGATE, A_ALERT));
        assert_eq!(s(ALERT, C_ALERT_DONE | C_QUARRY), (CHASE, A_ALERT));
        assert_eq!(
            s(ALERT, C_ALERT_DONE | C_ALERT_TARGET),
            (CHASE, A_ALERT_FORCE)
        );
        assert_eq!(
            s(ALERT, C_ALERT_DONE | C_ALERT_TARGET | C_FORCED),
            (CHASE, A_ALERT)
        );
        assert_eq!(
            s(ALERT, C_ALERT_DONE | C_ALERT_TARGET | C_QUARRY),
            (CHASE, A_ALERT)
        );
    }

    #[test]
    fn investigate_chase_and_lure() {
        assert_eq!(s(INVESTIGATE, C_QUARRY), (CHASE, A_NONE));
        assert_eq!(s(INVESTIGATE, 0), (INVESTIGATE, A_SEARCH));
        assert_eq!(after_search(true), INVESTIGATE);
        assert_eq!(after_search(false), IDLE | 1 << 8);
        assert_eq!(s(CHASE, 0), (CHASE, A_CHASE));
        assert_eq!(after_chase(true, false), CHASE);
        assert_eq!(after_chase(false, true), INVESTIGATE);
        assert_eq!(after_chase(false, false), IDLE | 1 << 8);
        assert_eq!(s(LURE, 0), (IDLE, A_NONE));
        assert_eq!(s(LURE, C_QUARRY), (CHASE, A_NONE));
        assert_eq!(s(LURE, C_LURE | C_TARGET_NEAR), (CHASE, A_NONE));
        assert_eq!(s(LURE, C_LURE | C_FORCED), (CHASE, A_NONE));
        assert_eq!(s(LURE, C_LURE | C_OUT_OF_RING), (LURE, A_LURE_GO));
        assert_eq!(s(LURE, C_LURE), (LURE, A_LURE_HOLD));
    }

    #[test]
    fn needs_cover_what_step_reads() {
        // A condition outside `needs` never changes the answer.
        for state in IDLE..=LURE {
            let n = needs(state);
            for c in 0..1024 {
                assert_eq!(step(state, c & n), step(state, c), "state {state} c {c}");
            }
        }
    }
}
