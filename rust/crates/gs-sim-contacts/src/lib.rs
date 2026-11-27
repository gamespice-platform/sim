//! A bot world's contact list (gs-bot's `GSBotContact` fields and the sight rows a native world
//! reads) from one packed record per player. The same values as the script to the bit: positions
//! are the f64 state columns rounded to f32 (`Vector3(_f[0], _f[1], _f[2])`), flags are f64
//! comparisons, the health ratio an f64 division, and a lifted point is Godot's
//! `at + Vector3.UP * h` (the height rounded to f32, multiplied by each axis of UP, then added).
//!
//! A record is [`STRIDE`] floats: [`HEAD`] head values (flags, team, pitch, health, max health, a
//! spare), then the player's `GSFpsState` columns ([`COLUMNS`] floats). A player without a
//! controller has only its position in the first three columns.
//!
//! [`world`] reads the think tiers' rows and the near grid's cells off the filled table.

pub mod world;

/// Head values before the state columns.
pub const HEAD: usize = 6;
/// `GSFpsStateColumns.STRIDE` (gs-player-controller 43: 30 motor columns, then the crouch fraction
/// and held flag, which a contact does not read).
pub const COLUMNS: usize = 32;
/// One player's record.
pub const STRIDE: usize = HEAD + COLUMNS;

/// Head flag: the player has a controller (its state columns follow).
pub const CONTROLLER: u32 = 1;
/// Head flag: `p.is_alive()`.
pub const ALIVE: u32 = 2;
/// Head flag: the player has a health component (health and max health are set).
pub const HEALTH: u32 = 4;

const FLAGS: usize = 0;
const TEAM: usize = 1;
const PITCH: usize = 2;
const HEALTH_NOW: usize = 3;
const HEALTH_MAX: usize = 4;

const POSITION: usize = 0;
const VELOCITY: usize = 3;
const MODE: usize = 9;
const YAW: usize = 12;
const SLIDE_TIME: usize = 17;
const LAUNCH_COOLDOWN_LEFT: usize = 19;
const WALL_RUN_TIME: usize = 22;
/// `GSFpsState.Mode.GROUND`.
const GROUND: f64 = 0.0;

/// A contact's per-tick fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub view_yaw: f64,
    pub view_pitch: f64,
    pub grounded: bool,
    pub sliding: bool,
    pub wall_running: bool,
    pub air_jump_ready: bool,
    pub health: f64,
    pub alive: bool,
    pub team: i64,
}

fn v3(r: &[f64], at: usize) -> [f32; 3] {
    [r[at] as f32, r[at + 1] as f32, r[at + 2] as f32]
}

/// The contact for one [`STRIDE`] record; [param launch_enabled] is the body tunables'.
pub fn contact(r: &[f64], launch_enabled: bool) -> Contact {
    let flags = r[FLAGS] as u32;
    let s = &r[HEAD..HEAD + COLUMNS];
    let controlled = flags & CONTROLLER != 0;
    let health = if flags & HEALTH != 0 && r[HEALTH_MAX] > 0.0 {
        r[HEALTH_NOW] / r[HEALTH_MAX]
    } else {
        1.0
    };
    let mut c = Contact {
        position: v3(s, POSITION),
        velocity: [0.0; 3],
        view_yaw: f64::NAN,
        view_pitch: f64::NAN,
        grounded: true,
        sliding: false,
        wall_running: false,
        air_jump_ready: false,
        health,
        alive: flags & ALIVE != 0,
        team: r[TEAM] as i64,
    };
    if controlled {
        c.velocity = v3(s, VELOCITY);
        c.view_yaw = s[YAW];
        c.view_pitch = r[PITCH];
        // `int(_f[9]) == GROUND`: truncation; NaN and out-of-range values are never GROUND.
        c.grounded = s[MODE].trunc() == GROUND;
        c.sliding = s[SLIDE_TIME] >= 0.0;
        c.wall_running = s[WALL_RUN_TIME] >= 0.0;
        c.air_jump_ready = launch_enabled && s[LAUNCH_COOLDOWN_LEFT] <= 0.0;
    }
    c
}

/// The contact fields a player keeps from tick to tick until its state changes (eye and aim
/// heights, alive, team, the four motion flags and the health ratio), as bits, in the order a
/// writer sets them. [`Stable::changed`] says which to write again; position, velocity and view
/// move nearly every tick and are always written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stable([u64; STABLE]);

/// Fields in a [`Stable`].
pub const STABLE: usize = 9;

impl Stable {
    /// [param c]'s kept fields with the world's [param eye] and [param aim] heights: eye, aim,
    /// alive, team, grounded, sliding, wall running, air jump ready, health.
    pub fn of(c: &Contact, eye: f64, aim: f64) -> Self {
        Self([
            eye.to_bits(),
            aim.to_bits(),
            u64::from(c.alive),
            c.team as u64,
            u64::from(c.grounded),
            u64::from(c.sliding),
            u64::from(c.wall_running),
            u64::from(c.air_jump_ready),
            c.health.to_bits(),
        ])
    }

    /// A bit per field (in [`Stable::of`]'s order) that differs from [param before], to the bit
    /// (a NaN that stays the same NaN is unchanged; -0.0 after 0.0 is a change). Every bit
    /// without a [param before].
    pub fn changed(&self, before: Option<&Stable>) -> u16 {
        let Some(b) = before else {
            return (1 << STABLE) - 1;
        };
        let mut mask = 0;
        for k in 0..STABLE {
            if self.0[k] != b.0[k] {
                mask |= 1 << k;
            }
        }
        mask
    }
}

/// A contact table row's flag bits (gs-bot's `GSBotContactTable`): alive, grounded, sliding,
/// wall running and air jump ready.
pub const ROW_ALIVE: i32 = 1;
pub const ROW_GROUNDED: i32 = 2;
pub const ROW_SLIDING: i32 = 4;
pub const ROW_WALL_RUNNING: i32 = 8;
pub const ROW_AIR_JUMP: i32 = 16;
/// Floats per contact table row: eye height, aim height, view yaw, view pitch, health.
pub const ROW_FLOATS: usize = 5;

/// The flag bits of a contact table row for `c`.
pub fn row_flags(c: &Contact) -> i32 {
    (if c.alive { ROW_ALIVE } else { 0 })
        | (if c.grounded { ROW_GROUNDED } else { 0 })
        | (if c.sliding { ROW_SLIDING } else { 0 })
        | (if c.wall_running { ROW_WALL_RUNNING } else { 0 })
        | (if c.air_jump_ready { ROW_AIR_JUMP } else { 0 })
}

/// The floats of a contact table row for `c`, [`ROW_FLOATS`] of them.
pub fn row_floats(c: &Contact, eye: f64, aim: f64) -> [f64; ROW_FLOATS] {
    [eye, aim, c.view_yaw, c.view_pitch, c.health]
}

/// Godot's `at + Vector3.UP * h`.
pub fn lift(at: [f32; 3], h: f64) -> [f32; 3] {
    let h = h as f32;
    [at[0] + 0.0 * h, at[1] + h, at[2] + 0.0 * h]
}

/// The weapon in hand of a contact for gs-bot's play styles: its interned style class (an index
/// into the caller's class names; [`NO_CLASS`] for no weapon) and the slot's rounds and the
/// definition's magazine size, as `GSWeaponSlot.magazine` and `GSWeaponDef.magazine` hold them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arms {
    pub class: i32,
    pub rounds: i64,
    pub size: i64,
}

/// [`Arms::class`] of a contact with no weapon (`weapon_class` `&""`, magazine -1).
pub const NO_CLASS: i32 = -1;

impl Arms {
    /// How full the magazine is, as the script's
    /// `float(slot.magazine) / float(def.magazine) if def.magazine > 0 else -1.0`
    /// (an f64 division of the two ints), and -1 with no weapon.
    pub fn magazine(&self) -> f64 {
        if self.class < 0 || self.size <= 0 {
            return -1.0;
        }
        self.rounds as f64 / self.size as f64
    }

    /// The written pair: the class (negative is no weapon) and the magazine ratio's bits, so two
    /// writes compare to the bit.
    pub fn written(&self) -> (i32, u64) {
        (self.class.max(NO_CLASS), self.magazine().to_bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(flags: u32) -> Vec<f64> {
        let mut r = vec![0.0; STRIDE];
        r[FLAGS] = flags as f64;
        r[TEAM] = 2.0;
        r[PITCH] = -0.25;
        r[HEALTH_NOW] = 30.0;
        r[HEALTH_MAX] = 120.0;
        for (i, v) in r[HEAD..].iter_mut().enumerate() {
            *v = 0.1 * i as f64 + 0.123_456_789;
        }
        r
    }

    #[test]
    fn stable_changes_to_the_bit() {
        let c = contact(&record(CONTROLLER | ALIVE | HEALTH), true);
        let a = Stable::of(&c, 1.6, 1.2);
        assert_eq!(a.changed(None), 0b1_1111_1111);
        assert_eq!(a.changed(Some(&a)), 0);
        let mut d = c;
        d.health = -0.0;
        let mut e = c;
        e.health = 0.0;
        assert_eq!(
            Stable::of(&d, 1.6, 1.2).changed(Some(&Stable::of(&e, 1.6, 1.2))),
            1 << 8
        );
        d.health = f64::NAN;
        let n = Stable::of(&d, 1.6, 1.2);
        assert_eq!(n.changed(Some(&n)), 0);
        let mut f = c;
        f.alive = !c.alive;
        f.sliding = !c.sliding;
        assert_eq!(Stable::of(&f, 1.6, 1.2).changed(Some(&a)), 1 << 2 | 1 << 5);
        assert_eq!(Stable::of(&c, 1.7, 1.2).changed(Some(&a)), 1);
        f = c;
        f.team = 3;
        f.position = [9.0; 3];
        f.view_yaw = 4.0;
        assert_eq!(Stable::of(&f, 1.6, 1.2).changed(Some(&a)), 1 << 3);
    }

    #[test]
    fn controlled_reads_the_columns() {
        let mut r = record(CONTROLLER | ALIVE | HEALTH);
        r[HEAD + MODE] = 0.0;
        r[HEAD + SLIDE_TIME] = -1.0;
        r[HEAD + WALL_RUN_TIME] = 0.0;
        r[HEAD + LAUNCH_COOLDOWN_LEFT] = 0.0;
        let c = contact(&r, true);
        assert_eq!(
            c.position,
            [r[HEAD] as f32, r[HEAD + 1] as f32, r[HEAD + 2] as f32]
        );
        assert_eq!(c.velocity[0], r[HEAD + 3] as f32);
        assert_eq!(c.view_yaw, r[HEAD + YAW]);
        assert_eq!(c.view_pitch, -0.25);
        assert!(c.grounded && !c.sliding && c.wall_running && c.air_jump_ready && c.alive);
        assert_eq!(c.health, 0.25);
        assert_eq!(c.team, 2);
        assert!(!contact(&r, false).air_jump_ready);
        r[HEAD + MODE] = 1.0;
        assert!(!contact(&r, true).grounded);
        r[HEAD + MODE] = f64::NAN;
        assert!(!contact(&r, true).grounded);
        r[HEAD + MODE] = 0.75;
        assert!(contact(&r, true).grounded);
    }

    #[test]
    fn uncontrolled_and_no_health() {
        let mut r = record(0);
        r[HEALTH_MAX] = 0.0;
        let c = contact(&r, true);
        assert_eq!(c.velocity, [0.0; 3]);
        assert!(c.view_yaw.is_nan() && c.view_pitch.is_nan());
        assert!(c.grounded && !c.sliding && !c.wall_running && !c.air_jump_ready && !c.alive);
        assert_eq!(c.health, 1.0);
        let r = record(HEALTH);
        assert_eq!(contact(&r, true).health, 0.25);
        let mut r = record(HEALTH);
        r[HEALTH_MAX] = 0.0;
        assert_eq!(contact(&r, true).health, 1.0);
    }

    #[test]
    fn row_flags_and_floats() {
        let c = contact(&record(CONTROLLER | ALIVE | HEALTH), true);
        let f = row_flags(&c);
        assert_eq!(f & ROW_ALIVE != 0, c.alive);
        assert_eq!(f & ROW_GROUNDED != 0, c.grounded);
        assert_eq!(f & ROW_SLIDING != 0, c.sliding);
        assert_eq!(f & ROW_WALL_RUNNING != 0, c.wall_running);
        assert_eq!(f & ROW_AIR_JUMP != 0, c.air_jump_ready);
        let dead = contact(&record(0), true);
        assert_eq!(row_flags(&dead) & ROW_ALIVE, 0);
        let r = row_floats(&c, 1.6, 1.2);
        assert_eq!(r, [1.6, 1.2, c.view_yaw, c.view_pitch, c.health]);
    }

    #[test]
    fn lift_is_godots_up_times() {
        assert_eq!(lift([1.0, 2.0, 3.0], 1.5), [1.0, 3.5, 3.0]);
        // -0.0 + 0.0 * h is +0.0, as Godot's add.
        let l = lift([-0.0, 0.0, -0.0], 1.25);
        assert!(l[0].is_sign_positive() && l[2].is_sign_positive());
        // A negative height keeps -0.0: -0.0 + 0.0 * -1 is -0.0.
        let l = lift([-0.0, 0.0, -0.0], -1.0);
        assert!(l[0].is_sign_negative());
        assert_eq!(lift([0.0; 3], 1.0 / 3.0)[1], (1.0f64 / 3.0) as f32);
    }

    #[test]
    fn arms_magazine_is_the_scripts_ratio() {
        let a = Arms {
            class: 2,
            rounds: 7,
            size: 30,
        };
        assert_eq!(a.magazine().to_bits(), (7.0f64 / 30.0).to_bits());
        assert_eq!(
            Arms {
                class: 0,
                rounds: 0,
                size: 12
            }
            .magazine(),
            0.0
        );
        // No magazine (a knife) and no weapon are -1.
        assert_eq!(
            Arms {
                class: 1,
                rounds: 0,
                size: 0
            }
            .magazine(),
            -1.0
        );
        assert_eq!(
            Arms {
                class: NO_CLASS,
                rounds: 5,
                size: 10
            }
            .magazine(),
            -1.0
        );
        assert_eq!(
            Arms {
                class: -7,
                rounds: 5,
                size: 10
            }
            .written(),
            (NO_CLASS, (-1.0f64).to_bits())
        );
        let b = Arms {
            class: 2,
            rounds: 8,
            size: 30,
        };
        assert_ne!(a.written(), b.written());
        assert_eq!(a.written(), Arms { ..a }.written());
    }
}
