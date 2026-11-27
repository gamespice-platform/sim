//! The head of FC's `FcPlayerBody.tick_post_head`, before the map's mechanics: the hit-zone
//! squeeze factor of `fit_hitboxes_to_crouch` and the map-volume skip of `_volumes_skip`, in one
//! answer, in the script's order and precision (f64 for the tunables, f32 for the positions).

/// `FcVolumePrefilter`'s reach: the slack a hull may move since the prefilter's point
/// (`SLACK`) and the box growth it was asked with (`BELOW`, `ABOVE`, f32 like the script's
/// `Vector3` constants).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub slack: f64,
    pub below: [f32; 3],
    pub above: [f32; 3],
}

impl Default for Reach {
    fn default() -> Self {
        Self {
            slack: 1.0,
            below: [3.0, 3.0, 3.0],
            above: [3.0, 5.0, 3.0],
        }
    }
}

/// One player's inputs: the controller's tunables (`crouch_height`, `stand_height`) for the
/// squeeze, the motor's (`radius`, `stand_height`) with the ladder's reach and the swim waist for
/// the skip, the tick's start and the state's position, and whether the prefilter marked the
/// player clear on this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadIn {
    pub crouch: f64,
    pub crouch_height: f64,
    pub fit_stand_height: f64,
    pub radius: f64,
    pub ladder_reach: f64,
    pub stand_height: f64,
    pub waist: f64,
    pub start: [f32; 3],
    pub position: [f32; 3],
    pub marked: bool,
}

// Godot's maxf, minf and clampf (NaN falls through as the engine's do).
fn maxf(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

fn minf(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

fn clampf(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// `fit_hitboxes_to_crouch`'s `lerpf(1, crouch_height / maxf(stand_height, 0.01), clampf(crouch, 0, 1))`.
pub fn fit_k(crouch: f64, crouch_height: f64, stand_height: f64) -> f64 {
    let to = crouch_height / maxf(stand_height, 0.01);
    let w = clampf(crouch, 0.0, 1.0);
    1.0 + (to - 1.0) * w
}

/// `_volumes_skip` past its marked check: moved at most the slack, and a body inside the reach.
pub fn volumes_clear(p: &HeadIn, r: &Reach) -> bool {
    if !p.marked {
        return false;
    }
    let slack = r.slack;
    let side = p.radius + p.ladder_reach + slack;
    let up = maxf(p.stand_height, p.waist) + slack;
    // Vector3.distance_squared_to in f32: (to - from).length_squared(), x, y, z in order.
    let dx = p.position[0] - p.start[0];
    let dy = p.position[1] - p.start[1];
    let dz = p.position[2] - p.start[2];
    let d2 = dx * dx + dy * dy + dz * dz;
    let (b, a) = (r.below, r.above);
    (d2 as f64) <= slack * slack
        && side
            <= minf(
                minf(b[0] as f64, b[2] as f64),
                minf(a[0] as f64, a[2] as f64),
            )
        && up <= b[1] as f64
        && slack <= a[1] as f64
}

/// Both answers in one float: the squeeze factor, negated when the player may skip the map's
/// volumes. NaN when the factor is not above zero (the sign could not carry the skip): the caller
/// runs the script then.
pub fn head(p: &HeadIn, r: &Reach) -> f64 {
    let k = fit_k(p.crouch, p.crouch_height, p.fit_stand_height);
    // Not above zero, NaN included.
    if k.is_nan() || k <= 0.0 {
        return f64::NAN;
    }
    if volumes_clear(p, r) { -k } else { k }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing() -> HeadIn {
        HeadIn {
            crouch: 0.0,
            crouch_height: 1.0,
            fit_stand_height: 1.8,
            radius: 0.4,
            ladder_reach: 0.5,
            stand_height: 1.8,
            waist: 0.9,
            start: [1.0, 2.0, 3.0],
            position: [1.25, 2.0, 3.5],
            marked: true,
        }
    }

    #[test]
    fn standing_is_one_and_clear() {
        assert_eq!(head(&standing(), &Reach::default()), -1.0);
    }

    #[test]
    fn unmarked_is_never_clear() {
        let p = HeadIn {
            marked: false,
            ..standing()
        };
        assert_eq!(head(&p, &Reach::default()), 1.0);
    }

    #[test]
    fn crouch_is_the_script_lerp() {
        for c in [0.0, 0.25, 0.5, 0.731, 1.0, 1.5, -0.2] {
            let p = HeadIn {
                crouch: c,
                ..standing()
            };
            let to = 1.0f64 / 1.8f64;
            let w = c.clamp(0.0, 1.0);
            let want = 1.0 + (to - 1.0) * w;
            assert_eq!((-head(&p, &Reach::default())).to_bits(), want.to_bits());
        }
    }

    #[test]
    fn moved_past_slack_is_not_clear() {
        let p = HeadIn {
            position: [1.0, 2.0, 4.01],
            ..standing()
        };
        assert_eq!(head(&p, &Reach::default()), 1.0);
        let edge = HeadIn {
            position: [1.0, 2.0, 4.0],
            ..standing()
        };
        assert_eq!(head(&edge, &Reach::default()), -1.0);
    }

    #[test]
    fn body_outside_reach_is_not_clear() {
        for p in [
            HeadIn {
                radius: 2.0,
                ..standing()
            },
            HeadIn {
                waist: 2.5,
                ..standing()
            },
        ] {
            assert_eq!(head(&p, &Reach::default()), 1.0);
        }
        let r = Reach {
            slack: 6.0,
            ..Reach::default()
        };
        assert_eq!(head(&standing(), &r), 1.0);
    }

    #[test]
    fn zero_factor_is_nan() {
        let p = HeadIn {
            crouch: 1.0,
            crouch_height: 0.0,
            ..standing()
        };
        assert!(head(&p, &Reach::default()).is_nan());
        let n = HeadIn {
            crouch: f64::NAN,
            ..standing()
        };
        assert!(head(&n, &Reach::default()).is_nan());
    }
}
