//! Resident motor rows: each player's `GSFpsState` columns kept natively across ticks, so a
//! server's players' loop writes a player's state only when the script changed it (a teleport,
//! a spawn, an outside velocity) instead of copying every state in and out every tick.
//!
//! A [`Slots`] holds one row per player at a slot that stays its own until removed (removed
//! slots are reused). A tick writes each ticked row's new columns into its slot and keeps the old
//! ones in a journal, so a run's rows can be taken back ([`Slots::rollback`]) as the batch takes
//! back the hull world's feet.

/// One player's resident row: its columns (empty until the script first writes them) and the
/// caller's data beside them (the gdext side keeps the body, tunables and motor state there).
pub struct Row<T> {
    pub cols: Vec<f64>,
    pub data: T,
}

impl<T> Row<T> {
    /// Whether the script has written the row's columns (a row without them cannot tick).
    pub fn synced(&self) -> bool {
        !self.cols.is_empty()
    }
}

/// Resident rows by slot, with a free list and the journal of the current run.
pub struct Slots<T> {
    rows: Vec<Option<Row<T>>>,
    free: Vec<usize>,
    /// Batch row, slot and the columns the slot held before that row ticked.
    journal: Vec<(usize, usize, Vec<f64>)>,
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Slots {
            rows: Vec::new(),
            free: Vec::new(),
            journal: Vec::new(),
        }
    }
}

impl<T> Slots<T> {
    /// A new row for `data` without columns; its slot.
    pub fn add(&mut self, data: T) -> usize {
        let row = Some(Row {
            cols: Vec::new(),
            data,
        });
        match self.free.pop() {
            Some(slot) => {
                self.rows[slot] = row;
                slot
            }
            None => {
                self.rows.push(row);
                self.rows.len() - 1
            }
        }
    }

    /// Drops the row at `slot`; its data, or `None` when there was none. Its journal entries go
    /// with it (a removed row is not given back by [`Self::rollback`]).
    pub fn remove(&mut self, slot: usize) -> Option<T> {
        let row = self.rows.get_mut(slot)?.take()?;
        self.journal.retain(|(_, s, _)| *s != slot);
        self.free.push(slot);
        Some(row.data)
    }

    pub fn get(&self, slot: usize) -> Option<&Row<T>> {
        self.rows.get(slot)?.as_ref()
    }

    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Row<T>> {
        self.rows.get_mut(slot)?.as_mut()
    }

    /// Rows held.
    pub fn len(&self) -> usize {
        self.rows.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The script's write of a row's columns (`cols` whole); false when there is no such row.
    pub fn push(&mut self, slot: usize, cols: &[f64]) -> bool {
        let Some(row) = self.get_mut(slot) else {
            return false;
        };
        row.cols.clear();
        row.cols.extend_from_slice(cols);
        true
    }

    /// Starts a run: forgets the journal.
    pub fn begin(&mut self) {
        self.journal.clear();
    }

    /// Batch row `row` ticked the row at `slot` to `new`: kept, the old columns journalled.
    pub fn ticked(&mut self, row: usize, slot: usize, new: &[f64]) {
        let Some(r) = self.rows.get_mut(slot).and_then(Option::as_mut) else {
            return;
        };
        let old = std::mem::replace(&mut r.cols, new.to_vec());
        self.journal.push((row, slot, old));
    }

    /// Takes batch rows `from` and later back: their slots hold again what they held before.
    pub fn rollback(&mut self, from: usize) {
        while let Some((row, _, _)) = self.journal.last() {
            if *row < from {
                break;
            }
            if let Some((_, slot, old)) = self.journal.pop()
                && let Some(r) = self.rows.get_mut(slot).and_then(Option::as_mut)
            {
                r.cols = old;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_reused_after_remove() {
        let mut s = Slots::default();
        let a = s.add('a');
        let b = s.add('b');
        assert_ne!(a, b);
        assert_eq!(s.len(), 2);
        assert_eq!(s.remove(a), Some('a'));
        assert_eq!(s.remove(a), None);
        assert!(s.get(a).is_none());
        let c = s.add('c');
        assert_eq!(c, a);
        assert_eq!(s.get(c).map(|r| r.data), Some('c'));
        assert_eq!(s.len(), 2);
        assert!(!s.push(99, &[1.0]));
    }

    #[test]
    fn a_row_ticks_only_once_written() {
        let mut s = Slots::default();
        let a = s.add(());
        assert!(!s.get(a).is_some_and(Row::synced));
        assert!(s.push(a, &[1.0, 2.0]));
        assert!(s.get(a).is_some_and(Row::synced));
        assert!(s.push(a, &[3.0]));
        assert_eq!(s.get(a).map(|r| r.cols.clone()), Some(vec![3.0]));
    }

    #[test]
    fn rollback_gives_back_the_rows_from_there() {
        let mut s = Slots::default();
        let (a, b, c) = (s.add(()), s.add(()), s.add(()));
        for slot in [a, b, c] {
            s.push(slot, &[slot as f64]);
        }
        s.begin();
        s.ticked(0, a, &[10.0]);
        s.ticked(1, b, &[11.0]);
        s.ticked(2, c, &[12.0]);
        s.rollback(1);
        let cols = |s: &Slots<()>, slot| s.get(slot).map(|r| r.cols[0]);
        assert_eq!(cols(&s, a), Some(10.0));
        assert_eq!(cols(&s, b), Some(b as f64));
        assert_eq!(cols(&s, c), Some(c as f64));
        // A new run forgets the journal: nothing more is taken back.
        s.begin();
        s.rollback(0);
        assert_eq!(cols(&s, a), Some(10.0));
    }

    #[test]
    fn a_removed_row_is_not_given_back() {
        let mut s = Slots::default();
        let a = s.add(());
        s.push(a, &[1.0]);
        s.begin();
        s.ticked(0, a, &[2.0]);
        s.remove(a);
        let b = s.add(());
        assert_eq!(a, b);
        s.rollback(0);
        assert!(!s.get(b).is_some_and(Row::synced));
    }
}
