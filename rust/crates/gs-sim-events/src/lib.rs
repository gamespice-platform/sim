//! A tick's event rows: what gs-player-controller's `GSFpsController.end_tick` used to emit one
//! signal at a time (`mode_changed`, `landed`, `jumped`, `stepped_up`, `crouch_changed`,
//! `slide_changed`, `launched`, `dashed`, `wall_jumped`, `wall_run_changed`, `view_turned`),
//! derived for every player of a tick from packed before and after rows into one table the game
//! drains after the tick and dispatches in one typed loop.
//!
//! Input rows are [`IN`] f64s per player (the `IN_*` columns): the values the controller captured
//! before its motor ran and the same values after, plus the motor's counters and view turn. The
//! table holds [`INTS`] i32s (`[entity, kind, i0, i1]`) and [`FLOATS`] f64s (`[f0, f1]`) per event,
//! in the order the controller emitted them: per player, in input order, mode, landing or jump,
//! step, crouch, then (not while replaying) slide, launch, dash, wall jump, wall run, view turn.
//! The arithmetic is the script's: f64 throughout (`absf`, the step's subtraction and bound).
//!
//! The script twin is `GSFpsTickEvents` (gs-player-controller 50); the two agree to the bit.

/// Floats per input row.
pub const IN: usize = 18;
/// The entity index the game gave the controller (its roster index).
pub const IN_ENTITY: usize = 0;
/// Flags: [`FLAG_REPLAYING`].
pub const IN_FLAGS: usize = 1;
/// `GSFpsTunables.step_height`.
pub const IN_STEP_HEIGHT: usize = 2;
/// `GSFpsState.mode` before the motor.
pub const IN_B_MODE: usize = 3;
/// `GSFpsState.position.y` before the motor.
pub const IN_B_Y: usize = 4;
/// `GSFpsState.velocity.y` before the motor.
pub const IN_B_VY: usize = 5;
/// [`BIT_CROUCHED`], [`BIT_SLIDING`], [`BIT_WALL_RUNNING`] before the motor.
pub const IN_B_BITS: usize = 6;
/// `GSFpsMotor.launches` before the motor.
pub const IN_B_LAUNCHES: usize = 7;
/// `GSFpsMotor.dashes` before the motor.
pub const IN_B_DASHES: usize = 8;
/// `GSFpsMotor.wall_jumps` before the motor.
pub const IN_B_WALL_JUMPS: usize = 9;
/// `GSFpsState.mode` after the motor.
pub const IN_A_MODE: usize = 10;
/// `GSFpsState.position.y` after the motor.
pub const IN_A_Y: usize = 11;
/// `GSFpsState.velocity.y` after the motor.
pub const IN_A_VY: usize = 12;
/// The bits after the motor.
pub const IN_A_BITS: usize = 13;
/// `GSFpsMotor.launches` after the motor.
pub const IN_A_LAUNCHES: usize = 14;
/// `GSFpsMotor.dashes` after the motor.
pub const IN_A_DASHES: usize = 15;
/// `GSFpsMotor.wall_jumps` after the motor.
pub const IN_A_WALL_JUMPS: usize = 16;
/// `GSFpsMotor.view_turn` after the motor.
pub const IN_VIEW_TURN: usize = 17;

/// The tick is a replay: slide, launch, dash, wall and view events are not emitted.
pub const FLAG_REPLAYING: i64 = 1;
/// `GSFpsState.is_crouched()`.
pub const BIT_CROUCHED: i64 = 1;
/// `GSFpsState.is_sliding()`.
pub const BIT_SLIDING: i64 = 2;
/// `GSFpsState.is_wall_running()`.
pub const BIT_WALL_RUNNING: i64 = 4;

/// `GSFpsState.Mode.GROUND`.
pub const MODE_GROUND: i32 = 0;
/// `GSFpsState.Mode.AIR`.
pub const MODE_AIR: i32 = 1;

/// I32s per event: entity, kind, i0, i1.
pub const INTS: usize = 4;
/// F64s per event: f0, f1.
pub const FLOATS: usize = 2;

/// Event kinds (`GSFpsTickEvents.Kind`); appended, never inserted.
pub mod kind {
    /// `mode_changed(i0, i1)`.
    pub const MODE_CHANGED: i32 = 0;
    /// `landed(f0)`.
    pub const LANDED: i32 = 1;
    /// `jumped()`.
    pub const JUMPED: i32 = 2;
    /// `stepped_up(f0)`.
    pub const STEPPED_UP: i32 = 3;
    /// `crouch_changed(i0 != 0)`.
    pub const CROUCH_CHANGED: i32 = 4;
    /// `slide_changed(i0 != 0)`.
    pub const SLIDE_CHANGED: i32 = 5;
    /// `launched()`.
    pub const LAUNCHED: i32 = 6;
    /// `dashed()`.
    pub const DASHED: i32 = 7;
    /// `wall_jumped()`.
    pub const WALL_JUMPED: i32 = 8;
    /// `wall_run_changed(i0 != 0)`.
    pub const WALL_RUN_CHANGED: i32 = 9;
    /// `view_turned(f0)`.
    pub const VIEW_TURNED: i32 = 10;
    /// How many kinds there are.
    pub const COUNT: i32 = 11;
}

/// A tick's events, packed: [`INTS`] i32s and [`FLOATS`] f64s per event.
#[derive(Default, Debug, Clone)]
pub struct EventTable {
    /// `[entity, kind, i0, i1]` per event.
    pub ints: Vec<i32>,
    /// `[f0, f1]` per event.
    pub floats: Vec<f64>,
}

impl EventTable {
    /// Empties the table, keeping its capacity.
    pub fn clear(&mut self) {
        self.ints.clear();
        self.floats.clear();
    }

    /// Events in the table.
    pub fn len(&self) -> usize {
        self.ints.len() / INTS
    }

    /// Whether the table has no event.
    pub fn is_empty(&self) -> bool {
        self.ints.is_empty()
    }

    /// Appends one event.
    pub fn push(&mut self, entity: i32, kind: i32, i: [i32; 2], f: [f64; 2]) {
        self.ints.extend_from_slice(&[entity, kind, i[0], i[1]]);
        self.floats.extend_from_slice(&f);
    }

    /// Appends the events of every [`IN`]-float row of `rows`, in row order.
    pub fn derive_rows(&mut self, rows: &[f64]) {
        for row in rows.as_chunks::<IN>().0 {
            self.derive_row(row);
        }
    }

    /// Appends one player's events from its input row, in the controller's order.
    pub fn derive_row(&mut self, row: &[f64]) {
        let e = row[IN_ENTITY] as i32;
        let replaying = (row[IN_FLAGS] as i64) & FLAG_REPLAYING != 0;
        let before = row[IN_B_MODE] as i32;
        let after = row[IN_A_MODE] as i32;
        if before != after {
            self.push(e, kind::MODE_CHANGED, [before, after], [0.0, 0.0]);
            if after == MODE_GROUND && before == MODE_AIR {
                self.push(e, kind::LANDED, [0, 0], [row[IN_B_VY].abs(), 0.0]);
            } else if after == MODE_AIR && before == MODE_GROUND && row[IN_A_VY] > 0.0 {
                self.push(e, kind::JUMPED, [0, 0], [0.0, 0.0]);
            }
        }
        if after == MODE_GROUND {
            let gained = row[IN_A_Y] - row[IN_B_Y];
            if gained > 0.001 && gained <= row[IN_STEP_HEIGHT] + 0.01 {
                self.push(e, kind::STEPPED_UP, [0, 0], [gained, 0.0]);
            }
        }
        let bb = row[IN_B_BITS] as i64;
        let ab = row[IN_A_BITS] as i64;
        self.edge(e, kind::CROUCH_CHANGED, bb, ab, BIT_CROUCHED);
        if replaying {
            return;
        }
        self.edge(e, kind::SLIDE_CHANGED, bb, ab, BIT_SLIDING);
        let counters = [
            (IN_B_LAUNCHES, IN_A_LAUNCHES, kind::LAUNCHED),
            (IN_B_DASHES, IN_A_DASHES, kind::DASHED),
            (IN_B_WALL_JUMPS, IN_A_WALL_JUMPS, kind::WALL_JUMPED),
        ];
        for (b, a, k) in counters {
            if row[a] != row[b] {
                self.push(e, k, [0, 0], [0.0, 0.0]);
            }
        }
        self.edge(e, kind::WALL_RUN_CHANGED, bb, ab, BIT_WALL_RUNNING);
        if row[IN_VIEW_TURN] != 0.0 {
            self.push(e, kind::VIEW_TURNED, [0, 0], [row[IN_VIEW_TURN], 0.0]);
        }
    }

    fn edge(&mut self, e: i32, k: i32, before: i64, after: i64, bit: i64) {
        if (before ^ after) & bit != 0 {
            self.push(e, k, [i32::from(after & bit != 0), 0], [0.0, 0.0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(entity: f64) -> [f64; IN] {
        let mut r = [0.0; IN];
        r[IN_ENTITY] = entity;
        r[IN_STEP_HEIGHT] = 0.35;
        r
    }

    fn kinds(t: &EventTable) -> Vec<i32> {
        t.ints.chunks_exact(INTS).map(|c| c[1]).collect()
    }

    #[test]
    fn quiet_row_has_no_event() {
        let mut t = EventTable::default();
        t.derive_row(&row(3.0));
        assert!(t.is_empty());
    }

    #[test]
    fn landing_reports_mode_then_impact() {
        let mut t = EventTable::default();
        let mut r = row(5.0);
        r[IN_B_MODE] = 1.0;
        r[IN_B_VY] = -7.25;
        t.derive_row(&r);
        assert_eq!(
            t.ints,
            vec![5, kind::MODE_CHANGED, 1, 0, 5, kind::LANDED, 0, 0]
        );
        assert_eq!(t.floats[2], 7.25);
    }

    #[test]
    fn jump_needs_upward_speed() {
        let mut t = EventTable::default();
        let mut r = row(0.0);
        r[IN_A_MODE] = 1.0;
        t.derive_row(&r);
        assert_eq!(kinds(&t), vec![kind::MODE_CHANGED]);
        t.clear();
        r[IN_A_VY] = 4.0;
        t.derive_row(&r);
        assert_eq!(kinds(&t), vec![kind::MODE_CHANGED, kind::JUMPED]);
    }

    #[test]
    fn step_is_bounded_by_step_height() {
        let mut t = EventTable::default();
        let mut r = row(1.0);
        r[IN_B_Y] = 1.0;
        r[IN_A_Y] = 1.3;
        t.derive_row(&r);
        assert_eq!(kinds(&t), vec![kind::STEPPED_UP]);
        assert_eq!(t.floats[0], 1.3 - 1.0);
        t.clear();
        r[IN_A_Y] = 1.5;
        t.derive_row(&r);
        assert!(t.is_empty());
    }

    #[test]
    fn replay_keeps_only_mode_step_and_crouch() {
        let mut t = EventTable::default();
        let mut r = row(2.0);
        r[IN_A_BITS] = 7.0;
        r[IN_A_LAUNCHES] = 1.0;
        r[IN_A_DASHES] = 1.0;
        r[IN_A_WALL_JUMPS] = 1.0;
        r[IN_VIEW_TURN] = 0.5;
        t.derive_row(&r);
        assert_eq!(
            kinds(&t),
            vec![
                kind::CROUCH_CHANGED,
                kind::SLIDE_CHANGED,
                kind::LAUNCHED,
                kind::DASHED,
                kind::WALL_JUMPED,
                kind::WALL_RUN_CHANGED,
                kind::VIEW_TURNED
            ]
        );
        t.clear();
        r[IN_FLAGS] = FLAG_REPLAYING as f64;
        t.derive_row(&r);
        assert_eq!(t.ints, vec![2, kind::CROUCH_CHANGED, 1, 0]);
    }

    #[test]
    fn rows_keep_input_order() {
        let mut t = EventTable::default();
        let mut a = row(9.0);
        a[IN_A_BITS] = 1.0;
        let mut b = row(4.0);
        b[IN_B_BITS] = 1.0;
        let rows: Vec<f64> = a.iter().chain(b.iter()).copied().collect();
        t.derive_rows(&rows);
        assert_eq!(
            t.ints,
            vec![9, kind::CROUCH_CHANGED, 1, 0, 4, kind::CROUCH_CHANGED, 0, 0]
        );
        assert_eq!(t.len(), 2);
    }
}
