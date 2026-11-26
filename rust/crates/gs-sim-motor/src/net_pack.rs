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

const POSITION: usize = 0;
const VELOCITY: usize = 3;

/// `GSFpsNetSync.wall_jump_counts`: `(airborne total, per-wall count)`, `(0, 0)` for a code it never writes.
pub fn wall_jump_counts(code: i64) -> (i64, i64) {
    for total in 1..4 {
        let first = ((total * (total - 1)) >> 1) + 1;
        if code >= first && code < first + total {
            return (total, code - first + 1);
        }
    }
    (0, 0)
}

fn field(flags: i64, shift: u32, bits: u32) -> f64 {
    ((flags >> shift) & ((1 << bits) - 1)) as f64
}

/// `GSFpsNetSync.unpack_flags` into `cols`; returns `crouch_held`, which is not a column.
pub fn unpack_flags(cols: &mut [f64], flags: i64, timer_scale: f64) -> bool {
    cols[MODE] = (flags & MODE_MASK) as f64;
    cols[LAUNCH_COOLDOWN_LEFT] = field(flags, LAUNCH_SHIFT, LAUNCH_BITS) / timer_scale;
    let slide = field(flags, SLIDE_SHIFT, SLIDE_BITS);
    cols[SLIDE_TIME] = if slide == NOT_SLIDING as f64 {
        -1.0
    } else {
        slide / timer_scale
    };
    cols[SLIDE_COOLDOWN_LEFT] =
        field(flags, SLIDE_COOLDOWN_SHIFT, SLIDE_COOLDOWN_BITS) / timer_scale;
    cols[PREVIOUS_BUTTONS] = ((flags >> BUTTONS_SHIFT) & ((1 << BUTTON_BITS) - 1)) as f64;
    cols[DASH_COOLDOWN_LEFT] = field(flags, DASH_SHIFT, DASH_BITS) / timer_scale;
    let wall_run = field(flags, WALL_RUN_SHIFT, WALL_RUN_BITS) as i64;
    cols[WALL_RUN_TIME] = if wall_run >= WALL_RUN_FIRST_TIME {
        (wall_run - WALL_RUN_FIRST_TIME) as f64 / timer_scale
    } else {
        -1.0
    };
    cols[WALL_RUN_SPENT_COL] = if wall_run == WALL_RUN_SPENT { 1.0 } else { 0.0 };
    let (in_air, same_wall) =
        wall_jump_counts(field(flags, WALL_JUMP_COUNT_SHIFT, WALL_JUMP_COUNT_BITS) as i64);
    cols[WALL_JUMPS_IN_AIR] = in_air as f64;
    cols[WALL_JUMP_COUNT] = same_wall as f64;
    cols[WALL_JUMP_HEADING] = field(flags, WALL_HEADING_SHIFT, 3);
    flags & FLAG_CROUCH_HELD != 0
}

/// `GSFpsNetSync.unpack_timers2` into `cols`.
pub fn unpack_timers2(cols: &mut [f64], code: i64) {
    let slide = code & T2_MAX;
    cols[SLIDE_TIME] = if slide == 0 {
        -1.0
    } else {
        (slide - 1) as f64 / JUMP_TIMER_SCALE
    };
    cols[SLIDE_COOLDOWN_LEFT] = ((code >> JUMP_TIMER_BITS) & T2_MAX) as f64 / JUMP_TIMER_SCALE;
    let run = (code >> (2 * JUMP_TIMER_BITS)) & T2_MAX;
    cols[WALL_RUN_TIME] = if run >= WALL_RUN_FIRST_TIME {
        (run - WALL_RUN_FIRST_TIME) as f64 / JUMP_TIMER_SCALE
    } else {
        -1.0
    };
    cols[WALL_RUN_SPENT_COL] = if run == WALL_RUN_SPENT { 1.0 } else { 0.0 };
}

/// What a receiver writes into a state's columns: the replicated position, velocity and yaw, then
/// [`unpack_flags`], then [`unpack_timers2`] when `timers2` is `Some` (it overrides the coarse slide and
/// wall-run fields). Returns `crouch_held`.
pub fn unpack(
    cols: &mut [f64],
    position: [f32; 3],
    velocity: [f32; 3],
    yaw: f64,
    flags: i64,
    timers2: Option<i64>,
    timer_scale: f64,
) -> bool {
    for i in 0..3 {
        cols[POSITION + i] = f64::from(position[i]);
        cols[VELOCITY + i] = f64::from(velocity[i]);
    }
    cols[YAW] = yaw;
    let crouch_held = unpack_flags(cols, flags, timer_scale);
    if let Some(code) = timers2 {
        unpack_timers2(cols, code);
    }
    crouch_held
}

/// Floats in an exact movement record (`GSFpsNetSync.EXACT_FLOATS`): position, velocity, crouch fraction,
/// time since grounded, time since jump, time since jump pressed, wall-run cooldown, stamina, stamina exhausted
/// (above 0.5), stamina delay.
pub const EXACT_FLOATS: usize = 14;

/// The column half of `GSFpsNetSync.push_exact`: an exact record's position, velocity and timers written over
/// `cols`, each `f32` widened as the script's `Vector3` and float setters do. Crouch fraction and stamina are
/// not columns; the caller writes them. Returns false (and writes nothing) for a record of another length.
pub fn unpack_exact(cols: &mut [f64], exact: &[f32]) -> bool {
    if exact.len() != EXACT_FLOATS {
        return false;
    }
    for i in 0..3 {
        cols[POSITION + i] = f64::from(exact[i]);
        cols[VELOCITY + i] = f64::from(exact[3 + i]);
    }
    cols[TIME_SINCE_GROUNDED] = f64::from(exact[7]);
    cols[TIME_SINCE_JUMP] = f64::from(exact[8]);
    cols[TIME_SINCE_JUMP_PRESSED] = f64::from(exact[9]);
    cols[WALL_RUN_COOLDOWN_LEFT] = f64::from(exact[10]);
    true
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

    #[test]
    fn unpack_exact_overwrites_only_its_columns() {
        let mut cols = DEFAULTS;
        let exact: [f32; EXACT_FLOATS] = [
            1.5, -2.25, 3.1, 0.1, -9.8, 7.0, 0.5, 0.33, 0.07, 1.9, 0.6, 0.4, 1.0, 0.2,
        ];
        let before = cols;
        assert!(unpack_exact(&mut cols, &exact));
        assert_eq!(cols[POSITION + 2], f64::from(3.1f32));
        assert_eq!(cols[VELOCITY + 1], f64::from(-9.8f32));
        assert_eq!(cols[TIME_SINCE_GROUNDED], f64::from(0.33f32));
        assert_eq!(cols[TIME_SINCE_JUMP], f64::from(0.07f32));
        assert_eq!(cols[TIME_SINCE_JUMP_PRESSED], f64::from(1.9f32));
        assert_eq!(cols[WALL_RUN_COOLDOWN_LEFT], f64::from(0.6f32));
        let written = [0, 1, 2, 3, 4, 5, 14, 15, 16, 21];
        for (i, (a, b)) in cols.iter().zip(before.iter()).enumerate() {
            if !written.contains(&i) {
                assert_eq!(a.to_bits(), b.to_bits(), "column {i}");
            }
        }
        let mut short = before;
        assert!(!unpack_exact(&mut short, &exact[..13]));
        assert_eq!(short, before);
    }

    #[test]
    fn unpack_undoes_pack() {
        let mut cols = DEFAULTS;
        cols[MODE] = 3.0;
        cols[SLIDE_TIME] = 0.5;
        cols[SLIDE_COOLDOWN_LEFT] = 0.25;
        cols[LAUNCH_COOLDOWN_LEFT] = 1.5;
        cols[DASH_COOLDOWN_LEFT] = 2.0;
        cols[PREVIOUS_BUTTONS] = 37.0;
        cols[WALL_RUN_TIME] = 0.75;
        cols[WALL_JUMPS_IN_AIR] = 3.0;
        cols[WALL_JUMP_COUNT] = 2.0;
        cols[WALL_JUMP_HEADING] = 7.0;
        let mut e = extra();
        e.crouch_held = true;
        let codes = pack(&cols, &e);
        let mut out = DEFAULTS;
        let held = unpack(
            &mut out,
            [1.0, 2.0, 3.0],
            [4.0, 5.0, 6.0],
            0.5,
            codes[0],
            Some(codes[2]),
            64.0,
        );
        assert!(held);
        for (i, want) in cols.iter().enumerate().skip(6) {
            if i != YAW {
                assert_eq!(out[i], *want, "col {i}");
            }
        }
        assert_eq!(&out[0..6], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(out[YAW], 0.5);
    }

    #[test]
    fn unpack_spent_and_idle() {
        let mut cols = DEFAULTS;
        cols[WALL_RUN_SPENT_COL] = 1.0;
        let codes = pack(&cols, &extra());
        let mut out = DEFAULTS;
        assert!(!unpack(
            &mut out, [0.0; 3], [0.0; 3], 0.0, codes[0], None, 64.0
        ));
        assert_eq!(out[WALL_RUN_SPENT_COL], 1.0);
        assert_eq!(out[WALL_RUN_TIME], -1.0);
        assert_eq!(out[SLIDE_TIME], -1.0);
        assert_eq!(wall_jump_counts(0), (0, 0));
        assert_eq!(wall_jump_counts(7), (0, 0));
        for total in 1..4 {
            for same in 1..=total {
                assert_eq!(wall_jump_counts(wall_jump_code(total, same)), (total, same));
            }
        }
    }
}
