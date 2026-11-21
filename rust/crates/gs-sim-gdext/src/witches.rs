//! `GSSimWitches`: mg-look-at-me's `LmGame._watch` witch half (`_listen` over every witch, then
//! `exposure` over every witch) for one player in one call, with the script's rays cast from here.
//!
//! The game registers each level's witch loops and door bodies once (`clear_level`, `add_witch`,
//! `set_doors`), then per player per tick packs the gaze it keeps, calls [`GSSimWitches::watch`]
//! and unpacks the answer: the same states, rates and `listened` as the script to the bit
//! (`gs_sim_witches`; the game's `headless_native_witches` and its `witch_verify` hold the two
//! together). The script keeps the gaze, the meter and the signals.

use std::collections::HashMap;

use gs_sim_math::Vec3;
use gs_sim_witches::{self as witches, Config, Gaze, Level, Loop, Player, Watched};
use godot::classes::{PhysicsDirectSpaceState3D, PhysicsRayQueryParameters3D};
use godot::prelude::*;

/// Floats in `watch`'s `config`.
const CONFIG_F: usize = 25;
/// Floats in `watch`'s `player`.
const PLAYER_F: usize = 6;
/// Floats per witch in `watch`'s `states` and at the head of each witch's answer.
const STATE_F: usize = 17;
/// Floats per witch in `watch`'s answer: the state, the rate, listened.
const OUT_F: usize = STATE_F + 2;
/// The world layer the script's rays hit (`LAYER_WORLD`).
const LAYER_WORLD: u32 = 1;

#[inline]
fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[inline]
fn gv(v: Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

#[derive(Default)]
struct Witches {
    loops: Vec<Loop>,
    doors: Vec<Rid>,
}

/// Every registered level's witches and the per-player pass over them.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimWitches {
    levels: HashMap<i64, Witches>,
    ray: Gd<PhysicsRayQueryParameters3D>,
    states: Vec<Option<Gaze>>,
    out: Vec<Watched>,
}

#[godot_api]
impl IRefCounted for GSSimWitches {
    fn init(_base: Base<RefCounted>) -> Self {
        let mut ray = PhysicsRayQueryParameters3D::new_gd();
        ray.set_collision_mask(LAYER_WORLD);
        Self {
            levels: HashMap::new(),
            ray,
            states: Vec::new(),
            out: Vec::new(),
        }
    }
}

/// The rays one `watch` casts: the player's exclusions plus the open door a witch stands in.
struct Caster<'a> {
    space: Option<Gd<PhysicsDirectSpaceState3D>>,
    ray: &'a mut Gd<PhysicsRayQueryParameters3D>,
    base: &'a Array<Rid>,
    doors: &'a [Rid],
    /// The open door the ray's exclusions were last set for (`None`: not set this call).
    set_for: Option<i32>,
    casts: i64,
}

impl witches::Rays for Caster<'_> {
    fn clear(&mut self, from: Vec3, to: Vec3, open_door: i32) -> bool {
        let Some(space) = self.space.as_mut() else {
            return true;
        };
        if self.set_for != Some(open_door) {
            self.set_for = Some(open_door);
            let door = usize::try_from(open_door)
                .ok()
                .and_then(|d| self.doors.get(d))
                .filter(|r| r.is_valid());
            match door {
                Some(rid) => {
                    let mut ex = self.base.duplicate_shallow();
                    ex.push(*rid);
                    self.ray.set_exclude(&ex);
                }
                None => self.ray.set_exclude(self.base),
            }
        }
        self.casts += 1;
        self.ray.set_from(gv(from));
        self.ray.set_to(gv(to));
        space.intersect_ray(&*self.ray).is_empty()
    }
}

#[godot_api]
impl GSSimWitches {
    /// Forgets level [param key]'s witches and doors (it then has none).
    #[func]
    fn clear_level(&mut self, key: i64) {
        self.levels.insert(key, Witches::default());
    }

    /// Forgets level [param key] altogether (a level freed).
    #[func]
    fn remove_level(&mut self, key: i64) {
        self.levels.remove(&key);
    }

    /// Whether level [param key] is registered.
    #[func]
    fn has_level(&self, key: i64) -> bool {
        self.levels.contains_key(&key)
    }

    /// Adds a witch to level [param key], as `LmLevel._witch_loop` built her: her points, leg
    /// lengths, the door slot at each point (-1 none; a slot of [method set_doors]), total, speed,
    /// sight and cone.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn add_witch(
        &mut self,
        key: i64,
        points: PackedVector3Array,
        lengths: PackedFloat32Array,
        doors: PackedInt32Array,
        total: f64,
        speed: f64,
        sight: f64,
        cone: f64,
    ) {
        self.levels.entry(key).or_default().loops.push(Loop {
            points: points.as_slice().iter().map(|p| v3(*p)).collect(),
            lengths: lengths.as_slice().to_vec(),
            doors: doors.as_slice().to_vec(),
            total,
            speed,
            sight,
            cone,
        });
    }

    /// Level [param key]'s door bodies by slot (`LmLevel.door_rid`; an invalid RID excludes nothing).
    #[func]
    fn set_doors(&mut self, key: i64, rids: Array<Rid>) {
        self.levels.entry(key).or_default().doors = rids.iter_shared().collect();
    }

    /// How many witches level [param key] has.
    #[func]
    fn witch_count(&self, key: i64) -> i32 {
        self.levels.get(&key).map_or(0, |w| w.loops.len() as i32)
    }

    /// One player's tick over level [param key]'s witches at [param origin]. [param config] is 25
    /// floats (`LmConfig` in `gs_sim_witches::Config`'s order, bools 0/1), [param player] 6 (noise
    /// radius, horizontal speed, caught_left, flashlight, crouching, level number), [param states]
    /// 17 per witch (present, head, notice, alert, hold, at xyz, mode, t, heard, stall, cool, pos xyz,
    /// face; absent ones start fresh). [param space] null sees through everything; [param exclude]
    /// is the player's body and the doors they opened. Answers 19 floats per witch (the 17 state
    /// floats, her exposure rate, 1 when she began to listen), then the rays cast; empty on bad input.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn watch(
        &mut self,
        key: i64,
        origin: Vector3,
        config: PackedFloat64Array,
        player: PackedFloat64Array,
        target: Vector3,
        states: PackedFloat64Array,
        seconds: f64,
        delta: f64,
        space: Option<Gd<PhysicsDirectSpaceState3D>>,
        exclude: Array<Rid>,
    ) -> PackedFloat64Array {
        let Some(level) = self.levels.get(&key) else {
            godot_error!("GSSimWitches.watch: level {key} is not registered");
            return PackedFloat64Array::new();
        };
        let c = config.as_slice();
        let pl = player.as_slice();
        if c.len() != CONFIG_F || pl.len() != PLAYER_F {
            godot_error!(
                "GSSimWitches.watch: config has {} floats (want {CONFIG_F}), player {} (want {PLAYER_F})",
                c.len(),
                pl.len()
            );
            return PackedFloat64Array::new();
        }
        let cfg = Config {
            speed_scale: c[0],
            head_sweep: c[1] != 0.0,
            door_seconds: c[2],
            sight_scale: c[3],
            flashlight_sight: c[4],
            cone_scale: c[5],
            exposure_rate: c[6],
            opening_ease: c[7],
            opening_levels: c[8] as i64,
            flashlight_exposure: c[9],
            crouch_exposure: c[10],
            still_exposure: c[11],
            listen_hearing: c[12],
            light_range: c[13],
            hear_walls: c[14],
            reaction_seconds: c[15],
            alert_seconds: c[16],
            listens: c[17] != 0.0,
            listen_seconds: c[18],
            listen_confirm: c[19],
            listen_cooldown: c[20],
            turn_delay: c[21],
            chase_seconds: c[22],
            turn_rate: c[23],
            chase_speed: c[24],
        };
        let p = Player {
            noise: pl[0],
            hspeed: pl[1],
            caught_left: pl[2],
            flashlight: pl[3] != 0.0,
            crouching: pl[4] != 0.0,
            level: pl[5] as i64,
            target: v3(target),
        };
        self.states.clear();
        self.states
            .extend(states.as_slice().as_chunks::<STATE_F>().0.iter().map(|s| {
                (s[0] != 0.0).then(|| Gaze {
                    head: s[1],
                    notice: s[2],
                    alert: s[3] != 0.0,
                    hold: s[4],
                    at: Vec3::new(s[5] as f32, s[6] as f32, s[7] as f32),
                    mode: s[8] as i32,
                    t: s[9],
                    heard: s[10],
                    stall: s[11],
                    cool: s[12],
                    pos: Vec3::new(s[13] as f32, s[14] as f32, s[15] as f32),
                    face: s[16],
                })
            }));
        let lv = Level {
            origin: v3(origin),
            loops: &level.loops,
        };
        let mut caster = Caster {
            space,
            ray: &mut self.ray,
            base: &exclude,
            doors: &level.doors,
            set_for: None,
            casts: 0,
        };
        witches::watch(
            &cfg,
            &lv,
            &p,
            &self.states,
            seconds,
            delta,
            &mut caster,
            &mut self.out,
        );
        let casts = caster.casts;
        let mut out = Vec::with_capacity(self.out.len() * OUT_F + 1);
        for w in &self.out {
            let s = &w.state;
            out.extend_from_slice(&[
                1.0,
                s.head,
                s.notice,
                f64::from(u8::from(s.alert)),
                s.hold,
                f64::from(s.at.x),
                f64::from(s.at.y),
                f64::from(s.at.z),
                f64::from(s.mode),
                s.t,
                s.heard,
                s.stall,
                s.cool,
                f64::from(s.pos.x),
                f64::from(s.pos.y),
                f64::from(s.pos.z),
                s.face,
                w.rate,
                f64::from(u8::from(w.listened)),
            ]);
        }
        out.push(casts as f64);
        PackedFloat64Array::from(out.as_slice())
    }
}
