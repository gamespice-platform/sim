//! gs-player-controller's `GSFpsNetSync` packers (`pack_flags`, `pack_timers`, `pack_timers2`,
//! `pack_stamina`) over a state's columns ([`crate::cols`]), so a server's per-player pull is one
//! call instead of a property read per field. Same integers as the script, bit for bit.

/// `GSFpsCommand.BUTTON_BITS`.
pub const BUTTON_BITS: u32 = 8;

const MODE_MASK: i64 = 15;
const FLAG_CROUCH_HELD: i64 = 1 << 4;
const LAUNCH_BITS: u32 = 12;
const SLIDE_BITS: u32 = 7;
const SLIDE_COOLDOWN_BITS: u32 = 7;
const NOT_SLIDING: i64 = (1 << SLIDE_BITS) - 1;
const LAUNCH_SHIFT: u32 = 5;
const SLIDE_SHIFT: u32 = LAUNCH_SHIFT + LAUNCH_BITS;
const SLIDE_COOLDOWN_SHIFT: u32 = SLIDE_SHIFT + SLIDE_BITS;
const BUTTONS_SHIFT: u32 = SLIDE_COOLDOWN_SHIFT + SLIDE_COOLDOWN_BITS;
const DASH_BITS: u32 = 12;
const DASH_SHIFT: u32 = BUTTONS_SHIFT + BUTTON_BITS;
const WALL_RUN_BITS: u32 = 7;
const WALL_RUN_SPENT: i64 = 1;
const WALL_RUN_FIRST_TIME: i64 = 2;
const WALL_JUMP_COUNT_BITS: u32 = 3;
const WALL_RUN_SHIFT: u32 = DASH_SHIFT + DASH_BITS;
const WALL_JUMP_COUNT_SHIFT: u32 = WALL_RUN_SHIFT + WALL_RUN_BITS;
const WALL_HEADING_SHIFT: u32 = WALL_JUMP_COUNT_SHIFT + WALL_JUMP_COUNT_BITS;
const JUMP_TIMER_SCALE: f64 = 7680.0;
const JUMP_TIMER_BITS: u32 = 14;
const T2_MAX: i64 = (1 << JUMP_TIMER_BITS) - 1;

// Column offsets (`GSFpsStateColumns`).
const MODE: usize = 9;
const PREVIOUS_BUTTONS: usize = 13;
const TIME_SINCE_JUMP: usize = 14;
const TIME_SINCE_GROUNDED: usize = 15;
const TIME_SINCE_JUMP_PRESSED: usize = 16;
const SLIDE_TIME: usize = 17;
const SLIDE_COOLDOWN_LEFT: usize = 18;
const LAUNCH_COOLDOWN_LEFT: usize = 19;
const DASH_COOLDOWN_LEFT: usize = 20;
const WALL_RUN_COOLDOWN_LEFT: usize = 21;
const WALL_RUN_TIME: usize = 22;
const WALL_RUN_SPENT_COL: usize = 23;
const WALL_JUMP_COUNT: usize = 24;
const WALL_JUMPS_IN_AIR: usize = 25;
const WALL_JUMP_HEADING: usize = 26;

/// The state values a pull packs that are not in the columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extra {
    pub crouch_held: bool,
    /// `GSFpsNetSync.timer_scale`.
    pub timer_scale: f64,
    pub stamina: f64,
    pub stamina_exhausted: bool,
    pub stamina_delay_left: f64,
}

/// The script's `roundi`: half away from zero, then to int.
fn roundi(x: f64) -> i64 {
    x.round() as i64
}

/// The script's `int(x)`: truncation toward zero.
fn gd_int(x: f64) -> i64 {
    x as i64
}

fn quantise(seconds: f64, bits: u32, timer_scale: f64) -> i64 {
    roundi(gs_sim_math::maxf(seconds, 0.0) * timer_scale).clamp(0, (1 << bits) - 1)
}

/// `GSFpsNetSync.wall_jump_code`.
pub fn wall_jump_code(in_air: i64, same_wall: i64) -> i64 {
    let total = in_air.clamp(0, 3);
    if total == 0 {
        return 0;
    }
    ((total * (total - 1)) >> 1) + same_wall.clamp(1, total)
}

/// `GSFpsNetSync.pack_flags`.
pub fn pack_flags(cols: &[f64], extra: &Extra) -> i64 {
    let ts = extra.timer_scale;
    let mut flags = gd_int(cols[MODE]) & MODE_MASK;
    if extra.crouch_held {
        flags |= FLAG_CROUCH_HELD;
    }
    flags |= quantise(cols[LAUNCH_COOLDOWN_LEFT], LAUNCH_BITS, ts) << LAUNCH_SHIFT;
    let slide_time = cols[SLIDE_TIME];
    let slide = if slide_time >= 0.0 {
        quantise(slide_time, SLIDE_BITS, ts).min(NOT_SLIDING - 1)
    } else {
        NOT_SLIDING
    };
    flags |= slide << SLIDE_SHIFT;
    flags |= quantise(cols[SLIDE_COOLDOWN_LEFT], SLIDE_COOLDOWN_BITS, ts) << SLIDE_COOLDOWN_SHIFT;
    flags |= (gd_int(cols[PREVIOUS_BUTTONS]) & ((1 << BUTTON_BITS) - 1)) << BUTTONS_SHIFT;
    flags |= quantise(cols[DASH_COOLDOWN_LEFT], DASH_BITS, ts) << DASH_SHIFT;
    let run_time = cols[WALL_RUN_TIME];
    let wall_run = if run_time >= 0.0 {
        WALL_RUN_FIRST_TIME
            + quantise(run_time, WALL_RUN_BITS, ts)
                .min((1 << WALL_RUN_BITS) - 1 - WALL_RUN_FIRST_TIME)
    } else if cols[WALL_RUN_SPENT_COL] != 0.0 {
        WALL_RUN_SPENT
    } else {
        0
    };
    flags |= wall_run << WALL_RUN_SHIFT;
    let jumps = wall_jump_code(
        gd_int(cols[WALL_JUMPS_IN_AIR]),
        gd_int(cols[WALL_JUMP_COUNT]),
    );
    flags |= jumps << WALL_JUMP_COUNT_SHIFT;
    flags | (gd_int(cols[WALL_JUMP_HEADING]) & 7).wrapping_shl(WALL_HEADING_SHIFT)
}

fn timer_code(seconds: f64) -> i64 {
    let n = roundi(gs_sim_math::maxf(seconds, 0.0) * JUMP_TIMER_SCALE);
    if n >= (1 << JUMP_TIMER_BITS) - 2 {
        0
    } else {
        n + 1
    }
}

/// `GSFpsNetSync.pack_timers`.
pub fn pack_timers(cols: &[f64]) -> i64 {
    timer_code(cols[TIME_SINCE_GROUNDED])
        | timer_code(cols[TIME_SINCE_JUMP]) << JUMP_TIMER_BITS
        | timer_code(cols[TIME_SINCE_JUMP_PRESSED]) << (2 * JUMP_TIMER_BITS)
        | timer_code(cols[WALL_RUN_COOLDOWN_LEFT]) << (3 * JUMP_TIMER_BITS)
}

/// `GSFpsNetSync.pack_timers2`.
pub fn pack_timers2(cols: &[f64]) -> i64 {
    let slide_time = cols[SLIDE_TIME];
    let slide = if slide_time >= 0.0 {
        (roundi(slide_time * JUMP_TIMER_SCALE) + 1).min(T2_MAX)
    } else {
        0
    };
    let cooldown =
        roundi(gs_sim_math::maxf(cols[SLIDE_COOLDOWN_LEFT], 0.0) * JUMP_TIMER_SCALE).min(T2_MAX);
    let run_time = cols[WALL_RUN_TIME];
    let run = if run_time >= 0.0 {
        (roundi(run_time * JUMP_TIMER_SCALE) + WALL_RUN_FIRST_TIME).min(T2_MAX)
    } else if cols[WALL_RUN_SPENT_COL] != 0.0 {
        WALL_RUN_SPENT
    } else {
        0
    };
    slide | cooldown << JUMP_TIMER_BITS | run << (2 * JUMP_TIMER_BITS)
}

/// `GSFpsNetSync.pack_stamina`.
pub fn pack_stamina(extra: &Extra) -> i64 {
    roundi(gs_sim_math::clampf(extra.stamina, 0.0, 1.0) * 16383.0)
        | i64::from(extra.stamina_exhausted) << 14
        | roundi(extra.stamina_delay_left * 2048.0).clamp(0, 8191) << 15
}

/// All four: `[flags, timers, timers2, stamina]`. `cols` is at least `GSFpsStateColumns.STRIDE` long.
pub fn pack(cols: &[f64], extra: &Extra) -> [i64; 4] {
    [
        pack_flags(cols, extra),
        pack_timers(cols),
        pack_timers2(cols),
        pack_stamina(extra),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: [f64; 30] = [
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1000.0, 0.0, 1000.0,
        -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ];

    fn extra() -> Extra {
        Extra {
            crouch_held: false,
            timer_scale: 64.0,
            stamina: 1.0,
            stamina_exhausted: false,
            stamina_delay_left: 0.0,
        }
    }

    #[test]
    fn fresh_state() {
        // Airborne, not sliding (127), wall run ready; jump timers past the field (0), grounded and wall-run cooldown at zero (1).
        assert_eq!(pack_flags(&DEFAULTS, &extra()), 1 | 127 << SLIDE_SHIFT);
        assert_eq!(pack_timers(&DEFAULTS), 1 | 1 << (3 * JUMP_TIMER_BITS));
        assert_eq!(pack_timers2(&DEFAULTS), 0);
        assert_eq!(pack_stamina(&extra()), 16383);
    }

    #[test]
    fn wall_fields_and_top_bit() {
        let mut cols = DEFAULTS;
        cols[WALL_RUN_TIME] = 0.5;
        cols[WALL_JUMPS_IN_AIR] = 3.0;
        cols[WALL_JUMP_COUNT] = 2.0;
        cols[WALL_JUMP_HEADING] = 7.0;
        let flags = pack_flags(&cols, &extra());
        assert_eq!((flags >> WALL_RUN_SHIFT) & 127, 2 + 32);
        assert_eq!((flags >> WALL_JUMP_COUNT_SHIFT) & 7, 5);
        assert!(flags < 0, "the heading's top bit is the sign bit");
        assert_eq!(wall_jump_code(1, 0), 1);
        assert_eq!(wall_jump_code(0, 5), 0);
    }

    #[test]
    fn clamps_and_rounding() {
        let mut cols = DEFAULTS;
        cols[SLIDE_TIME] = 10.0;
        cols[DASH_COOLDOWN_LEFT] = -3.0;
        cols[WALL_RUN_SPENT_COL] = 1.0;
        let flags = pack_flags(&cols, &extra());
        assert_eq!((flags >> SLIDE_SHIFT) & 127, 126);
        assert_eq!((flags >> DASH_SHIFT) & 4095, 0);
        assert_eq!((flags >> WALL_RUN_SHIFT) & 127, WALL_RUN_SPENT);
        assert_eq!(pack_timers2(&cols) & T2_MAX, T2_MAX);
        let e = Extra {
            stamina: 0.5,
            stamina_exhausted: true,
            stamina_delay_left: 9.0,
            ..extra()
        };
        assert_eq!(pack_stamina(&e), 8192 | 1 << 14 | 8191 << 15);
    }
}
