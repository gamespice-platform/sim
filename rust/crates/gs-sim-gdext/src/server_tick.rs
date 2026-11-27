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
use gs_sim_events::{EventTable, FLOATS, IN, INTS, kind};
use gs_sim_players::table::{BEGIN, FLAG_ALIVE, STRIDE, Table};
use gs_sim_players::{Config, OUT, run_rows};

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
    export: Vec<f64>,
    steps: i64,
    motor_rows: i64,
    feet_rows: i64,
    post_rows: i64,
    /// Seconds stepped since the last [`Self::reset_counters`].
    time: f64,
}

#[godot_api]
impl GSSimServerTick {
    /// The API level: callers check it and the methods they need, and say loudly what is missing.
    #[constant]
    const API: i64 = 2;
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
        self.events.len() as i64
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
    /// resident motor row, its origin from the hull world's feet. The rows refreshed.
    #[func]
    fn step(&mut self, dt: f64) -> i64 {
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

    /// The last [method derive]'s events as [method ints], widened ([constant EVENT_STRIDE] values each).
    #[func]
    fn events(&self) -> PackedInt64Array {
        let wide: Vec<i64> = self.events.ints.iter().map(|&v| i64::from(v)).collect();
        PackedInt64Array::from(&wide[..])
    }

    /// Steps, rows refreshed from motor rows, rows refreshed from feet, posture rows, event rows
    /// derived, since the last reset.
    #[func]
    fn counters(&self) -> PackedInt64Array {
        PackedInt64Array::from(
            &[
                self.steps,
                self.motor_rows,
                self.feet_rows,
                self.post_rows,
                self.event_rows,
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
        self.time = 0.0;
    }
}
