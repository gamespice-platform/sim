//! fc-weapons' `AwWeaponNet.pull` and gs-weapon's `GSWeaponNetSync.pull` over a carrier's weapon
//! numbers, so a server's per-player weapon pull is one call instead of a dynamic property write per
//! field. Same integers as the scripts, bit for bit; the scripts stay the specification.

/// `GSWeaponNetSync.SLOT_BITS`.
pub const SLOT_BITS: u32 = 5;
/// `GSWeaponNetSync.MAGAZINE_BITS`.
pub const MAGAZINE_BITS: u32 = 9;
/// `GSWeaponNetSync.RESERVE_BITS`.
pub const RESERVE_BITS: u32 = 11;
/// `GSWeaponNetSync.FIRE_MODE_BITS`.
pub const FIRE_MODE_BITS: u32 = 2;
/// `AwWeaponNet.FIRE_SEQ_WRAP`.
pub const FIRE_SEQ_WRAP: i64 = 1 << 4;
/// `AwWeaponNet.CHARGE_MAX`.
pub const CHARGE_MAX: f64 = 15.0;

/// What the scripts read off the arsenal and the rig.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeaponIn {
    /// `arsenal.current_slot()`.
    pub current_slot: i64,
    /// `arsenal.current() != null`; without it the fire mode, magazine and reserve are 0.
    pub has_slot: bool,
    /// The held slot's `fire_mode_index`.
    pub fire_mode_index: i64,
    /// The held slot's `magazine`.
    pub magazine: i64,
    /// `arsenal.ammo().count(slot.def.ammo_type)`.
    pub reserve: i64,
    /// The rig's `fire_seq`.
    pub fire_seq: i64,
    /// The rig's `fire_kind`.
    pub fire_kind: i64,
    /// The rig's `charge_shown`.
    pub charge_shown: f64,
    /// A charging player's `windup`; negative when not charging, and then the rig's charge stands.
    pub windup: f64,
}

/// GDScript's `roundi`: half away from zero. A NaN gives 0 here and `i64::MIN` there; no charge is NaN.
fn roundi(value: f64) -> i64 {
    value.round() as i64
}

/// GDScript's `clampf`: `value < min ? min : (value > max ? max : value)`, so NaN passes through.
fn clampf(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `[net_slot, net_fire_mode, net_magazine, net_reserve, net_fire_seq, net_fire_kind, net_charge]`.
///
/// `net_charge` is the rig's `roundi(clampf(charge_shown, 0, 1) * CHARGE_MAX)`, replaced by Fatal Contact's
/// windup draw `maxi(1, roundi(clampf(windup, 0, 1) * CHARGE_MAX))` when [`WeaponIn::windup`] is not negative.
pub fn pull(w: &WeaponIn) -> [i64; 7] {
    let slot = w.current_slot.clamp(0, (1 << SLOT_BITS) - 1);
    let (mode, magazine, reserve) = if w.has_slot {
        (
            w.fire_mode_index.clamp(0, (1 << FIRE_MODE_BITS) - 1),
            w.magazine.clamp(0, (1 << MAGAZINE_BITS) - 1),
            w.reserve.clamp(0, (1 << RESERVE_BITS) - 1),
        )
    } else {
        (0, 0, 0)
    };
    // GDScript's int `%` truncates like Rust's, so a negative count stays negative here too.
    let fire_seq = w.fire_seq % FIRE_SEQ_WRAP;
    let charge = if w.windup >= 0.0 {
        roundi(clampf(w.windup, 0.0, 1.0) * CHARGE_MAX).max(1)
    } else {
        roundi(clampf(w.charge_shown, 0.0, 1.0) * CHARGE_MAX)
    };
    [slot, mode, magazine, reserve, fire_seq, w.fire_kind, charge]
}

/// `FcContent.weapon_index`: 0 for no weapon, else the wire index plus one (an unknown id's -1 becomes 0).
pub fn weapon_code(is_empty: bool, wire_index: Option<i64>) -> i64 {
    if is_empty {
        0
    } else {
        wire_index.unwrap_or(-1) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held() -> WeaponIn {
        WeaponIn {
            current_slot: 2,
            has_slot: true,
            fire_mode_index: 1,
            magazine: 30,
            reserve: 90,
            fire_seq: 37,
            fire_kind: 1,
            charge_shown: 0.0,
            windup: -1.0,
        }
    }

    #[test]
    fn plain_held_weapon() {
        assert_eq!(pull(&held()), [2, 1, 30, 90, 5, 1, 0]);
    }

    #[test]
    fn clamps_to_the_wire() {
        let w = WeaponIn {
            current_slot: 40,
            fire_mode_index: 9,
            magazine: 900,
            reserve: 5000,
            ..held()
        };
        assert_eq!(pull(&w)[..4], [31, 3, 511, 2047]);
        let w = WeaponIn {
            current_slot: -3,
            fire_mode_index: -1,
            magazine: -5,
            reserve: -7,
            ..held()
        };
        assert_eq!(pull(&w)[..4], [0, 0, 0, 0]);
    }

    #[test]
    fn no_slot_zeroes_the_counts_but_not_the_slot() {
        let w = WeaponIn {
            has_slot: false,
            ..held()
        };
        assert_eq!(pull(&w)[..4], [2, 0, 0, 0]);
    }

    #[test]
    fn fire_seq_truncates_like_gdscript() {
        let w = WeaponIn {
            fire_seq: -17,
            ..held()
        };
        assert_eq!(pull(&w)[4], -1);
    }

    #[test]
    fn charge_and_windup() {
        let w = WeaponIn {
            charge_shown: 0.5,
            ..held()
        };
        assert_eq!(pull(&w)[6], 8, "7.5 rounds away from zero");
        let w = WeaponIn {
            charge_shown: 3.0,
            ..held()
        };
        assert_eq!(pull(&w)[6], 15);
        let w = WeaponIn {
            charge_shown: 0.9,
            windup: 0.0,
            ..held()
        };
        assert_eq!(pull(&w)[6], 1, "a fresh windup still shows");
        let w = WeaponIn {
            windup: 0.5,
            ..held()
        };
        assert_eq!(pull(&w)[6], 8);
    }

    #[test]
    fn weapon_codes() {
        assert_eq!(weapon_code(true, Some(4)), 0);
        assert_eq!(weapon_code(false, Some(4)), 5);
        assert_eq!(weapon_code(false, None), 0);
    }
}
