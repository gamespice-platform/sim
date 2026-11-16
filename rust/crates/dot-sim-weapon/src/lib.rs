//! A hitscan weapon's spread state tick: dot-weapon's `DotWeaponHitscan._tick` (bloom shedding, motion settling,
//! the `DotWeaponSpreadRules` factor and the cone of `_spread_at`), spelled in the script's order and in `f64`, so
//! it agrees with the script to the bit. The server runs it for every idle player every tick, in that player's turn
//! (a later player's hit changes an earlier one's flinch within the tick, so it is never batched across players).
//!
//! The script's steady skip (`_steady_holds`) is a memo of this same arithmetic: skipping it changes no state, so
//! it has no twin here.

use dot_sim_math::{clampf, maxf, minf};

/// `DotWeaponHitscan._NEVER`: the ticks-since-use count before any use, and its ceiling.
pub const NEVER: i64 = 1 << 20;
/// `DotWeaponAccuracyClass.FLINCH_SEC`.
pub const FLINCH_SEC: f64 = 0.4;

/// The script's `lerpf` (`Math::lerp`): `from + (to - from) * weight`.
#[inline]
pub fn lerpf(from: f64, to: f64, weight: f64) -> f64 {
    from + (to - from) * weight
}

/// One weapon's numbers, as the tick reads them. The accuracy terms are already resolved (the ballistics field when
/// 0 or more, else the class preset), and the tick counts already scaled to the running rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tuning {
    pub spread: f64,
    pub spread_moving: f64,
    pub spread_speed_ref: f64,
    pub spread_airborne: f64,
    pub spread_crouched: f64,
    /// `b.bloom`: shedding happens only when above 0.
    pub bloom: f64,
    pub bloom_max: f64,
    /// `scaled_per_tick(b.bloom_recovery)`.
    pub bloom_recovery: f64,
    /// `_bloom_delay_of(b)`.
    pub bloom_delay: i64,
    /// `_settle_ticks_of(b)`.
    pub settle: i64,
    pub hip: f64,
    pub slide: f64,
    pub sprint: f64,
    pub flinch: f64,
    pub crouch: f64,
    pub aim: f64,
}

/// Number of values in a packed [`Tuning`] row (see [`Tuning::from_row`]).
pub const TUNING_LEN: usize = 16;

impl Tuning {
    /// `[spread, spread_moving, spread_speed_ref, spread_airborne, spread_crouched, bloom, bloom_max,
    /// bloom_recovery, bloom_delay, settle, hip, slide, sprint, flinch, crouch, aim]`; `None` when short.
    pub fn from_row(r: &[f64]) -> Option<Self> {
        if r.len() < TUNING_LEN {
            return None;
        }
        Some(Self {
            spread: r[0],
            spread_moving: r[1],
            spread_speed_ref: r[2],
            spread_airborne: r[3],
            spread_crouched: r[4],
            bloom: r[5],
            bloom_max: r[6],
            bloom_recovery: r[7],
            bloom_delay: r[8] as i64,
            settle: r[9] as i64,
            hip: r[10],
            slide: r[11],
            sprint: r[12],
            flinch: r[13],
            crouch: r[14],
            aim: r[15],
        })
    }

    /// `DotWeaponBallistics.spread_for`: the cone half-angle for a carrier in this posture, before bloom.
    pub fn spread_for(&self, speed_now: f64, airborne: bool, crouched: bool) -> f64 {
        if airborne {
            return maxf(self.spread_airborne, maxf(self.spread, self.spread_moving));
        }
        let moving = clampf(speed_now / maxf(0.001, self.spread_speed_ref), 0.0, 1.0);
        let base = lerpf(self.spread, maxf(self.spread, self.spread_moving), moving);
        if crouched {
            base * minf(self.spread_crouched, 1.0)
        } else {
            base
        }
    }

    /// `DotWeaponBallistics.spread_for_motion`.
    pub fn spread_for_motion(&self, motion: f64, airborne: bool, crouched: bool) -> f64 {
        self.spread_for(
            clampf(motion, 0.0, 1.0) * self.spread_speed_ref,
            airborne,
            crouched,
        )
    }

    /// `DotWeaponHitscan._motion_of`.
    pub fn motion_of(&self, speed: f64) -> f64 {
        clampf(speed / maxf(0.001, self.spread_speed_ref), 0.0, 1.0)
    }
}

/// The server's `DotWeaponSpreadRules` (the `sv_spread_*` cvars), with the recovery already per running tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rules {
    pub enabled: bool,
    pub scale: f64,
    pub mult_hip: f64,
    pub mult_aim: f64,
    pub mult_stand: f64,
    pub mult_walk: f64,
    pub mult_run: f64,
    pub mult_air: f64,
    pub mult_sprint: f64,
    pub mult_slide: f64,
    pub mult_crouch: f64,
    pub walk_speed: f64,
    pub run_speed: f64,
    /// `recovery_per_tick(running_tick_rate())`.
    pub recovery_per_tick: f64,
    pub max_spread: f64,
}

/// Number of values in a packed [`Rules`] row (see [`Rules::from_row`]).
pub const RULES_LEN: usize = 15;

impl Rules {
    /// `[enabled, scale, mult_hip, mult_aim, mult_stand, mult_walk, mult_run, mult_air, mult_sprint, mult_slide,
    /// mult_crouch, walk_speed, run_speed, recovery_per_tick, max_spread]`; `None` when short.
    pub fn from_row(r: &[f64]) -> Option<Self> {
        if r.len() < RULES_LEN {
            return None;
        }
        Some(Self {
            enabled: r[0] != 0.0,
            scale: r[1],
            mult_hip: r[2],
            mult_aim: r[3],
            mult_stand: r[4],
            mult_walk: r[5],
            mult_run: r[6],
            mult_air: r[7],
            mult_sprint: r[8],
            mult_slide: r[9],
            mult_crouch: r[10],
            walk_speed: r[11],
            run_speed: r[12],
            recovery_per_tick: r[13],
            max_spread: r[14],
        })
    }

    /// `DotWeaponSpreadRules.target_factor`.
    pub fn target_factor(&self, p: &Posture) -> f64 {
        if !self.enabled {
            return 1.0;
        }
        let mut f = if p.airborne {
            self.mult_air
        } else {
            self.mult_stand
        };
        if p.sliding {
            f = self.mult_slide;
        } else if p.sprinting && !p.airborne {
            f = self.mult_sprint;
        } else if !p.airborne {
            if p.speed >= self.run_speed {
                f = self.mult_run;
            } else if p.speed >= self.walk_speed {
                f = self.mult_walk;
            }
        }
        if p.crouched {
            f *= self.mult_crouch;
        }
        f * (if p.aiming {
            self.mult_aim
        } else {
            self.mult_hip
        })
    }

    /// `DotWeaponSpreadRules.apply`.
    pub fn apply(&self, base: f64, factor: f64) -> f64 {
        let out = base * factor;
        if self.max_spread > 0.0 {
            minf(out, self.max_spread)
        } else {
            out
        }
    }
}

/// The `DotWeaponContext` fields the tick reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Posture {
    pub speed: f64,
    pub airborne: bool,
    pub crouched: bool,
    pub aiming: bool,
    pub sliding: bool,
    pub sprinting: bool,
    pub wall_running: bool,
    pub tick: i64,
    pub hurt_tick: i64,
}

/// Number of values in a packed [`Posture`] row (see [`Posture::from_row`]).
pub const POSTURE_LEN: usize = 9;

impl Posture {
    /// `[speed, airborne, crouched, aiming, sliding, sprinting, wall_running, tick, hurt_tick]`; `None` when short.
    pub fn from_row(r: &[f64]) -> Option<Self> {
        if r.len() < POSTURE_LEN {
            return None;
        }
        Some(Self {
            speed: r[0],
            airborne: r[1] != 0.0,
            crouched: r[2] != 0.0,
            aiming: r[3] != 0.0,
            sliding: r[4] != 0.0,
            sprinting: r[5] != 0.0,
            wall_running: r[6] != 0.0,
            tick: r[7] as i64,
            hurt_tick: r[8] as i64,
        })
    }
}

/// What the tick reads besides the weapon and the posture: the arsenal's rules object (enabled or not), whether
/// it tracks a crosshair cone, and the running tick rate (0 when the arsenal has none).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub rules: Option<Rules>,
    pub cone: bool,
    pub rate: i64,
}

impl Frame {
    /// `_tick`'s `cone`: a rules object and the arsenal's `crosshair_cone`.
    pub fn tracks_cone(&self) -> bool {
        self.rules.is_some() && self.cone
    }

    /// The flinch term's tick rate: `float(maxi(running, 1)) if running > 0 else 64.0`.
    fn flinch_rate(&self) -> f64 {
        if self.rate > 0 {
            self.rate.max(1) as f64
        } else {
            64.0
        }
    }
}

/// The behaviour's `state` keys the tick reads and writes; `None` is a key the dictionary does not hold.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub since: Option<i64>,
    pub bloom: Option<f64>,
    pub motion: Option<f64>,
    pub factor: Option<f64>,
    pub now: Option<f64>,
}

/// Keys a tick wrote (bits of [`Ticked::wrote`]). [`WROTE_MOTION`] also covers its erase.
pub const WROTE_SINCE: u8 = 1;
pub const WROTE_BLOOM: u8 = 2;
pub const WROTE_MOTION: u8 = 4;
pub const WROTE_FACTOR: u8 = 8;
pub const WROTE_NOW: u8 = 16;

/// One tick's result: the cone it computed (the behaviour's `_shown`; `None` for a tick with no context, which
/// leaves it) and the keys of the state it wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ticked {
    pub shown: Option<f64>,
    pub wrote: u8,
}

/// `DotWeaponHitscan._spread_at`: the cone for this posture at the given motion, factor and bloom.
pub fn spread_at(
    t: &Tuning,
    p: &Posture,
    frame: &Frame,
    motion: f64,
    factor: f64,
    bloom_state: f64,
) -> f64 {
    let mut out = t.spread_for_motion(motion, p.airborne, p.crouched);
    if !p.aiming {
        out += t.hip;
    }
    if p.sliding || p.wall_running {
        out *= t.slide;
    } else if p.sprinting && !p.airborne {
        out *= t.sprint;
    }
    if p.hurt_tick >= 0 && p.tick >= p.hurt_tick {
        let left = 1.0 - (p.tick - p.hurt_tick) as f64 / (FLINCH_SEC * frame.flinch_rate());
        out += t.flinch * clampf(left, 0.0, 1.0);
    }
    if let Some(r) = frame.rules.as_ref().filter(|r| r.enabled) {
        out = r.apply(out, factor);
    }
    let mut bloom = minf(bloom_state, t.bloom_max);
    if p.crouched {
        bloom *= t.crouch;
    }
    out += bloom;
    if p.aiming {
        out *= t.aim;
    }
    if let Some(every) = frame.rules.as_ref() {
        out *= every.scale;
        if every.max_spread > 0.0 {
            out = minf(out, every.max_spread);
        }
    }
    out
}

/// `DotWeaponHitscan._spread_with` (and `_spread_for` under the frame's rules): the state's motion, factor and bloom.
pub fn spread_with(t: &Tuning, p: &Posture, frame: &Frame, s: &State) -> f64 {
    let motion = maxf(s.motion.unwrap_or(0.0), t.motion_of(p.speed));
    spread_at(
        t,
        p,
        frame,
        motion,
        s.factor.unwrap_or(1.0),
        s.bloom.unwrap_or(0.0),
    )
}

/// `DotWeaponHitscan._tick` with a context ([`None`] for the posture is a tick without one), on a behaviour with a
/// ballistics tuning. Writes the state as the script does and returns the cone it computed.
pub fn tick(t: &Tuning, posture: Option<&Posture>, frame: &Frame, s: &mut State) -> Ticked {
    let since = (s.since.unwrap_or(NEVER) + 1).min(NEVER);
    s.since = Some(since);
    let mut wrote = WROTE_SINCE;
    let mut bloom = s.bloom.unwrap_or(0.0);
    if t.bloom > 0.0 && bloom > 0.0 && since >= t.bloom_delay {
        bloom = maxf(0.0, bloom - t.bloom_recovery);
        s.bloom = Some(bloom);
        wrote |= WROTE_BLOOM;
    }

    let Some(p) = posture else {
        return Ticked { shown: None, wrote };
    };

    let target_motion = t.motion_of(p.speed);
    let held_motion = s.motion.unwrap_or(0.0);
    let mut motion = target_motion;
    if target_motion < held_motion && t.settle > 0 {
        motion = maxf(target_motion, held_motion - 1.0 / t.settle as f64);
    }
    s.motion = if motion > 0.0 { Some(motion) } else { None };
    wrote |= WROTE_MOTION;

    let mut factor = 1.0;
    if let Some(r) = frame.rules.as_ref().filter(|r| r.enabled) {
        let target = r.target_factor(p);
        let held = s.factor.unwrap_or(1.0);
        factor = if target >= held {
            target
        } else {
            maxf(target, held - r.recovery_per_tick)
        };
        s.factor = Some(factor);
        wrote |= WROTE_FACTOR;
    }

    let shown = spread_at(t, p, frame, motion, factor, bloom);
    if frame.tracks_cone() || held_motion > 0.0 || motion > 0.0 {
        s.now = Some(shown);
        wrote |= WROTE_NOW;
    }
    Ticked {
        shown: Some(shown),
        wrote,
    }
}

#[cfg(test)]
mod tests;
