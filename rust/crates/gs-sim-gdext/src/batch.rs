//! `GSSimMotorBatch`: a run of players' whole motor ticks in one call, so a server's players'
//! loop crosses into native code once per run instead of once per player.
//!
//! Each row is `GSSimFpsBody.motor_tick` to the bit, in roster order: a body in hulls mode 3
//! gathers from the hull world as the single call does, and once it has ticked the world holds
//! its new feet, as the game's `moved` after that player's tick would. The old feet are kept so
//! the caller can take rows back when a player ends its tick somewhere else.
//!
//! Since gs-player-controller 49 the players' columns stay here between ticks
//! ([`GSSimMotorBatch::slot_add`], `gs_sim_motor::slots`): the script writes a player's state only
//! when it changed it ([`GSSimMotorBatch::slot_push`]) and reads back one packed array per run.

use crate::hulls::GSSimHullWorld;
use crate::motor::{FULL_TUN_LEN, GSSimFpsBody, TICK_IN_LEN, TICK_OUT_LEN, TICK_TUN_LEN};
use crate::motor_state::GSSimMotorState;
use godot::prelude::*;
use gs_sim_motor::Vec3 as V3;
use gs_sim_motor::cols;
use gs_sim_motor::crouch;
use gs_sim_motor::slots::Slots;
use gs_sim_motor::state::{self as mstate, Rest};

const _: () = assert!(TICK_IN_LEN == cols::ROW_IN && TICK_OUT_LEN == cols::ROW_OUT);
const _: () = assert!(TICK_TUN_LEN + FULL_TUN_LEN == crouch::TUN_AT);

/// Per row in [`GSSimMotorBatch::run`]'s `meta`: hulls mode (0 or 3), collision mask, pass-out
/// mask; [`GSSimMotorBatch::run_rows`] adds the row's timers flags (`tick_into`'s `timers`).
const META_LEN: usize = 3;
const ROWS_META_LEN: usize = 4;
/// Values per ticked row in [`GSSimMotorBatch::run_slots`]'s answer: the tick's answer, then the
/// row's new columns.
const SLOT_STRIDE: usize = cols::ROW_OUT + crouch::COLS_LEN;

/// A row's body and tunables, as a run ticks it.
type Src = (Gd<GSSimFpsBody>, PackedFloat64Array);
/// Per state row: the columns and height its crouch left, when it changed any.
type Crouched = Option<(Vec<f64>, f64)>;

/// A resident player's native side beside its columns.
struct SlotData {
    body: Gd<GSSimFpsBody>,
    tun: PackedFloat64Array,
    state: Gd<GSSimMotorState>,
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimMotorBatch {
    /// Row, world, slot and the feet the world held before the row ran.
    moved: Vec<(usize, Gd<GSSimHullWorld>, i64, V3)>,
    out: Vec<f64>,
    /// [`Self::run_cols`]'s input rows, kept between calls.
    cols_in: Vec<f64>,
    /// Row, state and the rest stamp it held before [`Self::run_states`] ticked the row.
    rests: Vec<(usize, Gd<GSSimMotorState>, Rest)>,
    /// The players' resident rows ([`Self::slot_add`]).
    slots: Slots<SlotData>,
    rows: i64,
    runs: i64,
}

#[godot_api]
impl GSSimMotorBatch {
    /// Starts a players' loop: forgets the feet kept for [`Self::rollback`].
    #[func]
    fn begin(&mut self) {
        self.moved.clear();
        self.rests.clear();
        self.slots.begin();
    }

    /// Ticks rows `from` to `to` (exclusive) in order: `bodies` the `GSSimFpsBody` per row,
    /// `tuns` its tunables, `meta` three values per row, `input` `motor_tick`'s state per row.
    /// Answers `motor_tick`'s values per row ticked; shorter than asked when a row has no native
    /// answer (the rows before it stand, that row and the ones after are the caller's).
    #[func]
    fn run(
        &mut self,
        bodies: VarArray,
        tuns: VarArray,
        meta: PackedInt64Array,
        input: PackedFloat64Array,
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        let src = var_src(&bodies, &tuns, from, to);
        self.ticks(&src, meta.as_slice(), META_LEN, input.as_slice(), from);
        PackedFloat64Array::from(&self.out[..])
    }

    /// [`Self::run`] with four values per row in `meta`, the fourth `tick_into`'s `timers`: a row
    /// may leave its clocks and cooldowns to this call; gs-player-controller 31 and later call it.
    #[func]
    fn run_rows(
        &mut self,
        bodies: VarArray,
        tuns: VarArray,
        meta: PackedInt64Array,
        input: PackedFloat64Array,
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        let src = var_src(&bodies, &tuns, from, to);
        self.ticks(&src, meta.as_slice(), ROWS_META_LEN, input.as_slice(), from);
        PackedFloat64Array::from(&self.out[..])
    }

    /// [`Self::run_rows`] over gs-player-controller 36's packed states: `cols` holds each row's
    /// `GSFpsState` columns (a `PackedFloat64Array`, `GSFpsStateColumns` layout), `extra` 27
    /// values per row (the row's 12..38: height, rest ok, rest position, delta, buttons,
    /// previous buttons, move, yaw, the effect and tunable values). Answers the same 45 values
    /// per row ticked, and puts each ticked row's new columns at its index in `into`, written as
    /// the script's apply writes the state; `cols` stays as it was, so rows taken back run again
    /// from it. A row past the end of `cols`, `extra` or `into` ends the run.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn run_cols(
        &mut self,
        bodies: VarArray,
        tuns: VarArray,
        meta: PackedInt64Array,
        cols: VarArray,
        extra: PackedFloat64Array,
        into: VarArray,
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        self.cols_run(&bodies, &tuns, &meta, &cols, &extra, into, None, from, to);
        PackedFloat64Array::from(&self.out[..])
    }

    /// [`Self::run_cols`] with each row's `GSSimMotorState` in `states` (gs-player-controller 38):
    /// the row's rest ok and rest position come from its state's rest stamp, `extra[13]` holding
    /// only the script's part (`gs_sim_motor::state::ROW_SCRIPT_OK`, `ROW_REUSE`), and a ticked
    /// row stamps its state's rest where the end check settled (else drops it); a row taken back
    /// gets its stamp back. An answer whose counters are not all zero has flag 32. A row with bit 4
    /// in `extra[13]` (`ROW_CONSTS`, gs-player-controller 40) takes its values 23..38 from its
    /// state's kept ones (`GSSimMotorState.set_consts`). A row with bit 8 (`ROW_EVENTS`,
    /// gs-player-controller 42) gets flag 64 and a bit per change its tick made
    /// (`gs_sim_motor::state::events`: 128 mode, 256 height, 512 slide, 1024 wall run). A row
    /// with bit 16 (`ROW_CROUCH`, gs-player-controller 43) has its crouch run here before its tick
    /// (`gs_sim_motor::crouch::row`), its columns 30 and 31 the crouch fraction and held flag, its
    /// tun the crouch's six values at 75; events compare with the columns before the crouch. A row
    /// without a state, or a crouching one without those, ends the run.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn run_states(
        &mut self,
        bodies: VarArray,
        tuns: VarArray,
        meta: PackedInt64Array,
        cols: VarArray,
        extra: PackedFloat64Array,
        into: VarArray,
        states: VarArray,
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        self.cols_run(
            &bodies,
            &tuns,
            &meta,
            &cols,
            &extra,
            into,
            Some(&states),
            from,
            to,
        );
        PackedFloat64Array::from(&self.out[..])
    }

    /// Keeps a player's row here across ticks (gs-player-controller 49): its body, tunables
    /// (`run_rows`' tun) and motor state. Answers its slot; the row ticks once the script wrote its
    /// columns ([`Self::slot_push`]). A removed slot is given to a later player.
    #[func]
    fn slot_add(
        &mut self,
        body: Gd<GSSimFpsBody>,
        state: Gd<GSSimMotorState>,
        tun: PackedFloat64Array,
    ) -> i64 {
        self.slots.add(SlotData { body, tun, state }) as i64
    }

    /// Drops a player's row; false when there was none.
    #[func]
    fn slot_remove(&mut self, slot: i64) -> bool {
        usize::try_from(slot)
            .ok()
            .and_then(|s| self.slots.remove(s))
            .is_some()
    }

    /// Writes a player's columns (its whole `GSFpsState` columns, gs-player-controller 43's 32
    /// values): the script's write when something other than the motor changed the state (a
    /// teleport, a spawn, an outside velocity). False when there is no such row or `cols` is not
    /// 32 values long.
    #[func]
    fn slot_push(&mut self, slot: i64, cols: PackedFloat64Array) -> bool {
        cols.len() == crouch::COLS_LEN
            && usize::try_from(slot).is_ok_and(|s| self.slots.push(s, cols.as_slice()))
    }

    /// Sets a player's tunables (when its motor's changed); false when there is no such row.
    #[func]
    fn slot_tun(&mut self, slot: i64, tun: PackedFloat64Array) -> bool {
        let Some(row) = usize::try_from(slot)
            .ok()
            .and_then(|s| self.slots.get_mut(s))
        else {
            return false;
        };
        row.data.tun = tun;
        true
    }

    /// A player's columns as held here (empty before the first [`Self::slot_push`]).
    #[func]
    fn slot_cols(&self, slot: i64) -> PackedFloat64Array {
        usize::try_from(slot)
            .ok()
            .and_then(|s| self.slots.get(s))
            .map(|r| PackedFloat64Array::from(&r.cols[..]))
            .unwrap_or_default()
    }

    /// Players' rows held.
    #[func]
    fn slot_count(&self) -> i64 {
        self.slots.len() as i64
    }

    /// Values per ticked row in [`Self::run_slots`]'s answer.
    #[func]
    fn slot_stride(&self) -> i64 {
        SLOT_STRIDE as i64
    }

    /// [`Self::run_states`] over the resident rows: `slots` the slot per row, `meta` four values
    /// per row, `extra` 27 per row. The rows' columns, bodies, tunables and motor states are the
    /// slots'; each ticked row's new columns stay in its slot (taken back with the row by
    /// [`Self::rollback`]). Answers [`Self::slot_stride`] values per row ticked: the tick's 45
    /// values (flags as `run_states` gives them), then the row's 32 new columns, for the
    /// script's state. A row whose slot is gone or has no columns ends the run.
    #[func]
    fn run_slots(
        &mut self,
        slots: PackedInt64Array,
        meta: PackedInt64Array,
        extra: PackedFloat64Array,
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        self.slots_run(
            slots.as_slice(),
            meta.as_slice(),
            extra.as_slice(),
            from,
            to,
        )
    }

    /// Takes rows `from` and later back: the hull world holds again the feet it held before
    /// each of them ran. The caller ticks them again or by itself.

    #[func]
    fn rollback(&mut self, from: i64) {
        while let Some((row, _, _, _)) = self.moved.last() {
            if (*row as i64) < from {
                break;
            }
            if let Some((_, mut world, slot, old)) = self.moved.pop() {
                world.bind_mut().set_feet(slot, old);
            }
        }
        while let Some((row, _, _)) = self.rests.last() {
            if (*row as i64) < from {
                break;
            }
            if let Some((_, mut state, old)) = self.rests.pop() {
                state.bind_mut().rest = old;
            }
        }
        self.slots.rollback(usize::try_from(from).unwrap_or(0));
    }

    /// Whether [`Self::run_states`] runs a row's crouch (`ROW_CROUCH`, gs-player-controller 43).
    #[func]
    fn crouch_rows(&self) -> bool {
        true
    }

    /// Rows ticked and `run` calls since the last reset.
    #[func]
    fn counters(&self) -> PackedInt64Array {
        PackedInt64Array::from(&[self.rows, self.runs][..])
    }

    #[func]
    fn reset_counters(&mut self) {
        self.rows = 0;
        self.runs = 0;
    }
}

impl GSSimMotorBatch {
    /// Ticks `src`'s rows in order, `src[i]` row `first + i`: fills [`Self::out`].
    fn ticks(&mut self, src: &[Src], mp: &[i64], stride: usize, ip: &[f64], first: i64) {
        let hulls = VarArray::new();
        self.out.clear();
        self.runs += 1;
        let first = usize::try_from(first).unwrap_or(0);
        let to = (first + src.len())
            .min(mp.len() / stride)
            .min(ip.len() / TICK_IN_LEN);
        for (row, (body, tun)) in (first..to).zip(src) {
            let m = &mp[row * stride..(row + 1) * stride];
            let timers = if stride > META_LEN { m[META_LEN] } else { 0 };
            let sp = &ip[row * TICK_IN_LEN..(row + 1) * TICK_IN_LEN];
            let at = self.out.len();
            let mut body = body.clone();
            let mut b = body.bind_mut();
            if !b.tick_into(
                sp,
                tun.as_slice(),
                m[0],
                &hulls,
                m[1],
                m[2],
                timers,
                &mut self.out,
            ) {
                break;
            }
            debug_assert_eq!(self.out.len(), at + TICK_OUT_LEN);
            self.rows += 1;
            if m[0] != 3 {
                continue;
            }
            let Some((mut world, slot)) = b.hull_world() else {
                continue;
            };
            let mut w = world.bind_mut();
            if let Some(old) = w.feet(slot) {
                let o = &self.out[at..];
                w.set_feet(slot, V3::new(o[0] as f32, o[1] as f32, o[2] as f32));
                drop(w);
                self.moved.push((row, world, slot, old));
            }
        }
    }

    /// [`Self::run_cols`] and [`Self::run_states`] (`states` given): fills [`Self::out`].
    #[allow(clippy::too_many_arguments)]
    fn cols_run(
        &mut self,
        bodies: &VarArray,
        tuns: &VarArray,
        meta: &PackedInt64Array,
        cols: &VarArray,
        extra: &PackedFloat64Array,
        mut into: VarArray,
        states: Option<&VarArray>,
        from: i64,
        to: i64,
    ) {
        let ep = extra.as_slice();
        let first = usize::try_from(from).unwrap_or(0);
        let mut last = usize::try_from(to)
            .unwrap_or(0)
            .min(cols.len())
            .min(into.len())
            .min(ep.len() / cols::EXTRA_LEN);
        if let Some(st) = states {
            last = last.min(st.len());
        }
        let src = var_src(bodies, tuns, from, last as i64);
        // The rows' columns (and states), kept for the answer; a row without them ends the run.
        let mut held = Vec::with_capacity(src.len());
        let mut held_states = Vec::new();
        let mut held_crouch: Vec<Crouched> = Vec::new();
        let mp = meta.as_slice();
        let mut input = std::mem::take(&mut self.cols_in);
        input.clear();
        input.resize(last.max(first) * TICK_IN_LEN, 0.0);
        for (row, (body, tun)) in (first..last).zip(&src) {
            let Some(c) = cols
                .get(row)
                .and_then(|v| v.try_to::<PackedFloat64Array>().ok())
                .filter(|c| c.len() >= cols::STRIDE)
            else {
                break;
            };
            let e = &ep[row * cols::EXTRA_LEN..(row + 1) * cols::EXTRA_LEN];
            let r = &mut input[row * TICK_IN_LEN..(row + 1) * TICK_IN_LEN];
            cols::fill_row(c.as_slice(), e, r);
            if let Some(st) = states {
                let Some(state) = st
                    .get(row)
                    .and_then(|v| v.try_to::<Gd<GSSimMotorState>>().ok())
                else {
                    break;
                };
                let Some(m) = mp.get(row * ROWS_META_LEN..(row + 1) * ROWS_META_LEN) else {
                    break;
                };
                let Some(crouched) = state_row(&state, body, tun, m, c.as_slice(), e, r) else {
                    break;
                };
                held_states.push(state);
                held_crouch.push(crouched);
            }
            held.push(c);
        }
        self.ticks(&src[..held.len()], mp, ROWS_META_LEN, &input, from);
        self.cols_in = input;
        let mut new = Vec::new();
        for (i, c) in held.iter().enumerate().take(self.out.len() / TICK_OUT_LEN) {
            let row = first + i;
            let e = &ep[row * cols::EXTRA_LEN..(row + 1) * cols::EXTRA_LEN];
            let o = &mut self.out[i * TICK_OUT_LEN..(i + 1) * TICK_OUT_LEN];
            let crouched = held_crouch.get(i).and_then(Option::as_ref);
            answer_cols(c.as_slice(), crouched, e, o, &mut new);
            if let Some(state) = held_states.get_mut(i) {
                let old = state_answer(state, c.as_slice(), crouched, &new, e, o);
                self.rests.push((row, state.clone(), old));
            }
            into.set(row, &PackedFloat64Array::from(&new[..]).to_variant());
        }
    }

    /// [`Self::run_slots`]: ticks the slots' rows, keeps their new columns, answers
    /// [`SLOT_STRIDE`] values per row ticked.
    fn slots_run(
        &mut self,
        slots: &[i64],
        mp: &[i64],
        ep: &[f64],
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        let first = usize::try_from(from).unwrap_or(0);
        let last = usize::try_from(to)
            .unwrap_or(0)
            .min(slots.len())
            .min(ep.len() / cols::EXTRA_LEN)
            .min(mp.len() / ROWS_META_LEN);
        let mut src = Vec::with_capacity(last.saturating_sub(first));
        let mut held: Vec<(usize, Crouched)> = Vec::with_capacity(src.capacity());
        let mut input = std::mem::take(&mut self.cols_in);
        input.clear();
        input.resize(last.max(first) * TICK_IN_LEN, 0.0);
        for row in first..last {
            let Some(slot) = usize::try_from(slots[row]).ok() else {
                break;
            };
            let Some(sr) = self
                .slots
                .get(slot)
                .filter(|r| r.cols.len() == crouch::COLS_LEN)
            else {
                break;
            };
            let e = &ep[row * cols::EXTRA_LEN..(row + 1) * cols::EXTRA_LEN];
            let m = &mp[row * ROWS_META_LEN..(row + 1) * ROWS_META_LEN];
            let r = &mut input[row * TICK_IN_LEN..(row + 1) * TICK_IN_LEN];
            cols::fill_row(&sr.cols, e, r);
            let d = &sr.data;
            let Some(crouched) = state_row(&d.state, &d.body, &d.tun, m, &sr.cols, e, r) else {
                break;
            };
            src.push((d.body.clone(), d.tun.clone()));
            held.push((slot, crouched));
        }
        self.ticks(&src, mp, ROWS_META_LEN, &input, from);
        self.cols_in = input;
        let ticked = held.len().min(self.out.len() / TICK_OUT_LEN);
        let mut answer = Vec::with_capacity(ticked * SLOT_STRIDE);
        let mut new = Vec::new();
        for (i, (slot, crouched)) in held.iter().enumerate().take(ticked) {
            let row = first + i;
            let e = &ep[row * cols::EXTRA_LEN..(row + 1) * cols::EXTRA_LEN];
            let o = &mut self.out[i * TICK_OUT_LEN..(i + 1) * TICK_OUT_LEN];
            let Some(sr) = self.slots.get(*slot) else {
                break;
            };
            let mut state = sr.data.state.clone();
            answer_cols(&sr.cols, crouched.as_ref(), e, o, &mut new);
            let old = state_answer(&mut state, &sr.cols, crouched.as_ref(), &new, e, o);
            self.rests.push((row, state, old));
            self.slots.ticked(row, *slot, &new);
            answer.extend_from_slice(o);
            answer.extend_from_slice(&new);
        }
        PackedFloat64Array::from(&answer[..])
    }
}

/// Rows `from` to `to`'s bodies and tunables from the script's arrays, up to the first row
/// without them (the run ends there).
fn var_src(bodies: &VarArray, tuns: &VarArray, from: i64, to: i64) -> Vec<Src> {
    let from = usize::try_from(from).unwrap_or(0);
    let to = usize::try_from(to).unwrap_or(0);
    let mut src = Vec::with_capacity(to.saturating_sub(from));
    for row in from..to {
        let (Some(bv), Some(tv)) = (bodies.get(row), tuns.get(row)) else {
            break;
        };
        let (Ok(body), Ok(tun)) = (
            bv.try_to::<Gd<GSSimFpsBody>>(),
            tv.try_to::<PackedFloat64Array>(),
        ) else {
            break;
        };
        src.push((body, tun));
    }
    src
}

/// A state row's input past its columns: its kept values, its crouch when it asks for one
/// (`ROW_CROUCH`), then its rest stamp. `None` when a crouching row lacks the crouch columns or
/// tun (the run ends there and the script ticks it).
fn state_row(
    state: &Gd<GSSimMotorState>,
    body: &Gd<GSSimFpsBody>,
    tun: &PackedFloat64Array,
    m: &[i64],
    c: &[f64],
    e: &[f64],
    r: &mut [f64],
) -> Option<Crouched> {
    let st = state.bind();
    mstate::fill_consts(st.consts.as_ref(), e, r);
    let crouched = if e[1] as i64 & mstate::ROW_CROUCH != 0 {
        crouch_row(body, tun, m, c, e, r)?
    } else {
        None
    };
    match &crouched {
        Some((cc, height)) => st.rest.fill_row(cc, &with_height(e, *height), r),
        None => st.rest.fill_row(c, e, r),
    }
    Some(crouched)
}

/// `e` with the height its crouch left.
fn with_height(e: &[f64], height: f64) -> [f64; cols::EXTRA_LEN] {
    let mut ex = [0.0; cols::EXTRA_LEN];
    ex.copy_from_slice(e);
    ex[0] = height;
    ex
}

/// A row's new columns into `new`, from its columns (or the ones its crouch left) and answer.
fn answer_cols(
    c: &[f64],
    crouched: Option<&(Vec<f64>, f64)>,
    e: &[f64],
    o: &[f64],
    new: &mut Vec<f64>,
) {
    let ex;
    let (base, e) = match crouched {
        Some((cc, height)) => {
            ex = with_height(e, *height);
            (cc.as_slice(), &ex[..])
        }
        None => (c, e),
    };
    new.clear();
    new.resize(base.len(), 0.0);
    cols::apply_out(base, e, o, new);
}

/// A state row after its tick: its rest stamped where the end check settled, its answer's flags
/// (counted, events). Answers the stamp it held before, for [`GSSimMotorBatch::rollback`].
fn state_answer(
    state: &mut Gd<GSSimMotorState>,
    c: &[f64],
    crouched: Option<&(Vec<f64>, f64)>,
    new: &[f64],
    e: &[f64],
    o: &mut [f64],
) -> Rest {
    let ex;
    let e_used = match crouched {
        Some((_, height)) => {
            ex = with_height(e, *height);
            &ex[..]
        }
        None => e,
    };
    let old = state.bind().rest;
    state.bind_mut().rest.after(new, e_used, o);
    let mut flags = o[12] as i64;
    if mstate::counted(o) {
        flags |= mstate::OUT_COUNTED;
    }
    if e[1] as i64 & mstate::ROW_EVENTS != 0 {
        flags |= mstate::events(c, new);
    }
    o[12] = flags as f64;
    old
}

/// A `ROW_CROUCH` row's crouch ([`crouch::row`]) through its own body: `None` when the row lacks
/// the crouch columns or tun.
fn crouch_row(
    body: &Gd<GSSimFpsBody>,
    tun: &PackedFloat64Array,
    m: &[i64],
    c: &[f64],
    e: &[f64],
    r: &mut [f64],
) -> Option<Crouched> {
    if c.len() < crouch::COLS_LEN {
        return None;
    }
    let t = crouch::Tun::read(tun.as_slice())?;
    let mut body = body.clone();
    let mut b = body.bind_mut();
    Some(crouch::row(&t, c, e, r, |at, h, radius| {
        b.crouch_overlaps(at, h, radius, m[1], m[2])
    }))
}
