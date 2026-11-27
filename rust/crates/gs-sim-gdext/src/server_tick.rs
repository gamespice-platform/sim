//! `GSSimServerTick`: the shell of a server's native tick (FC's native server tick, phase P1, see
//! FC `docs/native-server-tick.md`) and its player table, `gs_sim_players::table`.
//!
//! One object a game holds for its whole players' loop. It owns the loop's resident motor batch
//! ([`GSSimServerTick::motor`], the `GSSimMotorBatch` gs-player-controller's `GSFpsMotorBatch`
//! runs its rows on) and the idle posture lane ([`GSSimServerTick::run_rows`], the same kernel as
//! `GSSimPlayerPost.run_rows`), so both answer exactly as the standalone classes do. Once a tick:
//! [`GSSimServerTick::begin`] takes the script-owned columns in one packed array,
//! [`GSSimServerTick::step`] closes the loop and fills the table natively from the resident motor
//! rows and the hull world's feet, with no script call per player. Later phases move more of the
//! loop in behind `step`. The tick's motor edges are derived here too ([`GSSimServerTick::derive`],
//! the same table and layout as `GSSimTickEvents`, so gs-player-controller's `GSFpsTickEvents` takes
//! this object as its native table) and come out of [`GSSimServerTick::events`].

use crate::batch::GSSimMotorBatch;
use crate::hulls::GSSimHullWorld;
use godot::prelude::*;
use gs_sim_events::{EventTable, FLOATS, IN, IN_B_MODE, IN_B_Y, INTS, kind, slot};
use gs_sim_players::table::{BEGIN, FLAG_ALIVE, STRIDE, Table};
use gs_sim_players::{Config, OUT, post_head, pre, run_rows};

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimServerTick {
    cfg: Config,
    motor: Option<Gd<GSSimMotorBatch>>,
    world: Option<Gd<GSSimHullWorld>>,
    table: Table,
    /// This tick's derived motor edges ([`Self::EVENT_STRIDE`] ints and two floats each).
    events: EventTable,
    event_rows: i64,
    /// Rows [`Self::derive_slots`] filled from a motor row, and marker rows it found no motor row for.
    slot_rows: i64,
    slot_misses: i64,
    /// [`Self::derive_slots`]' row being filled, kept between calls.
    scratch: Vec<f64>,
    /// Whether [`Self::derive`] ran since the last [`Self::step`]: a tick that derived nothing has no events.
    derived: bool,
    export: Vec<f64>,
    steps: i64,
    motor_rows: i64,
    feet_rows: i64,
    post_rows: i64,
    /// [`Self::post_head`]'s answer, kept between calls.
    head_out: Vec<f64>,
    /// [`Self::post_head`] rows answered from a motor slot, and rows whose slot did not run.
    head_rows: i64,
    head_misses: i64,
    /// [`Self::pre`]'s answer, kept between calls.
    pre_out: Vec<f64>,
    /// [`Self::pre`] rows answered, and of those the rows that sat the tick out.
    pre_rows: i64,
    pre_skips: i64,
    /// Seconds stepped since the last [`Self::reset_counters`].
    time: f64,
}

#[godot_api]
impl GSSimServerTick {
    /// The API level: callers check it and the methods they need, and say loudly what is missing.
    /// 3: a tick that derives nothing ends with no events (2 kept the last derived tick's).
    /// 4: [`Self::derive_slots`], event rows filled from the resident motor rows.
    /// 5: [`Self::post_head`], the players' post-head pass from the resident motor rows.
    #[constant]
    /// 6: [`Self::pre`], the players' pre-motor pass (body gate and ladder wish).
    const API: i64 = 6;
    /// Floats per row of [`Self::pre`]'s input (`gs_sim_players::pre::IN`).
    #[constant]
    const PRE_IN: i64 = pre::IN as i64;
    /// Floats per row of [`Self::pre`]'s answer: kind (0 reset, 1 run, 2 skip), delta, wish x, wish z.
    #[constant]
    const PRE_OUT: i64 = pre::OUT as i64;
    /// Floats per row of [`Self::post_head`]'s input: motor slot, gate, crouch height ratio.
    #[constant]
    const POST_HEAD_IN: i64 = post_head::IN as i64;
    /// Floats per row of [`Self::post_head`]'s answer: valid, skip, height scale, end feet xyz.
    #[constant]
    const POST_HEAD_OUT: i64 = post_head::OUT as i64;
    /// Floats per row of [`Self::begin`]'s input: id, motor slot (-1: none), alive (0 or 1), health, weapon.
    #[constant]
    const BEGIN_STRIDE: i64 = BEGIN as i64;
    /// Floats per row of [`Self::table`]: id, origin xyz, velocity xyz, mode, ducking, flags
    /// (1 alive, 2 from the motor row, 4 origin from the hull world), health, weapon, motor slot.
    #[constant]
    const TABLE_STRIDE: i64 = STRIDE as i64;
    /// Values per row of [`Self::events`]: entity, kind, two arguments (`GSSimTickEvents.ints`).
    #[constant]
    const EVENT_STRIDE: i64 = INTS as i64;

    /// Sets `GSFpsState.Mode.GROUND` and `GSWeaponAccuracyClass.FLINCH_SEC` for the posture lane
    /// (as `GSSimPlayerPost.configure`).
    #[func]
    fn configure(&mut self, ground_mode: i64, flinch_sec: f64) {
        self.cfg = Config {
            ground_mode,
            flinch_sec,
        };
    }

    /// The players' loop's resident motor batch, made on first call: hand it to
    /// `GSFpsMotorBatch.use_native` so the loop's motor rows live where [`Self::step`] reads them.
    #[func]
    fn motor(&mut self) -> Gd<GSSimMotorBatch> {
        self.motor
            .get_or_insert_with(GSSimMotorBatch::new_gd)
            .clone()
    }

    /// The hull world whose feet [`Self::step`] takes as the table's origins (null: none).
    #[func]
    fn bind_world(&mut self, world: Option<Gd<GSSimHullWorld>>) {
        self.world = world;
    }

    /// Starts a tick: the roster in order, [constant BEGIN_STRIDE] floats per slot (the same slot
    /// as the hull world's). This tick's events stay until the next [method derive].
    #[func]
    fn begin(&mut self, rows: PackedFloat64Array) {
        self.table.begin(rows.as_slice());
    }

    /// `[IN, INTS, FLOATS, kinds]`, as `GSSimTickEvents.layout`: `GSFpsTickEvents` checks it before
    /// taking this object as its native table.
    #[func]
    fn layout(&self) -> PackedInt32Array {
        PackedInt32Array::from(&[IN as i32, INTS as i32, FLOATS as i32, kind::COUNT][..])
    }

    /// Replaces this tick's events with those of the first [param count] rows of [param rows]
    /// (`GSFpsTickEvents`' 18 `IN_*` floats each, in roster order); answers how many there are.
    /// The same derive as `GSSimTickEvents.derive`.
    #[func]
    fn derive(&mut self, rows: PackedFloat64Array, count: i64) -> i64 {
        self.events.clear();
        let n = (usize::try_from(count).unwrap_or(0) * IN).min(rows.len());
        self.events.derive_rows(&rows.as_slice()[..n]);
        self.event_rows += (n / IN) as i64;
        self.derived = true;
        self.events.len() as i64
    }

    /// [`Self::derive`] where a row may be a marker (gs-player-controller 55's
    /// `GSFpsTickEvents.record_slot`): `IN_B_MODE` below zero, its motor slot in `IN_B_Y`, its
    /// entity, flags and step height as any row. A marker's before and after values come from that
    /// slot's run in this loop of [`Self::motor`] (`gs_sim_events::slot`), the same values the
    /// controller would have recorded; a marker whose slot did not run gives no events and counts
    /// a miss ([`Self::counters`]' sixth value).
    #[func]
    fn derive_slots(&mut self, rows: PackedFloat64Array, count: i64) -> i64 {
        self.events.clear();
        let n = (usize::try_from(count).unwrap_or(0) * IN).min(rows.len());
        let motor = self.motor.as_ref().map(|m| m.bind());
        let mut row = std::mem::take(&mut self.scratch);
        for r in rows.as_slice()[..n].as_chunks::<IN>().0 {
            if r[IN_B_MODE] >= 0.0 {
                self.events.derive_row(r);
                continue;
            }
            let ran = motor
                .as_ref()
                .and_then(|m| m.slot_tick(r[IN_B_Y] as i64))
                .and_then(|(b, new, out)| Some((slot::before(b)?, slot::after(new, out)?)));
            let Some((b, a)) = ran else {
                self.slot_misses += 1;
                continue;
            };
            row.clear();
            row.extend_from_slice(r);
            slot::fill(&mut row, &b, &a);
            self.events.derive_row(&row);
            self.slot_rows += 1;
        }
        self.scratch = row;
        self.event_rows += (n / IN) as i64;
        self.derived = true;
        self.events.len() as i64
    }

    /// The players' post-head pass (`gs_sim_players::post_head`) over [param count] rows of
    /// [`Self::POST_HEAD_IN`] floats: each row's skip, hitbox height scale and end feet from its
    /// motor slot's run in this loop of [`Self::motor`]. A slot that did not run answers valid 0
    /// (counted in [`Self::counters`]' ninth value). [param slack] is the volume prefilter's.
    #[func]
    fn post_head(
        &mut self,
        rows: PackedFloat64Array,
        count: i64,
        slack: f64,
    ) -> PackedFloat64Array {
        let n = usize::try_from(count)
            .unwrap_or(0)
            .min(rows.len() / post_head::IN);
        self.head_out.resize(n * post_head::OUT, 0.0);
        let motor = self.motor.as_ref().map(|m| m.bind());
        let ins = rows.as_slice()[..n * post_head::IN]
            .as_chunks::<{ post_head::IN }>()
            .0;
        let outs = self.head_out.as_chunks_mut::<{ post_head::OUT }>().0;
        for (r, o) in ins.iter().zip(outs.iter_mut()) {
            let ran = motor
                .as_ref()
                .and_then(|m| m.slot_tick(r[0] as i64))
                .map(|(b, new, _)| (b, new));
            post_head::row(r, slack, ran, o);
            if o[0] != 0.0 {
                self.head_rows += 1;
            } else {
                self.head_misses += 1;
            }
        }
        PackedFloat64Array::from(&self.head_out[..])
    }

    /// The players' pre-motor pass for `count` packed rows ([constant PRE_IN] floats each, see
    /// `gs_sim_players::pre`): FC's body gate and ladder wish, [constant PRE_OUT] floats per row.
    /// `max_gap` is the longest gap made up; the ground mode is [method configure]'s.
    #[func]
    fn pre(&mut self, rows: PackedFloat64Array, count: i64, max_gap: i64) -> PackedFloat64Array {
        let n = usize::try_from(count)
            .unwrap_or(0)
            .min(rows.len() / pre::IN);
        self.pre_out.resize(n * pre::OUT, 0.0);
        let ground = self.cfg.ground_mode;
        let ins = rows.as_slice()[..n * pre::IN].as_chunks::<{ pre::IN }>().0;
        let outs = self.pre_out.as_chunks_mut::<{ pre::OUT }>().0;
        for (r, o) in ins.iter().zip(outs.iter_mut()) {
            pre::row(r, max_gap, ground, o);
            if o[0] == pre::SKIP {
                self.pre_skips += 1;
            }
        }
        self.pre_rows += n as i64;
        PackedFloat64Array::from(&self.pre_out[..])
    }

    /// This tick's events, `[entity, kind, i0, i1]` each (`GSSimTickEvents.ints`).
    #[func]
    fn ints(&self) -> PackedInt32Array {
        PackedInt32Array::from(&self.events.ints[..])
    }

    /// This tick's events' `[f0, f1]` each (`GSSimTickEvents.floats`).
    #[func]
    fn floats(&self) -> PackedFloat64Array {
        PackedFloat64Array::from(&self.events.floats[..])
    }

    /// The idle players' posture rows, 10 floats in and out per row, the same to the bit as
    /// `GSSimPlayerPost.run_rows` configured alike (same name and shape, so a game hands this
    /// object where it held a `GSSimPlayerPost`).
    #[func]
    fn run_rows(&mut self, input: PackedFloat64Array) -> PackedFloat64Array {
        let rows = input.len() / gs_sim_players::IN;
        let mut out = vec![0.0; rows * OUT];
        run_rows(input.as_slice(), &self.cfg, &mut out);
        self.post_rows += rows as i64;
        PackedFloat64Array::from(&out[..])
    }

    /// Ends the players' loop of a tick [param dt] long: each alive row's motor part from its
    /// resident motor row, its origin from the hull world's feet. The rows refreshed. A tick with
    /// no [method derive] since the last step (no motor rows: everyone dead, or none ticked) ends
    /// with no events, never the last derived tick's.
    #[func]
    fn step(&mut self, dt: f64) -> i64 {
        if !std::mem::take(&mut self.derived) {
            self.events.clear();
        }
        let mut refreshed = 0;
        let motor = self.motor.as_ref().map(|m| m.bind());
        let world = self.world.as_ref().map(|w| w.bind());
        for i in 0..self.table.len() {
            if self.table.flags[i] & FLAG_ALIVE == 0 {
                continue;
            }
            let mut hit = false;
            if let Some(cols) = motor
                .as_ref()
                .and_then(|m| m.resident_cols(self.table.slot[i]))
                && self.table.take_motor(i, cols)
            {
                self.motor_rows += 1;
                hit = true;
            }
            if let Some(feet) = world.as_ref().and_then(|w| w.feet_at(i)) {
                self.table.take_feet(i, feet);
                self.feet_rows += 1;
                hit = true;
            }
            refreshed += i64::from(hit);
        }
        self.steps += 1;
        self.time += dt;
        refreshed
    }

    /// The player table, [constant TABLE_STRIDE] floats per roster slot.
    #[func]
    fn table(&mut self) -> PackedFloat64Array {
        self.table.export(&mut self.export);
        PackedFloat64Array::from(&self.export[..])
    }

    /// The table's digest (FNV-1a over every value's bits): equal only when equal to the bit.
    #[func]
    fn digest(&self) -> i64 {
        self.table.digest() as i64
    }

    /// This tick's events as [method ints] (none when the tick derived none), widened ([constant EVENT_STRIDE] values each).
    #[func]
    fn events(&self) -> PackedInt64Array {
        let wide: Vec<i64> = self.events.ints.iter().map(|&v| i64::from(v)).collect();
        PackedInt64Array::from(&wide[..])
    }

    /// Steps, rows refreshed from motor rows, rows refreshed from feet, posture rows, event rows
    /// derived, rows filled from motor rows, marker rows missed ([`Self::derive_slots`]), since
    /// the last reset, then post-head rows answered and missed ([`Self::post_head`]), then pre rows
    /// answered and skipped ([`Self::pre`]).
    #[func]
    fn counters(&self) -> PackedInt64Array {
        PackedInt64Array::from(
            &[
                self.steps,
                self.motor_rows,
                self.feet_rows,
                self.post_rows,
                self.event_rows,
                self.slot_rows,
                self.slot_misses,
                self.head_rows,
                self.head_misses,
                self.pre_rows,
                self.pre_skips,
            ][..],
        )
    }

    /// Seconds stepped since the last reset.
    #[func]
    fn stepped_time(&self) -> f64 {
        self.time
    }

    #[func]
    fn reset_counters(&mut self) {
        self.steps = 0;
        self.motor_rows = 0;
        self.feet_rows = 0;
        self.post_rows = 0;
        self.event_rows = 0;
        self.slot_rows = 0;
        self.slot_misses = 0;
        self.head_rows = 0;
        self.head_misses = 0;
        self.pre_rows = 0;
        self.pre_skips = 0;
        self.time = 0.0;
    }
}
