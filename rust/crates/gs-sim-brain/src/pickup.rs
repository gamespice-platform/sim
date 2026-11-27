//! GSBotPickup's `_choose` and `gain`: which weapon lying about a quiet bot walks to. The script's
//! arithmetic in its order and precision (positions `f32`, the drop's numbers `f64`), and sight asked
//! for the same drops in the same order, so the native choice is the script's to the bit.

use crate::num::{Vec3, scale3};

/// GSBotPickup.MIN_GAIN: least gain worth a walk.
pub const MIN_GAIN: f64 = 0.1;
/// GSBotPickup.LOW_AMMO: under this fraction of ammunition a bot wants more of the same slot.
pub const LOW_AMMO: f64 = 0.3;
/// One packed drop row (GSBotWorld's drop table): slot, value, ammo.
pub const STRIDE: usize = 3;
/// One packed loadout entry: slot, value, ammo (only the slots holding a dictionary).
pub const MINE_STRIDE: usize = 3;

/// `GSBotPickup.gain`: how much better a drop of `value` and `ammo` is than `mine` (value, ammo)
/// in its slot, `None` when the slot holds nothing.
#[inline]
pub fn gain(mine: Option<(f64, f64)>, value: f64, ammo: f64) -> f64 {
    let Some((mine_value, mine_ammo)) = mine else {
        return value;
    };
    let mut g = value - mine_value;
    if mine_ammo < LOW_AMMO && ammo > mine_ammo + 0.3 {
        // maxf(g, -0.05) as the engine's MAX: the first unless it is less than the second.
        g = if g < -0.05 { -0.05 } else { g } + 0.15;
    }
    g
}

/// The loadout entry of `slot` in `mine` (`MINE_STRIDE` each), as `me.loadout.get(slot)`.
#[inline]
pub fn mine_of(mine: &[f64], slot: f64) -> Option<(f64, f64)> {
    mine.as_chunks::<MINE_STRIDE>()
        .0
        .iter()
        .find(|e| e[0] == slot)
        .map(|e| (e[1], e[2]))
}

/// The drops a bot leaves alone: ids and, beside each, until when.
#[derive(Clone, Copy, Debug, Default)]
pub struct Skip<'a> {
    pub ids: &'a [i64],
    pub until: &'a [f64],
}

impl Skip<'_> {
    /// `float(_skip.get(id, -INF)) > now`.
    #[inline]
    pub fn holds(&self, id: i64, now: f64) -> bool {
        self.ids
            .iter()
            .position(|&x| x == id)
            .is_some_and(|i| self.until.get(i).is_some_and(|&t| t > now))
    }
}

/// What `_choose` reads besides the drops.
#[derive(Clone, Copy, Debug)]
pub struct ChooseIn {
    /// The bot's position.
    pub pos: Vec3,
    /// `p.pickup_range`, metres.
    pub range: f64,
    /// `p.through_walls >= 0.5`: no sight asked.
    pub through: bool,
    pub now: f64,
}

/// `GSBotPickup._choose` over the drop table (`ids`, `at`, `rows` of `STRIDE`): the row of the best
/// drop or `None`. `sees(to)` is the world's line of sight from the bot's eye to `to`, asked only
/// for a drop in range, worth a walk and not skipped, in row order, as the script asks it.
pub fn choose(
    ids: &[i64],
    at: &[Vec3],
    rows: &[f64],
    mine: &[f64],
    skip: Skip<'_>,
    inp: &ChooseIn,
    mut sees: impl FnMut(Vec3) -> bool,
) -> Option<usize> {
    let lift = scale3(Vec3::new(0.0, 1.0, 0.0), 0.3);
    let mut best: Option<usize> = None;
    let mut best_score = f64::NEG_INFINITY;
    for (i, (&id, &p)) in ids.iter().zip(at).enumerate() {
        if skip.holds(id, inp.now) {
            continue;
        }
        let dist = inp.pos.distance_to(p) as f64;
        if dist > inp.range {
            continue;
        }
        let row = &rows[i * STRIDE..i * STRIDE + STRIDE];
        let g = gain(mine_of(mine, row[0]), row[1], row[2]);
        if g < MIN_GAIN {
            continue;
        }
        if !inp.through && !sees(Vec3::new(p.x + lift.x, p.y + lift.y, p.z + lift.z)) {
            continue;
        }
        let score = g - dist * 0.02;
        if score > best_score {
            best_score = score;
            best = Some(i);
        }
    }
    best
}

/// `GSBotArms.ammo_fraction`: how full a weapon of `full` rounds (magazine plus reserve cap) is with
/// `magazine` and `reserve` left, 1 for an infinite reserve. The script's ints and its `f64` divide.
#[inline]
pub fn ammo_fraction(infinite: bool, full: i64, magazine: i64, reserve: i64) -> f64 {
    if infinite {
        return 1.0;
    }
    // clampf as the engine's CLAMP; the quotient of two ints is never NaN, so f64::clamp is the same.
    (magazine.wrapping_add(reserve) as f64 / full.max(1) as f64).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp(range: f64, through: bool) -> ChooseIn {
        ChooseIn {
            pos: Vec3::new(0.0, 0.0, 0.0),
            range,
            through,
            now: 10.0,
        }
    }

    #[test]
    fn gain_of_an_empty_slot_is_the_value() {
        assert_eq!(gain(None, 0.7, 0.0), 0.7);
    }

    #[test]
    fn low_ammo_wants_the_same_weapon_with_more() {
        // Same value, mine nearly dry, the drop full: maxf(0, -0.05) + 0.15.
        assert_eq!(gain(Some((0.5, 0.1)), 0.5, 1.0), 0.15);
        // A worse weapon is floored at -0.05 first.
        assert_eq!(gain(Some((0.9, 0.1)), 0.5, 1.0), -0.05 + 0.15);
        // Not dry enough: the plain difference.
        assert_eq!(gain(Some((0.5, 0.5)), 0.8, 1.0), 0.8 - 0.5);
    }

    #[test]
    fn choose_takes_the_best_score_in_range_and_in_sight() {
        let ids = [7, 8, 9, 10];
        let at = [
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(6.0, 0.0, 0.0),
            Vec3::new(50.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
        ];
        // slot, value, ammo: 8 is the best but hidden, 9 out of range, 10 skipped.
        let rows = [1.0, 0.6, 1.0, 1.0, 0.9, 1.0, 1.0, 2.0, 1.0, 1.0, 0.95, 1.0];
        let mine = [1.0, 0.3, 1.0];
        let skip = Skip {
            ids: &[10],
            until: &[30.0],
        };
        let mut asked = Vec::new();
        let got = choose(&ids, &at, &rows, &mine, skip, &inp(20.0, false), |to| {
            asked.push(to);
            to.x < 5.0
        });
        assert_eq!(got, Some(0));
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].y, 0.3f32);
        // Through walls nothing is asked and the hidden one wins.
        let got = choose(&ids, &at, &rows, &mine, skip, &inp(20.0, true), |_| {
            panic!("sight asked through walls")
        });
        assert_eq!(got, Some(1));
        // An expired skip is no skip.
        let skip = Skip {
            ids: &[10],
            until: &[5.0],
        };
        let got = choose(&ids, &at, &rows, &mine, skip, &inp(20.0, true), |_| true);
        assert_eq!(got, Some(3));
    }

    #[test]
    fn nothing_worth_a_walk_is_none() {
        let ids = [1];
        let at = [Vec3::new(1.0, 0.0, 0.0)];
        let rows = [0.0, 0.35, 1.0];
        let mine = [0.0, 0.3, 1.0];
        let (skip, i) = (Skip::default(), inp(20.0, true));
        let got = choose(&ids, &at, &rows, &mine, skip, &i, |_| true);
        assert_eq!(got, None);
    }

    #[test]
    fn ammo_fraction_is_the_scripts() {
        assert_eq!(ammo_fraction(true, 0, 0, 0), 1.0);
        assert_eq!(ammo_fraction(false, 120, 30, 60), 0.75);
        assert_eq!(ammo_fraction(false, 0, 0, 0), 0.0);
        assert_eq!(ammo_fraction(false, 0, 5, 0), 1.0);
        assert_eq!(ammo_fraction(false, 90, 30, 90), 1.0);
        assert_eq!(ammo_fraction(false, 90, -5, 0), 0.0);
        assert_eq!(ammo_fraction(false, 7, 1, 2), 3.0 / 7.0);
    }
}
