//! A bot world's contact list (gs-bot's `GSBotContact` fields and the sight rows a native world
//! reads) from one packed record per player. The same values as the script to the bit: positions
//! are the f64 state columns rounded to f32 (`Vector3(_f[0], _f[1], _f[2])`), flags are f64
//! comparisons, the health ratio an f64 division, and a lifted point is Godot's
//! `at + Vector3.UP * h` (the height rounded to f32, multiplied by each axis of UP, then added).
//!
//! A record is [`STRIDE`] floats: [`HEAD`] head values (flags, team, pitch, health, max health, a
//! spare), then the player's `GSFpsState` columns ([`COLUMNS`] floats). A player without a
//! controller has only its position in the first three columns.

/// Head values before the state columns.
pub const HEAD: usize = 6;
/// `GSFpsStateColumns.STRIDE`.
pub const COLUMNS: usize = 30;
/// One player's record.
pub const STRIDE: usize = HEAD + COLUMNS;

/// Head flag: the player has a controller (its state columns follow).
pub const CONTROLLER: u32 = 1;
/// Head flag: `p.is_alive()`.
pub const ALIVE: u32 = 2;
/// Head flag: the player has a health component (health and max health are set).
pub const HEALTH: u32 = 4;

const FLAGS: usize = 0;
const TEAM: usize = 1;
const PITCH: usize = 2;
const HEALTH_NOW: usize = 3;
const HEALTH_MAX: usize = 4;

const POSITION: usize = 0;
const VELOCITY: usize = 3;
const MODE: usize = 9;
const YAW: usize = 12;
const SLIDE_TIME: usize = 17;
const LAUNCH_COOLDOWN_LEFT: usize = 19;
const WALL_RUN_TIME: usize = 22;
/// `GSFpsState.Mode.GROUND`.
const GROUND: f64 = 0.0;

/// A contact's per-tick fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub view_yaw: f64,
    pub view_pitch: f64,
    pub grounded: bool,
    pub sliding: bool,
    pub wall_running: bool,
    pub air_jump_ready: bool,
    pub health: f64,
    pub alive: bool,
    pub team: i64,
}

fn v3(r: &[f64], at: usize) -> [f32; 3] {
    [r[at] as f32, r[at + 1] as f32, r[at + 2] as f32]
}

/// The contact for one [`STRIDE`] record; [param launch_enabled] is the body tunables'.
pub fn contact(r: &[f64], launch_enabled: bool) -> Contact {
    let flags = r[FLAGS] as u32;
    let s = &r[HEAD..HEAD + COLUMNS];
    let controlled = flags & CONTROLLER != 0;
    let health = if flags & HEALTH != 0 && r[HEALTH_MAX] > 0.0 {
        r[HEALTH_NOW] / r[HEALTH_MAX]
    } else {
        1.0
    };
    let mut c = Contact {
        position: v3(s, POSITION),
        velocity: [0.0; 3],
        view_yaw: f64::NAN,
        view_pitch: f64::NAN,
        grounded: true,
        sliding: false,
        wall_running: false,
        air_jump_ready: false,
        health,
        alive: flags & ALIVE != 0,
        team: r[TEAM] as i64,
    };
    if controlled {
        c.velocity = v3(s, VELOCITY);
        c.view_yaw = s[YAW];
        c.view_pitch = r[PITCH];
        // `int(_f[9]) == GROUND`: truncation; NaN and out-of-range values are never GROUND.
        c.grounded = s[MODE].trunc() == GROUND;
        c.sliding = s[SLIDE_TIME] >= 0.0;
        c.wall_running = s[WALL_RUN_TIME] >= 0.0;
        c.air_jump_ready = launch_enabled && s[LAUNCH_COOLDOWN_LEFT] <= 0.0;
    }
    c
}

/// Godot's `at + Vector3.UP * h`.
pub fn lift(at: [f32; 3], h: f64) -> [f32; 3] {
    let h = h as f32;
    [at[0] + 0.0 * h, at[1] + h, at[2] + 0.0 * h]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(flags: u32) -> Vec<f64> {
        let mut r = vec![0.0; STRIDE];
        r[FLAGS] = flags as f64;
        r[TEAM] = 2.0;
        r[PITCH] = -0.25;
        r[HEALTH_NOW] = 30.0;
        r[HEALTH_MAX] = 120.0;
        for (i, v) in r[HEAD..].iter_mut().enumerate() {
            *v = 0.1 * i as f64 + 0.123_456_789;
        }
        r
    }

    #[test]
    fn controlled_reads_the_columns() {
        let mut r = record(CONTROLLER | ALIVE | HEALTH);
        r[HEAD + MODE] = 0.0;
        r[HEAD + SLIDE_TIME] = -1.0;
        r[HEAD + WALL_RUN_TIME] = 0.0;
        r[HEAD + LAUNCH_COOLDOWN_LEFT] = 0.0;
        let c = contact(&r, true);
        assert_eq!(
            c.position,
            [r[HEAD] as f32, r[HEAD + 1] as f32, r[HEAD + 2] as f32]
        );
        assert_eq!(c.velocity[0], r[HEAD + 3] as f32);
        assert_eq!(c.view_yaw, r[HEAD + YAW]);
        assert_eq!(c.view_pitch, -0.25);
        assert!(c.grounded && !c.sliding && c.wall_running && c.air_jump_ready && c.alive);
        assert_eq!(c.health, 0.25);
        assert_eq!(c.team, 2);
        assert!(!contact(&r, false).air_jump_ready);
        r[HEAD + MODE] = 1.0;
        assert!(!contact(&r, true).grounded);
        r[HEAD + MODE] = f64::NAN;
        assert!(!contact(&r, true).grounded);
        r[HEAD + MODE] = 0.75;
        assert!(contact(&r, true).grounded);
    }

    #[test]
    fn uncontrolled_and_no_health() {
        let mut r = record(0);
        r[HEALTH_MAX] = 0.0;
        let c = contact(&r, true);
        assert_eq!(c.velocity, [0.0; 3]);
        assert!(c.view_yaw.is_nan() && c.view_pitch.is_nan());
        assert!(c.grounded && !c.sliding && !c.wall_running && !c.air_jump_ready && !c.alive);
        assert_eq!(c.health, 1.0);
        let r = record(HEALTH);
        assert_eq!(contact(&r, true).health, 0.25);
        let mut r = record(HEALTH);
        r[HEALTH_MAX] = 0.0;
        assert_eq!(contact(&r, true).health, 1.0);
    }

    #[test]
    fn lift_is_godots_up_times() {
        assert_eq!(lift([1.0, 2.0, 3.0], 1.5), [1.0, 3.5, 3.0]);
        // -0.0 + 0.0 * h is +0.0, as Godot's add.
        let l = lift([-0.0, 0.0, -0.0], 1.25);
        assert!(l[0].is_sign_positive() && l[2].is_sign_positive());
        // A negative height keeps -0.0: -0.0 + 0.0 * -1 is -0.0.
        let l = lift([-0.0, 0.0, -0.0], -1.0);
        assert!(l[0].is_sign_negative());
        assert_eq!(lift([0.0; 3], 1.0 / 3.0)[1], (1.0f64 / 3.0) as f32);
    }
}
