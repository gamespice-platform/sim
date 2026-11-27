//! The choice GSBotBrain._perceive makes over a scan, resumable: the current target kept while in
//! sight, else the best score seen. Sight is asked through an oracle that may not know yet
//! ([`Pick::Need`]); the caller finds out (a ray on the main thread) and asks again with the same
//! [`Found`], which picks up where it stopped and ends on the same row an always-answering oracle
//! gives. A row's answer is asked at most once per choice when the oracle answers at once.

use crate::{Contacts, Found};

/// Where a choice stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The chosen row of the table, -1 for none.
    Chosen(i32),
    /// Sight must be found out about this row of the table before the choice can go on.
    Need(usize),
}

/// The choice over `found` (from [`crate::candidates`], `visible` and `target_id` as after a dead
/// target is forgotten). `sees(row)` is whether the bot sees the body at `row`, `None` when not yet
/// known. `through`: the bot sees through walls, nothing is asked. Rows given up are scored
/// infinite in `found`, so a call after [`Pick::Need`] goes on from there.
pub fn choose(
    found: &mut Found,
    table: &Contacts,
    target_id: i64,
    visible: bool,
    through: bool,
    mut sees: impl FnMut(usize) -> Option<bool>,
) -> Pick {
    if visible {
        for i in 0..found.index.len() {
            let row = found.index[i] as usize;
            if table.ids[row] == target_id {
                let seen = through
                    || match sees(row) {
                        Some(seen) => seen,
                        None => return Pick::Need(row),
                    };
                found.scores[i] = f32::INFINITY;
                if seen {
                    return Pick::Chosen(found.index[i]);
                }
                break;
            }
        }
    }
    loop {
        let mut bi: Option<usize> = None;
        let mut best = f64::INFINITY;
        for (i, s) in found.scores.iter().enumerate() {
            if (*s as f64) < best {
                best = *s as f64;
                bi = Some(i);
            }
        }
        let Some(bi) = bi else {
            return Pick::Chosen(-1);
        };
        let row = found.index[bi] as usize;
        let seen = through
            || match sees(row) {
                Some(seen) => seen,
                None => return Pick::Need(row),
            };
        if seen {
            return Pick::Chosen(found.index[bi]);
        }
        found.scores[bi] = f32::INFINITY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gs_sim_math::Vec3;

    fn table(n: usize) -> Contacts {
        Contacts {
            ids: (0..n as i64).map(|i| 100 + i).collect(),
            points: (0..n).map(|i| Vec3::new(i as f32, 0.0, 0.0)).collect(),
            teams: vec![1; n],
            alive: vec![true; n],
        }
    }

    fn found(scores: &[f32]) -> Found {
        Found {
            index: (0..scores.len() as i32).collect(),
            scores: scores.to_vec(),
            target_dead: false,
        }
    }

    /// The same row whether the oracle answers at once or one unknown row at a time.
    fn agree(scores: &[f32], seen: &[bool], target: i64, visible: bool) {
        let t = table(scores.len());
        let mut f = found(scores);
        let direct = choose(&mut f, &t, target, visible, false, |r| Some(seen[r]));
        let mut f = found(scores);
        let mut known = vec![None; scores.len()];
        let mut asked = 0;
        let resumed = loop {
            match choose(&mut f, &t, target, visible, false, |r| known[r]) {
                Pick::Need(r) => {
                    assert!(known[r].is_none(), "row {r} asked twice");
                    known[r] = Some(seen[r]);
                    asked += 1;
                }
                done => break done,
            }
        };
        assert_eq!(direct, resumed);
        assert!(asked <= scores.len());
    }

    #[test]
    fn keeps_the_visible_target_in_sight() {
        let t = table(3);
        let mut f = found(&[1.0, 5.0, 0.5]);
        assert_eq!(
            choose(&mut f, &t, 101, true, false, |_| Some(true)),
            Pick::Chosen(1)
        );
    }

    #[test]
    fn best_seen_score_wins_when_the_target_is_hidden() {
        let t = table(3);
        let mut f = found(&[1.0, 5.0, 0.5]);
        let seen = [true, false, false];
        assert_eq!(
            choose(&mut f, &t, 101, true, false, |r| Some(seen[r])),
            Pick::Chosen(0)
        );
    }

    #[test]
    fn through_walls_asks_nothing() {
        let t = table(2);
        let mut f = found(&[3.0, 2.0]);
        assert_eq!(
            choose(&mut f, &t, -1, false, true, |_| None),
            Pick::Chosen(1)
        );
    }

    #[test]
    fn nothing_seen_is_none() {
        let t = table(2);
        let mut f = found(&[3.0, 2.0]);
        assert_eq!(
            choose(&mut f, &t, -1, false, false, |_| Some(false)),
            Pick::Chosen(-1)
        );
    }

    #[test]
    fn resumed_choice_matches_the_direct_one() {
        let mut x: u32 = 0x9e37_79b9;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for _ in 0..500 {
            let n = (next() % 7) as usize;
            let scores: Vec<f32> = (0..n).map(|_| (next() % 9) as f32).collect();
            let seen: Vec<bool> = (0..n).map(|_| next() % 3 == 0).collect();
            let target = 100 + (next() % 8) as i64;
            agree(&scores, &seen, target, next() % 2 == 0);
        }
    }
}
