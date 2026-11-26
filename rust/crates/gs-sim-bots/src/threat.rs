//! A survival bot's look at a crowd: which zombies it would rather shoot, and how many commons
//! stand near. The twin of Zombie Zone's `zz_bots.gd` `_threat` scoring and `_crowd_within`,
//! over plain arrays. The sight rays stay with the script; this orders the candidates.

use crate::Vec3;
use std::collections::HashMap;

/// A plain zombie of the horde (`_crowd_within` counts these).
pub const COMMON: u8 = 1;
/// Any kind but the common: its score is lowered by [`SPECIAL_BONUS`].
pub const SPECIAL: u8 = 2;
/// A sleeper still sitting: never a threat.
pub const SITTING: u8 = 4;

/// Metres a special is moved closer in the ranking.
pub const SPECIAL_BONUS: f64 = 12.0;
/// How many of the ranked the script ray-tests at most.
pub const TOP: usize = 10;
/// Share of the body height the chest aim point sits at.
pub const AIM_FRACTION: f64 = 0.72;

/// What the script knows of one zombie that never changes during a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info {
    pub height: f64,
    pub flags: u8,
}

/// Per-zombie constants keyed by instance id, filled when a zombie spawns.
#[derive(Default, Debug)]
pub struct Table {
    map: HashMap<i64, Info>,
}

impl Table {
    pub fn set(&mut self, id: i64, height: f64, flags: u8) {
        self.map.insert(id, Info { height, flags });
    }

    pub fn remove(&mut self, id: i64) {
        self.map.remove(&id);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, id: i64) -> Option<Info> {
        self.map.get(&id).copied()
    }

    /// Turns [`SITTING`] on or off for `id`; false when it is not in the table.
    pub fn set_sitting(&mut self, id: i64, on: bool) -> bool {
        match self.map.get_mut(&id) {
            Some(i) => {
                if on {
                    i.flags |= SITTING;
                } else {
                    i.flags &= !SITTING;
                }
                true
            }
            None => false,
        }
    }
}

/// One zombie near the bot, in the order the crowd grid listed them.
#[derive(Clone, Copy, Debug)]
pub struct Cand {
    /// The crowd grid's index.
    pub index: i32,
    pub at: Vec3,
    pub info: Info,
}

/// The script's `Vector3(0, height * 0.72, 0)` added to a position.
#[inline]
pub fn aim_point(at: Vec3, height: f64) -> Vec3 {
    at + Vec3::new(0.0, (height * AIM_FRACTION) as f32, 0.0)
}

/// Ranks `near` for a bot at `feet` with eyes at `eye`: fills `out` with the grid indices of the
/// (at most [`TOP`]) it would rather shoot, best first, and returns whether it is calm (no
/// zombie, sitters included, nearer than `calm_radius`).
///
/// The order is the script's: score in millimetres above, position in `near` below, so equal
/// scores keep the grid's order.
pub fn rank(near: &[Cand], feet: Vec3, eye: Vec3, calm_radius: f64, out: &mut Vec<i32>) -> bool {
    out.clear();
    let close_sq = calm_radius * calm_radius;
    let mut calm = true;
    let mut keys: Vec<(i64, usize)> = Vec::with_capacity(near.len());

    for (i, c) in near.iter().enumerate() {
        if calm && f64::from((c.at - feet).length_squared()) < close_sq {
            calm = false;
        }
        if c.info.flags & SITTING != 0 {
            continue;
        }
        let mut score = f64::from(aim_point(c.at, c.info.height).distance_to(eye));
        if c.info.flags & SPECIAL != 0 {
            score -= SPECIAL_BONUS;
        }
        keys.push((((score + 64.0) * 1000.0) as i64, i));
    }

    // Unique (key, i) pairs: the unstable sort is deterministic.
    keys.sort_unstable();
    out.extend(keys.iter().take(TOP).map(|k| near[k.1].index));
    calm
}

/// How many of `near` are commons.
pub fn commons(near: impl Iterator<Item = Info>) -> i32 {
    near.filter(|i| i.flags & COMMON != 0).count() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(index: i32, x: f32, flags: u8) -> Cand {
        Cand {
            index,
            at: Vec3::new(x, 0.0, 0.0),
            info: Info { height: 1.7, flags },
        }
    }

    #[test]
    fn nearest_first_and_a_special_twelve_metres_ahead() {
        let near = [
            cand(0, 5.0, COMMON),
            cand(1, 9.0, SPECIAL),
            cand(2, 3.0, COMMON),
        ];
        let mut out = Vec::new();
        rank(
            &near,
            Vec3::default(),
            Vec3::new(0.0, 1.6, 0.0),
            4.0,
            &mut out,
        );
        assert_eq!(out, vec![1, 2, 0]);
    }

    #[test]
    fn equal_scores_keep_the_grid_order() {
        let near = [cand(7, 5.0, COMMON), cand(3, 5.0, COMMON)];
        let mut out = Vec::new();
        rank(&near, Vec3::default(), Vec3::default(), 1.0, &mut out);
        assert_eq!(out, vec![7, 3]);
    }

    #[test]
    fn a_sitter_is_skipped_but_still_breaks_the_calm() {
        let near = [cand(0, 1.0, COMMON | SITTING)];
        let mut out = Vec::new();
        let calm = rank(&near, Vec3::default(), Vec3::default(), 4.0, &mut out);
        assert!(out.is_empty());
        assert!(!calm);
    }

    #[test]
    fn only_ten_are_ranked() {
        let near: Vec<Cand> = (0..30).map(|i| cand(i, 40.0 - i as f32, COMMON)).collect();
        let mut out = Vec::new();
        let calm = rank(&near, Vec3::default(), Vec3::default(), 4.0, &mut out);
        assert_eq!(out.len(), TOP);
        assert_eq!(out[0], 29);
        assert_eq!(out[9], 20);
        assert!(calm);
    }

    #[test]
    fn commons_counts_only_commons() {
        let n = [COMMON, SPECIAL, COMMON, 0].map(|f| Info {
            height: 1.7,
            flags: f,
        });
        assert_eq!(commons(n.into_iter()), 2);
    }

    #[test]
    fn the_table_sits_and_stands_a_sleeper() {
        let mut t = Table::default();
        t.set(9, 1.6, SPECIAL);
        assert!(t.set_sitting(9, true));
        assert_eq!(t.get(9).unwrap().flags, SPECIAL | SITTING);
        assert!(t.set_sitting(9, false));
        assert_eq!(t.get(9).unwrap().flags, SPECIAL);
        assert!(!t.set_sitting(1, true));
    }
}
