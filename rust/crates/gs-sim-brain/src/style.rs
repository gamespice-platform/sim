//! GSBotStyle's place queries (gs-bot `gs_bot_style.gd`): `cover_near`, a standable spot a
//! threat cannot see, and `vantage`, a waypoint that sees far. The script's arithmetic in its
//! order and precision (positions `f32` as `Vector3`/`Vector2` hold them, scores `f64`), drawing
//! the same numbers from the bot's style stream. The world's answers (the nearest standable
//! point, line of sight) are the caller's closures, asked in the script's order.

use crate::num::Stream;

/// A position as a `Vector3` holds it.
pub type P = [f32; 3];

/// `Vector3.INF`: "none" for a standable point or a face.
pub const NONE: P = [f32::INFINITY; 3];
/// Directions tried for cover, degrees off straight away (GSBotStyle.COVER_TURNS).
pub const COVER_TURNS: [f32; 3] = [0.0, 55.0, 110.0];
/// How far cover is tried (GSBotStyle.COVER_REACH).
pub const COVER_REACH: [f32; 2] = [3.5, 7.0];
/// Sight rays a cover search asks at most (GSBotStyle.COVER_RAYS).
pub const COVER_RAYS: i32 = 6;
/// Metres a vantage may be from the bot (GSBotStyle.VANTAGE_FAR).
pub const VANTAGE_FAR: f64 = 60.0;

/// `Vector3.length()`: the squares summed x, y, z, in `f32`.
#[inline]
fn length(v: P) -> f32 {
    let (x2, y2, z2) = (v[0] * v[0], v[1] * v[1], v[2] * v[2]);
    (x2 + y2 + z2).sqrt()
}

/// `a.distance_to(b)`: `(b - a).length()`.
#[inline]
fn distance(a: P, b: P) -> f32 {
    length([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
}

/// `at + Vector3.UP * up` (the zero components added too, as the engine adds them).
#[inline]
fn raised(at: P, up: f64) -> P {
    let u = up as f32;
    [at[0] + 0.0, at[1] + u, at[2] + 0.0]
}

/// `points[mini(n - 1, int(rng.next_unit() * n))]`.
#[inline]
fn draw(points: &[P], rng: &mut Stream) -> P {
    let n = points.len();
    let i = (rng.next_unit() * n as f64) as i64;
    points[(i.max(0) as usize).min(n - 1)]
}

/// `GSBotStyle.cover_near`: a spot near `here` that the threat cannot see (`sees(to)`: the
/// threat's eye sees `to`, asked of the spot raised 0.9 m), tried away from `threat_pos` and to
/// either side, else a few metres straight back. `snaps`: the world has a standable-point query,
/// `walk(at)` ([`NONE`] when there is none near).
pub fn cover_near(
    here: P,
    threat_pos: P,
    rng: &mut Stream,
    snaps: bool,
    mut walk: impl FnMut(P) -> P,
    mut sees: impl FnMut(P) -> bool,
) -> P {
    let (mut ax, mut ay) = (here[0] - threat_pos[0], here[2] - threat_pos[2]);
    if ((ax * ax + ay * ay).sqrt() as f64) > 0.01 {
        let l = ax * ax + ay * ay;
        if l != 0.0 {
            let l = l.sqrt();
            ax /= l;
            ay /= l;
        }
    } else {
        (ax, ay) = (0.0, 1.0);
    }
    let flip = if rng.next_unit() < 0.5 { 1.0 } else { -1.0 };
    let mut rays = 0;
    for reach in COVER_REACH {
        for turn in COVER_TURNS {
            for sgn in 0..2 {
                if turn == 0.0 && sgn == 1 {
                    continue;
                }
                // `Vector2.rotated` takes the angle as `real_t`.
                let by =
                    (deg_to_rad(turn as f64) * flip * if sgn == 0 { 1.0 } else { -1.0 }) as f32;
                let (sine, cosi) = (by.sin(), by.cos());
                let (dx, dy) = (ax * cosi - ay * sine, ax * sine + ay * cosi);
                let mut at = [
                    here[0] + dx * reach,
                    here[1] + 0.0 * reach,
                    here[2] + dy * reach,
                ];
                if snaps {
                    at = walk(at);
                    if at == NONE {
                        continue;
                    }
                }
                rays += 1;
                if !sees(raised(at, 0.9)) {
                    return at;
                }
                if rays >= COVER_RAYS {
                    return back_off(here, ax, ay, snaps, &mut walk);
                }
            }
        }
    }
    back_off(here, ax, ay, snaps, &mut walk)
}

fn back_off(here: P, ax: f32, ay: f32, snaps: bool, walk: &mut impl FnMut(P) -> P) -> P {
    let at = [here[0] + ax * 5.0, here[1] + 0.0 * 5.0, here[2] + ay * 5.0];
    if snaps {
        let snapped = walk(at);
        return if snapped != NONE { snapped } else { here };
    }
    at
}

/// `deg_to_rad` on a `float`: `deg * (PI / 180)`.
#[inline]
fn deg_to_rad(deg: f64) -> f64 {
    deg * (std::f64::consts::PI / 180.0)
}

/// `GSBotStyle.vantage`: of six waypoints drawn from `points`, the one that best sees others far
/// off (`sees(eye, to)`), higher the better, not beyond [`VANTAGE_FAR`] of `here` nor within 4 m
/// of `away_from` ([`NONE`]: anywhere). `(at, face)` (`face` [`NONE`] when it saw none), or
/// `None` for no points or no candidate.
pub fn vantage(
    here: P,
    away_from: P,
    points: &[P],
    rng: &mut Stream,
    mut sees: impl FnMut(P, P) -> bool,
) -> Option<(P, P)> {
    if points.is_empty() {
        return None;
    }
    let mut best = None;
    let mut best_score = f64::NEG_INFINITY;
    for _ in 0..6 {
        let at = draw(points, rng);
        let far = distance(at, here);
        if far as f64 > VANTAGE_FAR || (away_from != NONE && (distance(at, away_from) as f64) < 4.0)
        {
            continue;
        }
        let eye = raised(at, 1.6);
        let mut seen = 0;
        let mut face = NONE;
        for _ in 0..3 {
            let q = draw(points, rng);
            let gap = distance(q, at) as f64;
            if gap < 8.0 || gap > 70.0 {
                continue;
            }
            if sees(eye, raised(q, 1.2)) {
                seen += 1;
                if face == NONE {
                    face = q;
                }
            }
        }
        let score = seen as f64 + (at[1] as f64 - here[1] as f64) * 0.1 - far as f64 * 0.02
            + rng.next_unit() * 0.5;
        if score > best_score {
            best_score = score;
            best = Some((at, face));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_takes_the_first_hidden_spot_and_one_draw() {
        let mut rng = Stream::new(77, 0);
        let mut asked = 0;
        let at = cover_near(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -10.0],
            &mut rng,
            false,
            |p| p,
            |_| {
                asked += 1;
                asked < 3
            },
        );
        assert_eq!(rng.counter, 1);
        assert_eq!(asked, 3);
        // Straight away first (+z), then the two 55-degree turns: the third ray is a turn.
        assert!((length(at) - 3.5).abs() < 1e-5);
        assert!(at[0].abs() > 1.0);
    }

    #[test]
    fn cover_backs_off_after_six_rays_and_snaps() {
        let mut rng = Stream::new(5, 3);
        let mut rays = 0;
        let at = cover_near(
            [1.0, 2.0, 3.0],
            [1.0, 2.0, 3.0],
            &mut rng,
            true,
            |p| p,
            |_| {
                rays += 1;
                true
            },
        );
        assert_eq!(rays, COVER_RAYS);
        // Threat on top of the bot: away is +z, back off 5 m.
        assert_eq!(at, [1.0, 2.0, 8.0]);
        let none = cover_near(
            [1.0, 2.0, 3.0],
            [0.0, 0.0, 0.0],
            &mut rng,
            true,
            |_| NONE,
            |_| false,
        );
        assert_eq!(none, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn vantage_draws_as_the_script_does() {
        let points: Vec<P> = (0..20)
            .map(|i| [i as f32 * 3.0, (i % 4) as f32, 0.0])
            .collect();
        let mut rng = Stream::new(1234, 9);
        let mut copy = rng;
        let got = vantage([0.0, 0.0, 0.0], NONE, &points, &mut rng, |_, _| true);
        assert!(got.is_some());
        // Every draw is one unit off the stream: replay the script's skips by hand.
        let n = points.len();
        let mut used = 0;
        for _ in 0..6 {
            let at = points[((copy.next_unit() * n as f64) as usize).min(n - 1)];
            used += 1;
            if distance(at, [0.0; 3]) as f64 > VANTAGE_FAR {
                continue;
            }
            for _ in 0..3 {
                copy.next_unit();
                used += 1;
            }
            copy.next_unit();
            used += 1;
        }
        assert_eq!(rng.counter, 9 + used);
        assert_eq!(vantage([0.0; 3], NONE, &[], &mut rng, |_, _| true), None);
    }
}
