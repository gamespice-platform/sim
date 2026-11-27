//! The packed pre-motor pass: what FC's players' loop works out per player before it queues the
//! motor row, for every row in one call, in the script's order and precision.
//!
//! Per row it answers two things the script computed per player per tick:
//! - FC's `FcPlayerBody.body_delta` gate: whether the body's motor runs this tick and with which
//!   delta. A gap out of range (no last tick, not after it, or longer than `max_gap`) resets
//!   (kind [`RESET`], `d = delta`); a gap of at least `every`, a button change, a waiting push, a
//!   mode other than ground, a slide or a wall run runs (kind [`RUN`], `d = delta * gap` in f64);
//!   anything else sits the tick out (kind [`SKIP`], `d = 0`; the caller writes the view and
//!   advances the state's tick).
//! - FC's `FcPlayerBody.tick_pre` ladder wish: `GSFpsMotor._view_basis(yaw, 0)`'s right and
//!   forward scaled by the command's move in f32, flattened and `Vector3.normalized()` in f32.
//!
//! Rows are packed f64: [`IN`] floats in, [`OUT`] floats out (`[kind, d, wish_x, wish_z]`).

/// Floats per input row.
pub const IN: usize = 13;
/// This tick.
pub const IN_TICK: usize = 0;
/// `FcPlayerBody.body_last_tick` (negative: none).
pub const IN_LAST: usize = 1;
/// `FcPlayerBody.body_every`.
pub const IN_EVERY: usize = 2;
/// The tick's delta.
pub const IN_DELTA: usize = 3;
/// The sanitised command's buttons.
pub const IN_BUTTONS: usize = 4;
/// The state's previous buttons (`GSFpsStateColumns` 13).
pub const IN_PREV_BUTTONS: usize = 5;
/// `GSFpsController.has_external_velocity()` (1 or 0).
pub const IN_EXT_VEL: usize = 6;
/// The state's mode (columns 9).
pub const IN_MODE: usize = 7;
/// The state's slide time (columns 17; negative: not sliding).
pub const IN_SLIDE: usize = 8;
/// The state's wall run time (columns 22; negative: not wall running).
pub const IN_WALL: usize = 9;
/// The yaw in degrees before the tick (`FcPlayerBody._yaw_before_tick`).
pub const IN_YAW: usize = 10;
/// The command's move x (f32 widened).
pub const IN_MOVE_X: usize = 11;
/// The command's move y (f32 widened).
pub const IN_MOVE_Y: usize = 12;

/// Floats per output row: kind, delta, wish x, wish z.
pub const OUT: usize = 4;
/// The gap was out of range: the body's last tick becomes this tick, `d` is the tick's delta.
pub const RESET: f64 = 0.0;
/// The motor runs: the body's last tick becomes this tick, `d` is the made-up delta.
pub const RUN: f64 = 1.0;
/// The body sits this tick out: `d` is 0.
pub const SKIP: f64 = 2.0;

/// The gate alone: `(kind, d)`.
pub fn gate(r: &[f64], max_gap: i64, ground_mode: i64) -> (f64, f64) {
    let tick = r[IN_TICK] as i64;
    let last = r[IN_LAST] as i64;
    let gap = tick - last;
    let delta = r[IN_DELTA];
    if last < 0 || gap <= 0 || gap > max_gap {
        return (RESET, delta);
    }
    if gap as f64 >= r[IN_EVERY]
        || r[IN_BUTTONS] as i64 != r[IN_PREV_BUTTONS] as i64
        || r[IN_EXT_VEL] != 0.0
        || r[IN_MODE] as i64 != ground_mode
        || r[IN_SLIDE] >= 0.0
        || r[IN_WALL] >= 0.0
    {
        return (RUN, delta * gap as f64);
    }
    (SKIP, 0.0)
}

/// The ladder wish: `(x, z)` of `Vector3(intent.x, 0, intent.z).normalized()` in f32, `intent`
/// being `right * move.x + forward * move.y` of the yaw's view basis at pitch 0.
pub fn wish(yaw_degrees: f64, move_x: f64, move_y: f64) -> (f32, f32) {
    let yaw = yaw_degrees * (std::f64::consts::PI / 180.0);
    let (s, c) = (yaw.sin(), yaw.cos());
    // forward = (-sin * cos(0), sin(0), -cos * cos(0)); right = (cos, 0, -sin); each stored f32.
    // Multiplying by cos(0) = 1 and adding 0 * 0 (the flattened y) are exact, so both are left out.
    let fx = (-s) as f32;
    let fz = (-c) as f32;
    let rx = c as f32;
    let rz = (-s) as f32;
    let (mx, my) = (move_x as f32, move_y as f32);
    let ix = rx * mx + fx * my;
    let iz = rz * mx + fz * my;
    let len2 = ix * ix + iz * iz;
    if len2 == 0.0 {
        return (0.0, 0.0);
    }
    let len = len2.sqrt();
    (ix / len, iz / len)
}

/// One row: writes [`OUT`] floats to `out`.
pub fn row(r: &[f64], max_gap: i64, ground_mode: i64, out: &mut [f64]) {
    let (kind, d) = gate(r, max_gap, ground_mode);
    let (wx, wz) = wish(r[IN_YAW], r[IN_MOVE_X], r[IN_MOVE_Y]);
    out[0] = kind;
    out[1] = d;
    out[2] = f64::from(wx);
    out[3] = f64::from(wz);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grounded walker at tick 10, last ticked at 9, every 2, same buttons.
    fn walker() -> [f64; IN] {
        let mut r = [0.0; IN];
        r[IN_TICK] = 10.0;
        r[IN_LAST] = 9.0;
        r[IN_EVERY] = 2.0;
        r[IN_DELTA] = 1.0 / 60.0;
        r[IN_BUTTONS] = 4.0;
        r[IN_PREV_BUTTONS] = 4.0;
        r[IN_SLIDE] = -1.0;
        r[IN_WALL] = -1.0;
        r
    }

    #[test]
    fn out_of_range_gap_resets() {
        for last in [-1.0, 10.0, 11.0, 1.0] {
            let mut r = walker();
            r[IN_LAST] = last;
            assert_eq!(gate(&r, 8, 0), (RESET, 1.0 / 60.0));
        }
    }

    #[test]
    fn steady_walker_skips_until_every() {
        let r = walker();
        assert_eq!(gate(&r, 8, 0), (SKIP, 0.0));
        let mut r = walker();
        r[IN_LAST] = 8.0;
        assert_eq!(gate(&r, 8, 0), (RUN, 2.0 / 60.0));
    }

    #[test]
    fn edges_run_at_once() {
        let edits: [(usize, f64); 5] = [
            (IN_BUTTONS, 5.0),
            (IN_EXT_VEL, 1.0),
            (IN_MODE, 1.0),
            (IN_SLIDE, 0.0),
            (IN_WALL, 0.25),
        ];
        for (i, v) in edits {
            let mut r = walker();
            r[i] = v;
            assert_eq!(gate(&r, 8, 0), (RUN, 1.0 / 60.0), "column {i}");
        }
    }

    #[test]
    fn wish_matches_view_basis() {
        assert_eq!(wish(0.0, 0.0, 0.0), (0.0, 0.0));
        // Yaw 0, forward: -Z.
        assert_eq!(wish(0.0, 0.0, 1.0), (0.0, -1.0));
        // Yaw 0, right: +X.
        assert_eq!(wish(0.0, 1.0, 0.0), (1.0, 0.0));
        let (x, z) = wish(37.5, 0.6, -0.8);
        assert!(((x * x + z * z) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn row_packs_gate_and_wish() {
        let mut r = walker();
        r[IN_YAW] = 90.0;
        r[IN_MOVE_Y] = 1.0;
        let mut out = [9.0; OUT];
        row(&r, 8, 0, &mut out);
        let (wx, wz) = wish(90.0, 0.0, 1.0);
        assert_eq!(out, [SKIP, 0.0, f64::from(wx), f64::from(wz)]);
    }
}
