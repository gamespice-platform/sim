//! Players' last-active ticks in a column (gs-match's `GSPlayerScore.last_active_tick` as FC's
//! players' loop writes it through `GSMatch.note_activity`), noted once a tick for the whole roster
//! and handed back to the score records only when an idle check could tell the difference.
//!
//! Every write of a record's `last_active_tick` (an activity note, a kill, an assist, an objective,
//! a join, the idle check's own stamp) is the current tick, so the true value is the latest of all
//! of them. The column keeps, per player, its latest note ([`Activity::note`]) and the latest value
//! it handed over ([`Activity::due`]). A record already holds at least what was handed over, so a
//! player whose handed-over tick is younger than the idle threshold cannot be idle either way and
//! needs no write; one that could be gets `max(record, noted)`, which is what the record would
//! hold had every note gone straight to it.

use std::collections::BTreeMap;

/// One player's column entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Entry {
    /// The latest tick noted.
    noted: i64,
    /// The latest noted tick handed to the record.
    handed: i64,
}

/// The column: player id to its entry, in id order so every answer has one order.
#[derive(Clone, Debug, Default)]
pub struct Activity {
    entries: BTreeMap<i64, Entry>,
    /// The latest tick noted or checked: a tick before it (a restarted match) clears the column.
    tick: i64,
    /// Rows noted since the last [`Activity::reset_counters`].
    pub notes: i64,
    /// Ids handed over since the last [`Activity::reset_counters`].
    pub handed: i64,
}

impl Activity {
    /// Clears the column when `tick` runs backwards (a new match counts from zero again).
    fn at(&mut self, tick: i64) {
        if tick < self.tick {
            self.entries.clear();
        }
        self.tick = tick;
    }

    /// Notes every `ids[i]` whose `rows[i]` is not -1 (the players' loop's marker for a player with
    /// no tick this time: dead, or no command pair) as active at `tick`. Only the first
    /// `min(ids, rows)` slots are read.
    pub fn note(&mut self, ids: &[i64], rows: &[i32], tick: i64) {
        self.at(tick);
        for (&id, &row) in ids.iter().zip(rows) {
            if row == -1 {
                continue;
            }
            let e = self.entries.entry(id).or_default();
            if tick > e.noted {
                e.noted = tick;
            }
            self.notes += 1;
        }
    }

    /// The players an idle check at `tick` with `threshold` could misjudge without their notes, as
    /// `[id, noted]` pairs in id order (appended to `out`): noted later than handed over, and handed
    /// over at least `threshold` ticks ago. Each is marked handed over; the caller writes
    /// `max(record, noted)` into its record.
    pub fn due(&mut self, tick: i64, threshold: i64, out: &mut Vec<i64>) {
        self.at(tick);
        for (&id, e) in &mut self.entries {
            if e.noted > e.handed && tick.saturating_sub(e.handed) >= threshold {
                e.handed = e.noted;
                out.push(id);
                out.push(e.noted);
                self.handed += 1;
            }
        }
    }

    /// Drops `id` from the column, answering its latest note when that was never handed over (the
    /// caller writes it into the record before the player leaves), else -1.
    pub fn take(&mut self, id: i64) -> i64 {
        match self.entries.remove(&id) {
            Some(e) if e.noted > e.handed => {
                self.handed += 1;
                e.noted
            }
            _ => -1,
        }
    }

    /// Every entry's latest note never handed over, as `[id, noted]` pairs in id order, all marked
    /// handed over: what a caller writes before anything else reads the records.
    pub fn flush(&mut self, out: &mut Vec<i64>) {
        self.due(self.tick, i64::MIN, out);
    }

    /// Forgets every player.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.tick = 0;
    }

    /// Players in the column.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the column holds nobody.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Zeroes [`Activity::notes`] and [`Activity::handed`].
    pub fn reset_counters(&mut self) {
        self.notes = 0;
        self.handed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The script's records: id to last_active_tick, written straight by every note.
    fn script_note(rec: &mut BTreeMap<i64, i64>, ids: &[i64], rows: &[i32], tick: i64) {
        for (&id, &row) in ids.iter().zip(rows) {
            if row != -1 {
                if let Some(v) = rec.get_mut(&id) {
                    *v = tick;
                }
            }
        }
    }

    /// gs-match's `_check_idle`: stamps and answers the idle ids in id order.
    fn check_idle(rec: &mut BTreeMap<i64, i64>, tick: i64, threshold: i64) -> Vec<i64> {
        let mut idle = vec![];
        for (&id, v) in rec.iter_mut() {
            if tick - *v >= threshold {
                *v = tick;
                idle.push(id);
            }
        }
        idle
    }

    fn hand(rec: &mut BTreeMap<i64, i64>, pairs: &[i64]) {
        for p in pairs.chunks(2) {
            if let Some(v) = rec.get_mut(&p[0]) {
                *v = (*v).max(p[1]);
            }
        }
    }

    /// A tiny deterministic generator (xorshift) so the test needs no crate.
    fn next(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    #[test]
    fn matches_the_script_idle_check_and_records() {
        let ids: Vec<i64> = (1..=12).collect();
        for threshold in [1, 3, 7, 40] {
            let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ threshold as u64;
            let mut script: BTreeMap<i64, i64> = ids.iter().map(|&i| (i, 0)).collect();
            let mut native = script.clone();
            let mut col = Activity::default();
            let mut out = vec![];
            for tick in 1..400 {
                // Players with long idle stretches, and the odd kill stamp.
                let rows: Vec<i32> = ids
                    .iter()
                    .map(|&id| {
                        let r = next(&mut seed) % 100;
                        if r < (id as u64 * 7) % 90 { -1 } else { 0 }
                    })
                    .collect();
                script_note(&mut script, &ids, &rows, tick);
                col.note(&ids, &rows, tick);
                if next(&mut seed) % 17 == 0 {
                    let k = ids[(next(&mut seed) % ids.len() as u64) as usize];
                    *script.get_mut(&k).unwrap() = tick;
                    *native.get_mut(&k).unwrap() = tick;
                }
                out.clear();
                col.due(tick, threshold, &mut out);
                hand(&mut native, &out);
                assert_eq!(
                    check_idle(&mut script, tick, threshold),
                    check_idle(&mut native, tick, threshold),
                    "tick {tick} threshold {threshold}"
                );
            }
            out.clear();
            col.flush(&mut out);
            hand(&mut native, &out);
            assert_eq!(script, native, "threshold {threshold}");
        }
    }

    #[test]
    fn take_answers_only_unhanded_notes() {
        let mut col = Activity::default();
        col.note(&[5, 6], &[0, -2], 10);
        assert_eq!(col.take(5), 10);
        assert_eq!(col.take(5), -1);
        let mut out = vec![];
        col.due(20, 5, &mut out);
        assert_eq!(out, vec![6, 10]);
        assert_eq!(col.take(6), -1);
        assert!(col.is_empty());
    }

    #[test]
    fn skips_unmarked_rows_and_clears_on_a_restart() {
        let mut col = Activity::default();
        col.note(&[1, 2, 3], &[-1, 4, 0], 50);
        assert_eq!(col.len(), 2);
        assert_eq!(col.notes, 2);
        col.note(&[1], &[0], 3);
        assert_eq!(col.len(), 1);
        let mut out = vec![];
        col.flush(&mut out);
        assert_eq!(out, vec![1, 3]);
        out.clear();
        col.flush(&mut out);
        assert!(out.is_empty());
    }
}
