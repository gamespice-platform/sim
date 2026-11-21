//! `DotSimPlayerPost`: the idle posture group of a player's `tick_post` (FC's `_idle_row`) over
//! `dot_sim_players`' packed columns: the posture row dot-weapon's spread reads and the settled,
//! flinch-free flag, the same to the bit as the script (FC's `headless_posture_native_parity`
//! runs both over seeded states). `DotSim.has(&"players")` switches it; the script stays the
//! fallback on the web and wherever the library is absent.

use dot_sim_players::{Config, OUT, POSTURE, PostIn, idle_row, run_rows};
use godot::prelude::*;

/// A player's post-motor posture (stage 1 of the native player table).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimPlayerPost {
    cfg: Config,
}

#[godot_api]
impl DotSimPlayerPost {
    /// Sets `DotFpsState.Mode.GROUND` ([param ground_mode]) and `DotWeaponAccuracyClass.FLINCH_SEC`
    /// ([param flinch_sec]); until then 0 and 0.4.
    #[func]
    fn configure(&mut self, ground_mode: i64, flinch_sec: f64) {
        self.cfg = Config {
            ground_mode,
            flinch_sec,
        };
    }

    /// FC's `_idle_row` for one player: the posture row `[speed, airborne, crouched, aiming(0),
    /// sliding, sprinting, wall_running, tick, hurt_tick]` with the still flag (1 or 0) appended at
    /// index 9. [param last_hurt] is `last_damage_tick` (-1 with no health), [param rate] the
    /// arsenal's `tick_rate` (0 with no arsenal).
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn idle_row(
        &self,
        velocity: Vector3,
        mode: i64,
        crouch_fraction: f64,
        slide_time: f64,
        wall_run_time: f64,
        sprinting: bool,
        tick: i64,
        last_hurt: i64,
        rate: i64,
    ) -> PackedFloat64Array {
        let p = PostIn {
            vel_x: velocity.x,
            vel_z: velocity.z,
            mode,
            crouch_fraction,
            slide_time,
            wall_run_time,
            sprinting,
            tick,
            last_hurt,
            rate,
        };
        let mut out = [0.0; OUT];
        let still = idle_row(&p, &self.cfg, &mut out[..POSTURE]);
        out[POSTURE] = if still { 1.0 } else { 0.0 };
        PackedFloat64Array::from(&out[..])
    }

    /// [method idle_row] over packed input rows (10 floats each: vel x, vel z, mode, crouch
    /// fraction, slide time, wall run time, sprinting, tick, last hurt, rate), 10 floats out per row.
    #[func]
    fn run_rows(&self, input: PackedFloat64Array) -> PackedFloat64Array {
        let rows = input.len() / dot_sim_players::IN;
        let mut out = vec![0.0; rows * OUT];
        run_rows(input.as_slice(), &self.cfg, &mut out);
        PackedFloat64Array::from(&out[..])
    }
}
