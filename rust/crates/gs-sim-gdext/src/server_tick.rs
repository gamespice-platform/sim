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
//! loop in behind `step`; end-of-tick events come out of [`GSSimServerTick::events`] (filled once
//! native event rows land, empty until then).

use crate::batch::GSSimMotorBatch;
use crate::hulls::GSSimHullWorld;
use godot::prelude::*;
use gs_sim_players::table::{BEGIN, FLAG_ALIVE, STRIDE, Table};
use gs_sim_players::{Config, OUT, run_rows};

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimServerTick {
    cfg: Config,
    motor: Option<Gd<GSSimMotorBatch>>,
    world: Option<Gd<GSSimHullWorld>>,
    table: Table,
    /// Native event rows of this tick ([`Self::EVENT_STRIDE`] values each).
    events: Vec<i64>,
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
    const API: i64 = 1;
    /// Floats per row of [`Self::begin`]'s input: id, motor slot (-1: none), alive (0 or 1), health, weapon.
    #[constant]
    const BEGIN_STRIDE: i64 = BEGIN as i64;
    /// Floats per row of [`Self::table`]: id, origin xyz, velocity xyz, mode, ducking, flags
    /// (1 alive, 2 from the motor row, 4 origin from the hull world), health, weapon, motor slot.
    #[constant]
    const TABLE_STRIDE: i64 = STRIDE as i64;
    /// Values per row of [`Self::events`]: kind, player id, two arguments.
    #[constant]
    const EVENT_STRIDE: i64 = 4;

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
    /// as the hull world's). Drops last tick's events.
    #[func]
    fn begin(&mut self, rows: PackedFloat64Array) {
        self.table.begin(rows.as_slice());
        self.events.clear();
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

    /// This tick's native event rows ([constant EVENT_STRIDE] values each). Empty until native
    /// event rows fill it; callers read it after [method step] and handle what is there.
    #[func]
    fn events(&self) -> PackedInt64Array {
        PackedInt64Array::from(&self.events[..])
    }

    /// Steps, rows refreshed from motor rows, rows refreshed from feet, posture rows, since the
    /// last reset.
    #[func]
    fn counters(&self) -> PackedInt64Array {
        PackedInt64Array::from(&[self.steps, self.motor_rows, self.feet_rows, self.post_rows][..])
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
        self.time = 0.0;
    }
}
