//! gs-player-controller's `GSFpsNetSync` packers (`pack_flags`, `pack_timers`, `pack_timers2`,
//! `pack_stamina`) over a state's columns ([`crate::cols`]), so a server's per-player pull is one
//! call instead of a property read per field. Same integers as the script, bit for bit. [`pull`] adds the
//! sub-quantum hold a carrier applies after it, so the whole movement pull is one call.

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
const YAW: usize = 12;
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

/// [`hold_mask`] bit: keep the velocity from before the pull.
pub const HOLD_VELOCITY: i64 = 1;
/// [`hold_mask`] bit: keep the yaw from before the pull.
pub const HOLD_YAW: i64 = 2;
/// [`hold_mask`] bit: keep the pitch from before the pull.
pub const HOLD_PITCH: i64 = 4;

/// How a carrier writes its velocity (gs-net `VECTOR3_VELOCITY`, `-speed..speed` in `velocity_bits`) and its
/// yaw and pitch (`ANGLE`, `*_bits` wide). A width outside 1..=62 leaves that property out of the hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wire {
    pub speed: f64,
    pub velocity_bits: i64,
    pub yaw_bits: i64,
    pub pitch_bits: i64,
}

/// The replicated values before a pull: the velocity as the carrier stores it (`Vector3`, so `f32`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub velocity: [f32; 3],
    pub yaw: f64,
    pub pitch: f64,
}

fn width_ok(bits: i64) -> bool {
    (1..=62).contains(&bits)
}

/// Godot's `clampf`: a NaN passes through.
fn clampf(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Godot's `fposmod`.
fn fposmod(x: f64, y: f64) -> f64 {
    let mut value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value += y;
    }
    value + 0.0
}

/// gs-net `GSNetVar._snap_range`: the value a range-quantised float reads back as.
pub fn snap_range(value: f64, min: f64, max: f64, bits: i64) -> f64 {
    let mask = (1i64 << bits) - 1;
    let normalised = clampf((value - min) / (max - min), 0.0, 1.0);
    let quantised = (normalised * mask as f64).round() as i64 & mask;
    min + quantised as f64 / mask as f64 * (max - min)
}

/// The code gs-net's `write_angle` puts on the wire for `degrees` (Fatal Contact's `FcPlayerNet._angle_code`).
pub fn angle_code(degrees: f64, bits: i64) -> i64 {
    let mask = (1i64 << bits) - 1;
    (fposmod(degrees, 360.0) / 360.0 * mask as f64).round() as i64 & mask
}

fn snap_velocity(v: [f32; 3], wire: &Wire) -> [f32; 3] {
    v.map(|c| snap_range(f64::from(c), -wire.speed, wire.speed, wire.velocity_bits) as f32)
}

/// Which of velocity, yaw and pitch a pull should leave at [`Held`] (`HOLD_*` bits): those whose new value
/// (`cols` velocity and yaw, `pitch`) differs but writes the same wire codes, so the carrier reports no change no
/// receiver could see. Fatal Contact's `FcPlayerNet._hold_subquantum`, to the bit.
pub fn hold_mask(cols: &[f64], pitch: f64, held: &Held, wire: &Wire) -> i64 {
    let mut mask = 0;
    if width_ok(wire.velocity_bits) && wire.speed > 0.0 {
        let velocity = [cols[3] as f32, cols[4] as f32, cols[5] as f32];
        if velocity != held.velocity
            && snap_velocity(velocity, wire) == snap_velocity(held.velocity, wire)
        {
            mask |= HOLD_VELOCITY;
        }
    }
    let yaw = cols[YAW];
    if width_ok(wire.yaw_bits)
        && yaw != held.yaw
        && angle_code(yaw, wire.yaw_bits) == angle_code(held.yaw, wire.yaw_bits)
    {
        mask |= HOLD_YAW;
    }
    if width_ok(wire.pitch_bits)
        && pitch != held.pitch
        && angle_code(pitch, wire.pitch_bits) == angle_code(held.pitch, wire.pitch_bits)
    {
        mask |= HOLD_PITCH;
    }
    mask
}

/// [`pack`] and then [`hold_mask`]: `[flags, timers, timers2, stamina, hold]`, a server's whole movement pull.
pub fn pull(cols: &[f64], extra: &Extra, pitch: f64, held: &Held, wire: &Wire) -> [i64; 5] {
    let [flags, timers, timers2, stamina] = pack(cols, extra);
    [
        flags,
        timers,
        timers2,
        stamina,
        hold_mask(cols, pitch, held, wire),
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

    const WIRE: Wire = Wire {
        speed: 100.0,
        velocity_bits: 12,
        yaw_bits: 16,
        pitch_bits: 16,
    };

    fn held(cols: &[f64], pitch: f64) -> Held {
        Held {
            velocity: [cols[3] as f32, cols[4] as f32, cols[5] as f32],
            yaw: cols[YAW],
            pitch,
        }
    }

    #[test]
    fn hold_keeps_subquantum_changes_only() {
        let mut cols = DEFAULTS;
        cols[3] = 3.0;
        cols[YAW] = 90.0;
        let before = held(&cols, 10.0);
        // Nothing changed: nothing to hold.
        assert_eq!(hold_mask(&cols, 10.0, &before, &WIRE), 0);
        // A step far below a code (200 / 4095 for velocity, 360 / 65535 for angles) is held.
        cols[3] = 3.0001;
        cols[YAW] = 90.0001;
        assert_eq!(
            hold_mask(&cols, 10.0001, &before, &WIRE),
            HOLD_VELOCITY | HOLD_YAW | HOLD_PITCH
        );
        // A step of whole codes is not.
        cols[3] = 4.0;
        cols[YAW] = 91.0;
        assert_eq!(hold_mask(&cols, 11.0, &before, &WIRE), 0);
        // A width out of range drops the property.
        let off = Wire {
            velocity_bits: 0,
            yaw_bits: 63,
            ..WIRE
        };
        cols[3] = 3.0001;
        cols[YAW] = 90.0001;
        assert_eq!(hold_mask(&cols, 10.0001, &before, &off), HOLD_PITCH);
    }

    #[test]
    fn angle_code_wraps_like_fposmod() {
        assert_eq!(angle_code(-90.0, 8), angle_code(270.0, 8));
        assert_eq!(angle_code(360.0, 8), 0);
        assert_eq!(angle_code(-0.0, 8), 0);
        assert_eq!(angle_code(f64::NAN, 8), 0);
        assert_eq!(angle_code(359.9, 8), 255);
    }

    #[test]
    fn snap_range_clamps() {
        assert_eq!(snap_range(500.0, -100.0, 100.0, 12), 100.0);
        assert_eq!(snap_range(-500.0, -100.0, 100.0, 12), -100.0);
        assert_eq!(
            snap_range(0.0, -1.0, 1.0, 1),
            1.0,
            "half rounds away from zero"
        );
    }

    #[test]
    fn pull_is_pack_then_hold() {
        let cols = DEFAULTS;
        let before = held(&cols, 0.0);
        let out = pull(&cols, &extra(), 0.0, &before, &WIRE);
        assert_eq!(out[..4], pack(&cols, &extra()));
        assert_eq!(out[4], 0);
    }
}
