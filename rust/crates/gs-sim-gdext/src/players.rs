//! `GSSimPlayerPost`: the idle posture group of a player's `tick_post` (FC's `_idle_row`) over
//! `gs_sim_players`' packed columns: the posture row gs-weapon's spread reads and the settled,
//! flinch-free flag, the same to the bit as the script (FC's `headless_posture_native_parity`
//! runs both over seeded states). `GSSim.has(&"players")` switches it; the script stays the
//! fallback on the web and wherever the library is absent. `GSSimPlayerHead` is the head of
//! `tick_post_head` (`GSSim.has(&"player_head")`).

use godot::prelude::*;
use gs_sim_players::head::{HeadIn, Reach, head};
use gs_sim_players::{Config, OUT, POSTURE, PostIn, idle_row, run_rows};

/// A player's post-motor posture (stage 1 of the native player table).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimPlayerPost {
    cfg: Config,
}

#[godot_api]
impl GSSimPlayerPost {
    /// Sets `GSFpsState.Mode.GROUND` ([param ground_mode]) and `GSWeaponAccuracyClass.FLINCH_SEC`
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
        let rows = input.len() / gs_sim_players::IN;
        let mut out = vec![0.0; rows * OUT];
        run_rows(input.as_slice(), &self.cfg, &mut out);
        PackedFloat64Array::from(&out[..])
    }
}

/// The head of FC's `tick_post_head` (`gs_sim_players::head`): the hit-zone squeeze factor and
/// the map-volume skip in one call, the same to the bit as the script (FC's
/// `headless_head_native_parity`).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimPlayerHead {
    reach: Reach,
}

#[godot_api]
impl GSSimPlayerHead {
    /// `FcVolumePrefilter`'s `SLACK`, `BELOW` and `ABOVE`; until then 1, (3, 3, 3) and (3, 5, 3).
    #[func]
    fn configure(&mut self, slack: f64, below: Vector3, above: Vector3) {
        self.reach = Reach {
            slack,
            below: [below.x, below.y, below.z],
            above: [above.x, above.y, above.z],
        };
    }

    /// FC's `fit_hitboxes_to_crouch` factor, negated when `_volumes_skip` holds ([param marked]:
    /// the prefilter marked the player clear on this tick); NaN when the factor is not above
    /// zero (run the script).
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn head(
        &self,
        crouch: f64,
        crouch_height: f64,
        fit_stand_height: f64,
        radius: f64,
        ladder_reach: f64,
        stand_height: f64,
        waist: f64,
        start: Vector3,
        position: Vector3,
        marked: bool,
    ) -> f64 {
        let p = HeadIn {
            crouch,
            crouch_height,
            fit_stand_height,
            radius,
            ladder_reach,
            stand_height,
            waist,
            start: [start.x, start.y, start.z],
            position: [position.x, position.y, position.z],
            marked,
        };
        head(&p, &self.reach)
    }
}
