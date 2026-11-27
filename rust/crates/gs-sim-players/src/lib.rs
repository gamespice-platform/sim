//! Player body tick state in packed columns, pure Rust in the script's order and precision.
//!
//! Stage 1 holds the idle posture group of FC's `FcPlayerBody.tick_post`: from the motor state a
//! player ends its tick with (velocity, mode, crouch, slide, wall run, sprint) and its last hurt
//! tick, the 9-float posture row gs-weapon's spread reads (`[speed, airborne, crouched, aiming,
//! sliding, sprinting, wall_running, tick, hurt_tick]`, `GSWeaponHitscan`'s row) and whether the
//! pose is settled with no flinch left (FC's `_idle_row`, which `AwWeaponRig.simulate_idle_row`
//! takes as `still`).
//!
//! Rows are packed f64: [`IN`] floats in per player (the `IN_*` columns), [`OUT`] floats out (the
//! posture row, then [`OUT_STILL`]).
//!
//! [`head`] is the head of `tick_post_head`: the hit-zone squeeze and the map-volume skip. Velocity components are the engine's f32 values widened, so
//! the speed is the f32 `Vector2(x, z).length()` the script computes.

pub mod head;

/// Floats per input row.
pub const IN: usize = 10;
/// Velocity x (an f32 widened).
pub const IN_VEL_X: usize = 0;
/// Velocity z (an f32 widened).
pub const IN_VEL_Z: usize = 1;
/// `GSFpsState.mode`.
pub const IN_MODE: usize = 2;
/// `GSFpsState.crouch_fraction`.
pub const IN_CROUCH: usize = 3;
/// `GSFpsState.slide_time` (negative: not sliding).
pub const IN_SLIDE: usize = 4;
/// `GSFpsState.wall_run_time` (negative: not wall running).
pub const IN_WALL: usize = 5;
/// `GSFpsState.sprinting`, 0 or 1.
pub const IN_SPRINT: usize = 6;
/// The tick being run.
pub const IN_TICK: usize = 7;
/// `GSHealth.last_damage_tick`, or -1 with no health.
pub const IN_LAST_HURT: usize = 8;
/// The arsenal's `tick_rate`, or 0 with no arsenal.
pub const IN_RATE: usize = 9;

/// Floats per output row.
pub const OUT: usize = 10;
/// Length of the posture row at the head of an output row.
pub const POSTURE: usize = 9;
/// 1 when the pose is settled and no flinch is left, else 0.
pub const OUT_STILL: usize = 9;

/// The lowest tick rate the flinch window is counted at (FC's `maxi(arsenal.tick_rate, 64)`).
pub const MIN_RATE: i64 = 64;

/// What the script reads from outside the row: `GSFpsState.Mode.GROUND` and
/// `GSWeaponAccuracyClass.FLINCH_SEC`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub ground_mode: i64,
    pub flinch_sec: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ground_mode: 0,
            flinch_sec: 0.4,
        }
    }
}

/// One player's inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PostIn {
    pub vel_x: f32,
    pub vel_z: f32,
    pub mode: i64,
    pub crouch_fraction: f64,
    pub slide_time: f64,
    pub wall_run_time: f64,
    pub sprinting: bool,
    pub tick: i64,
    pub last_hurt: i64,
    pub rate: i64,
}

impl PostIn {
    /// Reads row [param r] (at least [`IN`] floats).
    pub fn from_row(r: &[f64]) -> Self {
        Self {
            vel_x: r[IN_VEL_X] as f32,
            vel_z: r[IN_VEL_Z] as f32,
            mode: r[IN_MODE] as i64,
            crouch_fraction: r[IN_CROUCH],
            slide_time: r[IN_SLIDE],
            wall_run_time: r[IN_WALL],
            sprinting: r[IN_SPRINT] != 0.0,
            tick: r[IN_TICK] as i64,
            last_hurt: r[IN_LAST_HURT] as i64,
            rate: r[IN_RATE] as i64,
        }
    }
}

fn flag(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// FC's `_idle_row`: writes the posture row into `out[..POSTURE]` and returns `still`.
pub fn idle_row(p: &PostIn, cfg: &Config, out: &mut [f64]) -> bool {
    // GSFpsState.horizontal_speed: Vector2(velocity.x, velocity.z).length() in f32.
    let speed = (p.vel_x * p.vel_x + p.vel_z * p.vel_z).sqrt() as f64;
    let grounded = p.mode == cfg.ground_mode;
    let crouched = p.crouch_fraction > 0.001;
    let sliding = p.slide_time >= 0.0;
    let wall = p.wall_run_time >= 0.0;
    let hurt = if p.last_hurt >= 0 { p.last_hurt } else { -1 };
    out[0] = speed;
    out[1] = flag(!grounded);
    out[2] = flag(crouched);
    out[3] = 0.0;
    out[4] = flag(sliding);
    out[5] = flag(p.sprinting);
    out[6] = flag(wall);
    out[7] = p.tick as f64;
    out[8] = hurt as f64;
    if speed != 0.0 || !grounded || crouched || sliding || p.sprinting || wall {
        return false;
    }
    if hurt < 0 || p.tick < hurt {
        return true;
    }
    let rate = p.rate.max(MIN_RATE);
    p.tick - hurt > (cfg.flinch_sec * rate as f64).ceil() as i64
}

/// [`idle_row`] over every row of `input` ([`IN`] floats each) into `out` ([`OUT`] each, the
/// still flag last). Rows past the shorter of the two are left alone; returns the rows run.
pub fn run_rows(input: &[f64], cfg: &Config, out: &mut [f64]) -> usize {
    let n = (input.len() / IN).min(out.len() / OUT);
    for i in 0..n {
        let p = PostIn::from_row(&input[i * IN..i * IN + IN]);
        let o = &mut out[i * OUT..i * OUT + OUT];
        o[OUT_STILL] = flag(idle_row(&p, cfg, o));
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest() -> PostIn {
        PostIn {
            slide_time: -1.0,
            wall_run_time: -1.0,
            tick: 1000,
            last_hurt: -1,
            ..Default::default()
        }
    }

    fn run(p: &PostIn) -> ([f64; POSTURE], bool) {
        let mut o = [0.0; POSTURE];
        let still = idle_row(p, &Config::default(), &mut o);
        (o, still)
    }

    #[test]
    fn rest_never_hurt_is_still() {
        let (row, still) = run(&rest());
        assert!(still);
        assert_eq!(row, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1000.0, -1.0]);
    }

    #[test]
    fn speed_is_f32_length() {
        let p = PostIn {
            vel_x: 3.1,
            vel_z: -4.7,
            ..rest()
        };
        let (row, still) = run(&p);
        let want = (3.1f32 * 3.1f32 + 4.7f32 * 4.7f32).sqrt() as f64;
        assert_eq!(row[0].to_bits(), want.to_bits());
        assert!(!still);
    }

    #[test]
    fn each_flag_breaks_still() {
        for p in [
            PostIn { mode: 1, ..rest() },
            PostIn {
                crouch_fraction: 0.0011,
                ..rest()
            },
            PostIn {
                slide_time: 0.0,
                ..rest()
            },
            PostIn {
                wall_run_time: 0.0,
                ..rest()
            },
            PostIn {
                sprinting: true,
                ..rest()
            },
        ] {
            assert!(!run(&p).1, "{p:?}");
        }
        assert!(
            run(&PostIn {
                crouch_fraction: 0.001,
                ..rest()
            })
            .1
        );
        let (row, _) = run(&PostIn {
            mode: 1,
            crouch_fraction: 1.0,
            slide_time: 0.2,
            wall_run_time: 0.1,
            sprinting: true,
            ..rest()
        });
        assert_eq!(&row[1..7], &[1.0, 1.0, 0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn flinch_window() {
        // ceil(0.4 * 64) = 26: still only once more than 26 ticks have passed.
        assert!(
            !run(&PostIn {
                last_hurt: 974,
                ..rest()
            })
            .1
        );
        assert!(
            run(&PostIn {
                last_hurt: 973,
                ..rest()
            })
            .1
        );
        // A hurt tick ahead of this one (a rewound clock) counts as settled.
        assert!(
            run(&PostIn {
                last_hurt: 1001,
                ..rest()
            })
            .1
        );
        // 128 Hz: ceil(51.2) = 52; a rate under 64 counts as 64.
        assert!(
            !run(&PostIn {
                last_hurt: 948,
                rate: 128,
                ..rest()
            })
            .1
        );
        assert!(
            run(&PostIn {
                last_hurt: 947,
                rate: 128,
                ..rest()
            })
            .1
        );
        assert!(
            run(&PostIn {
                last_hurt: 973,
                rate: 30,
                ..rest()
            })
            .1
        );
        assert_eq!(
            run(&PostIn {
                last_hurt: 973,
                ..rest()
            })
            .0[8],
            973.0
        );
    }

    #[test]
    fn rows_match_single() {
        let cfg = Config::default();
        let ps = [
            rest(),
            PostIn {
                vel_x: 0.5,
                ..rest()
            },
            PostIn {
                last_hurt: 990,
                ..rest()
            },
        ];
        let mut input = Vec::new();
        for p in &ps {
            let mut r = [0.0; IN];
            r[IN_VEL_X] = p.vel_x as f64;
            r[IN_VEL_Z] = p.vel_z as f64;
            r[IN_MODE] = p.mode as f64;
            r[IN_CROUCH] = p.crouch_fraction;
            r[IN_SLIDE] = p.slide_time;
            r[IN_WALL] = p.wall_run_time;
            r[IN_SPRINT] = flag(p.sprinting);
            r[IN_TICK] = p.tick as f64;
            r[IN_LAST_HURT] = p.last_hurt as f64;
            r[IN_RATE] = p.rate as f64;
            input.extend_from_slice(&r);
        }
        let mut out = vec![0.0; ps.len() * OUT];
        assert_eq!(run_rows(&input, &cfg, &mut out), 3);
        for (i, p) in ps.iter().enumerate() {
            let (row, still) = run(p);
            assert_eq!(&out[i * OUT..i * OUT + POSTURE], &row);
            assert_eq!(out[i * OUT + OUT_STILL], flag(still));
        }
    }
}
