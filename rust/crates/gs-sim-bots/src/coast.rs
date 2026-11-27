//! A coasting bot's view: the tick a bot sits out its think, its view carries on from where the last think left it
//! towards where that think turned it (FC's `FcBots._coast`), the same arithmetic in the same precision.

/// The script's `fposmod`: `fmod`, moved onto the divisor's side, with `+ 0.0` so a `-0.0` comes out `0.0`.
#[inline]
pub fn fposmod(x: f64, y: f64) -> f64 {
    let mut v = x % y;
    if (v < 0.0 && y > 0.0) || (v > 0.0 && y < 0.0) {
        v += y;
    }
    v + 0.0
}

/// Degrees from `a` to `b` the short way round (`FcBots._turn`).
#[inline]
pub fn turn(a: f64, b: f64) -> f64 {
    fposmod(b - a + 180.0, 360.0) - 180.0
}

/// The (yaw, pitch) of a coasting tick `step` ticks after the think, of `every` (the view's `Vector2` halves are
/// `f32`, the arithmetic `f64`): `from + turn(from, to) * (step + 1) / every` for yaw, a straight blend for pitch.
#[inline]
pub fn coast_view(from: (f32, f32), to: (f32, f32), step: i64, every: i64) -> (f64, f64) {
    let fraction = step.wrapping_add(1) as f64 / every as f64;
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    (
        fx + turn(fx, to.0 as f64) * fraction,
        fy + (to.1 as f64 - fy) * fraction,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fposmod_keeps_the_divisor_sign() {
        assert_eq!(fposmod(-10.0, 360.0), 350.0);
        assert_eq!(fposmod(370.0, 360.0), 10.0);
        assert_eq!(fposmod(-360.0, 360.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(fposmod(10.0, -360.0), -350.0);
    }

    #[test]
    fn turn_goes_the_short_way() {
        assert_eq!(turn(350.0, 10.0), 20.0);
        assert_eq!(turn(10.0, 350.0), -20.0);
        assert_eq!(turn(0.0, 180.0), -180.0);
        assert_eq!(turn(-170.0, 170.0), -20.0);
    }

    #[test]
    fn coast_view_blends_by_the_step() {
        let (yaw, pitch) = coast_view((350.0, -10.0), (10.0, 20.0), 0, 4);
        assert_eq!(yaw, 355.0);
        assert_eq!(pitch, -2.5);
        let (yaw, pitch) = coast_view((350.0, -10.0), (10.0, 20.0), 3, 4);
        assert_eq!(yaw, 370.0);
        assert_eq!(pitch, 20.0);
    }

    #[test]
    fn coast_view_works_in_f64_from_f32_halves() {
        let from = (0.1f32, 0.2f32);
        let to = (0.7f32, -0.3f32);
        let (yaw, pitch) = coast_view(from, to, 1, 3);
        let fraction = 2.0 / 3.0;
        assert_eq!(
            yaw.to_bits(),
            (0.1f32 as f64 + (0.7f32 as f64 - 0.1f32 as f64) * fraction).to_bits()
        );
        assert_eq!(
            pitch.to_bits(),
            (0.2f32 as f64 + (-0.3f32 as f64 - 0.2f32 as f64) * fraction).to_bits()
        );
    }
}
