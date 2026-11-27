//! The packed post-head pass: what FC's `FcPlayerBody.tick_post_head` works out from a player's
//! motor run before its weapons, for every row in one call, in the script's order and precision.
//!
//! Per row it takes the tick's first motor input columns (`before`) and last output columns
//! (`new`) of that player's resident motor slot and answers:
//! - whether the tick may skip the map's volumes (FC's `_volumes_skip`): the row's gate (marked
//!   clear this tick and a body inside the prefilter's reaches, both decided by the caller) and
//!   the feet moved at most `slack` from the tick start, as Godot's f32
//!   `start.distance_squared_to(end)` compared in f64 with `slack * slack`;
//! - the hitbox height scale (FC's `fit_hitboxes_to_crouch`): `lerpf(1, ratio, clampf(crouch, 0, 1))`
//!   in f64, `ratio` being the caller's `crouch_height / maxf(stand_height, 0.01)`;
//! - the end feet, which the caller compares with its state before trusting the row.
//!
//! Rows are packed f64: [`IN`] floats in (`[slot, gate, ratio]`), [`OUT`] floats out
//! (`[valid, skip, k, end_x, end_y, end_z]`). A row whose slot did not run this loop answers
//! `valid` 0 and zeros: the caller runs its own path.

/// Floats per input row: motor slot, gate (1 or 0), crouch height ratio.
pub const IN: usize = 3;
/// Floats per output row: valid, skip, height scale, end feet xyz.
pub const OUT: usize = 6;
/// `GSFpsStateColumns.CROUCH_FRACTION`.
pub const COL_CROUCH: usize = 30;
/// Columns a row needs to answer: x, y, z first and the crouch fraction.
pub const COLS_MIN: usize = COL_CROUCH + 1;

/// Godot's `Vector3.distance_squared_to` on two f32 points widened to f64: `(to - from)`
/// squared in f32, summed in engine order.
pub fn distance_squared_f32(from: &[f64], to: &[f64]) -> f32 {
    let dx = to[0] as f32 - from[0] as f32;
    let dy = to[1] as f32 - from[1] as f32;
    let dz = to[2] as f32 - from[2] as f32;
    dx * dx + dy * dy + dz * dz
}

/// GDScript `clampf`.
fn clampf(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// One row: `input` is `[slot, gate, ratio]` (the slot is the caller's lookup), `ran` the slot's
/// `(before, new)` columns when it ran this loop. Writes [`OUT`] floats to `out`.
pub fn row(input: &[f64], slack: f64, ran: Option<(&[f64], &[f64])>, out: &mut [f64]) {
    let Some((before, new)) = ran.filter(|(b, n)| b.len() >= 3 && n.len() >= COLS_MIN) else {
        out[..OUT].fill(0.0);
        return;
    };
    let near = f64::from(distance_squared_f32(before, new)) <= slack * slack;
    let ratio = input[2];
    let k = 1.0 + (ratio - 1.0) * clampf(new[COL_CROUCH], 0.0, 1.0);
    out[0] = 1.0;
    out[1] = if input[1] != 0.0 && near { 1.0 } else { 0.0 };
    out[2] = k;
    out[3] = new[0];
    out[4] = new[1];
    out[5] = new[2];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(x: f64, y: f64, z: f64, crouch: f64) -> Vec<f64> {
        let mut c = vec![0.0; 32];
        c[0] = x;
        c[1] = y;
        c[2] = z;
        c[COL_CROUCH] = crouch;
        c
    }

    #[test]
    fn missing_slot_is_invalid() {
        let mut out = [9.0; OUT];
        row(&[3.0, 1.0, 0.5], 1.0, None, &mut out);
        assert_eq!(out, [0.0; OUT]);
    }

    #[test]
    fn skip_needs_gate_and_slack() {
        let b = cols(0.0, 0.0, 0.0, 0.0);
        let near = cols(0.5, 0.0, 0.5, 0.0);
        let far = cols(1.0, 0.0, 0.5, 0.0);
        let mut out = [0.0; OUT];
        row(&[0.0, 1.0, 0.5], 1.0, Some((&b, &near)), &mut out);
        assert_eq!(out[..2], [1.0, 1.0]);
        row(&[0.0, 0.0, 0.5], 1.0, Some((&b, &near)), &mut out);
        assert_eq!(out[1], 0.0);
        row(&[0.0, 1.0, 0.5], 1.0, Some((&b, &far)), &mut out);
        assert_eq!(out[1], 0.0);
        // Exactly the slack still skips (<=).
        let edge = cols(1.0, 0.0, 0.0, 0.0);
        row(&[0.0, 1.0, 0.5], 1.0, Some((&b, &edge)), &mut out);
        assert_eq!(out[1], 1.0);
    }

    #[test]
    fn distance_is_f32() {
        let b = [0.1_f32 as f64, 0.2_f32 as f64, 0.3_f32 as f64];
        let n = [0.7_f32 as f64, 0.9_f32 as f64, 1.3_f32 as f64];
        let dx = 0.7_f32 - 0.1_f32;
        let dy = 0.9_f32 - 0.2_f32;
        let dz = 1.3_f32 - 0.3_f32;
        assert_eq!(distance_squared_f32(&b, &n), dx * dx + dy * dy + dz * dz);
    }

    #[test]
    fn height_scale_is_lerp_of_clamped_crouch() {
        let b = cols(0.0, 0.0, 0.0, 0.0);
        let mut out = [0.0; OUT];
        let ratio = 1.1 / 1.8;
        for (c, want) in [
            (0.0, 1.0),
            (1.0, ratio),
            (2.0, ratio),
            (-1.0, 1.0),
            (0.25, 1.0 + (ratio - 1.0) * 0.25),
        ] {
            let n = cols(1.0, 2.0, 3.0, c);
            row(&[0.0, 1.0, ratio], 1.0, Some((&b, &n)), &mut out);
            assert_eq!(out[2], want);
            assert_eq!(out[3..], [1.0, 2.0, 3.0]);
        }
    }
}
