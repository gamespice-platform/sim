//! `GSSimWeaponSpread`: gs-weapon's `GSWeaponHitscan._tick` and `_spread_with` (with `GSWeaponSpreadRules` and
//! `GSWeaponBallistics.spread_for`), bit for bit the script's, reading and writing the behaviour's `state`
//! dictionary in place. The caller keeps one per weapon behaviour: the weapon's numbers are set once
//! ([method set_tuning]) and again only when they change, so a tick marshals the posture row and nothing else.

use godot::prelude::*;
use gs_sim_math::Vec3;
use gs_sim_weapon::shot::{Recoil, RecoilState, Shot, offset_direction, recoil, scatter};
use gs_sim_weapon::{
    Frame, Posture, Rules, State, Tuning, WROTE_BLOOM, WROTE_FACTOR, WROTE_MOTION, WROTE_NOW,
    WROTE_SINCE, rest, rests, spread_with, tick,
};

/// The behaviour's state keys (`GSWeaponHitscan._SINCE` and the rest), as dictionary keys.
struct Keys {
    since: Variant,
    bloom: Variant,
    motion: Variant,
    factor: Variant,
    now: Variant,
    recoil_n: Variant,
    recoil_tick: Variant,
    recoil_ax: Variant,
    recoil_ay: Variant,
}

impl Keys {
    fn new() -> Self {
        let k = |s: &str| StringName::from(s).to_variant();
        Self {
            since: k("since_use"),
            bloom: k("bloom"),
            motion: k("motion"),
            factor: k("spread_factor"),
            now: k("spread_now"),
            recoil_n: k("recoil_n"),
            recoil_tick: k("recoil_tick"),
            recoil_ax: k("recoil_ax"),
            recoil_ay: k("recoil_ay"),
        }
    }
}

/// One hitscan weapon's spread tick. Until [method set_tuning] succeeds every call answers -1 and writes nothing.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimWeaponSpread {
    tuning: Option<Tuning>,
    frame: Frame,
    keys: Keys,
    /// [method scatter]'s pellets, kept so a shot allocates no buffer.
    pellets: Vec<Vec3>,
}

#[godot_api]
impl IRefCounted for GSSimWeaponSpread {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            tuning: None,
            frame: Frame::default(),
            keys: Keys::new(),
            pellets: Vec::new(),
        }
    }
}

fn float_of(v: &Variant) -> Option<f64> {
    match v.get_type() {
        VariantType::FLOAT => Some(v.to::<f64>()),
        VariantType::INT => Some(v.to::<i64>() as f64),
        _ => None,
    }
}

fn vec3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn int_of(v: &Variant) -> Option<i64> {
    match v.get_type() {
        VariantType::INT => Some(v.to::<i64>()),
        VariantType::FLOAT => Some(v.to::<f64>() as i64),
        _ => None,
    }
}

impl GSSimWeaponSpread {
    fn read(&self, state: &VarDictionary) -> State {
        let k = &self.keys;
        State {
            since: int_of(&state.get_or_nil(&k.since)),
            bloom: float_of(&state.get_or_nil(&k.bloom)),
            motion: float_of(&state.get_or_nil(&k.motion)),
            factor: float_of(&state.get_or_nil(&k.factor)),
            now: float_of(&state.get_or_nil(&k.now)),
        }
    }

    fn posture(row: &PackedFloat64Array) -> Option<Posture> {
        Posture::from_row(row.as_slice())
    }

    /// [method tick]'s body: tick [param s] (read from [param state]) and write back the keys it changed.
    fn run(
        &self,
        t: &Tuning,
        p: Option<&Posture>,
        s: &mut State,
        state: &mut VarDictionary,
    ) -> f64 {
        let out = tick(t, p, &self.frame, s);
        let k = &self.keys;
        if out.wrote & WROTE_SINCE != 0 {
            state.set(&k.since, s.since.unwrap_or(0));
        }
        if out.wrote & WROTE_BLOOM != 0 {
            state.set(&k.bloom, s.bloom.unwrap_or(0.0));
        }
        if out.wrote & WROTE_MOTION != 0 {
            match s.motion {
                Some(m) => state.set(&k.motion, m),
                None => {
                    state.remove(&k.motion);
                }
            }
        }
        if out.wrote & WROTE_FACTOR != 0 {
            state.set(&k.factor, s.factor.unwrap_or(1.0));
        }
        if out.wrote & WROTE_NOW != 0 {
            state.set(&k.now, s.now.unwrap_or(0.0));
        }
        out.shown.unwrap_or(-1.0)
    }
}

#[godot_api]
impl GSSimWeaponSpread {
    /// The weapon's numbers: `[spread, spread_moving, spread_speed_ref, spread_airborne, spread_crouched, bloom,
    /// bloom_max, scaled_per_tick(bloom_recovery), _bloom_delay_of(b), _settle_ticks_of(b), hip, slide, sprint,
    /// flinch, bloom_crouch, aim]`, each accuracy term already the field when 0 or more, else the class preset.
    /// False (and the weapon cleared) for a short row.
    #[func]
    fn set_tuning(&mut self, row: PackedFloat64Array) -> bool {
        self.tuning = Tuning::from_row(row.as_slice());
        self.tuning.is_some()
    }

    /// The arsenal's side: its `spread_rules` as `[enabled, scale, mult_hip, mult_aim, mult_stand, mult_walk,
    /// mult_run, mult_air, mult_sprint, mult_slide, mult_crouch, walk_speed, run_speed,
    /// recovery_per_tick(running_tick_rate()), max_spread]` (empty: no rules object), its `crosshair_cone`, and
    /// `running_tick_rate()`. False for a row that is neither empty nor whole (the rules are then dropped).
    #[func]
    fn set_frame(&mut self, rules: PackedFloat64Array, cone: bool, rate: i64) -> bool {
        let r = rules.as_slice();
        let parsed = Rules::from_row(r);
        self.frame = Frame {
            rules: parsed,
            cone,
            rate,
        };
        r.is_empty() || parsed.is_some()
    }

    /// `_tick(ctx)` on [param state]: [param posture] is `[speed, airborne, crouched, aiming, sliding, sprinting,
    /// wall_running, tick, hurt_tick]`, or empty for a tick with no context. Returns the cone it computed (the
    /// behaviour's `_shown`), or -1 when it computed none (no context, no tuning).
    #[func]
    fn tick(&self, mut state: VarDictionary, posture: PackedFloat64Array) -> f64 {
        let Some(t) = self.tuning.as_ref() else {
            return -1.0;
        };
        let p = Self::posture(&posture);
        let mut s = self.read(&state);
        self.run(t, p.as_ref(), &mut s, &mut state)
    }

    /// An idle player's tick in one call: [method GSWeaponHitscan.rest_tick] when [param rest_ok] (the carrier is
    /// settled and the arsenal skips rest ticks) and the state has nothing to recover, else the [method tick] the arsenal
    /// would run with [param posture] (the `idle_posture` context as a row). Returns -2 for a rest tick (only the ticks
    /// since the last use moved; the cone was not computed), the cone for a tick, -1 with no tuning or a short row.
    #[func]
    fn idle(&self, mut state: VarDictionary, posture: PackedFloat64Array, rest_ok: bool) -> f64 {
        let (Some(t), Some(p)) = (self.tuning.as_ref(), Self::posture(&posture)) else {
            return -1.0;
        };
        let mut s = self.read(&state);
        if rest_ok && rests(t, &self.frame, &s) {
            rest(&mut s);
            state.set(&self.keys.since, s.since.unwrap_or(0));
            return -2.0;
        }
        self.run(t, Some(&p), &mut s, &mut state)
    }

    /// `_spread_with(b, ctx, _rules())`: the cone for [param posture] over [param state], changing nothing; -1 with
    /// no tuning or a short posture row.
    #[func]
    fn spread(&self, state: VarDictionary, posture: PackedFloat64Array) -> f64 {
        match (self.tuning.as_ref(), Self::posture(&posture)) {
            (Some(t), Some(p)) => spread_with(t, &p, &self.frame, &self.read(&state)),
            _ => -1.0,
        }
    }

    /// `GSWeaponSpreadRules.target_factor(ctx)` under the frame's rules (1 with none or disabled).
    #[func]
    fn target_factor(&self, posture: PackedFloat64Array) -> f64 {
        match (self.frame.rules.as_ref(), Self::posture(&posture)) {
            (Some(r), Some(p)) => r.target_factor(&p),
            _ => 1.0,
        }
    }

    /// `GSWeaponBallistics.spread_for(speed, airborne, crouched)`; -1 with no tuning.
    #[func]
    fn spread_for(&self, speed: f64, airborne: bool, crouched: bool) -> f64 {
        self.tuning
            .as_ref()
            .map_or(-1.0, |t| t.spread_for(speed, airborne, crouched))
    }

    /// `GSWeaponRecoilRules.apply` then `offset_direction` for one use, on [param state]'s `recoil_*` keys (all four
    /// set). [param posture] is the use's posture row (as [method tick]'s; its tick is the use's), [param row]
    /// `[maxi(tick_rate, 1), reset, b.recoil_recovery * recovery, b.recoil_build * build, b.recoil_build_max * build,
    /// b.recoil_pitch * vertical, b.recoil_yaw * horizontal, share, follow, seed_for(b, id), scale, mult_aim,
    /// mult_crouch, mult_air, mult_move, weapon aim, weapon crouch, weapon move]` (the weapon terms of
    /// `recoil_posture`, each the field when 0 or more, else the class preset). Returns `[recoil.x, recoil.y, dir.x,
    /// dir.y, dir.z]`: the outcome's view kick and [param direction] turned by the bullet's offset. Empty (and
    /// nothing written) for a short row.
    #[func]
    fn recoil(
        &self,
        mut state: VarDictionary,
        posture: PackedFloat64Array,
        row: PackedFloat64Array,
        direction: Vector3,
    ) -> PackedFloat64Array {
        let (Some(r), Some(p)) = (Recoil::from_row(row.as_slice()), Self::posture(&posture)) else {
            return PackedFloat64Array::new();
        };
        let k = &self.keys;
        let mut s = RecoilState {
            n: int_of(&state.get_or_nil(&k.recoil_n)),
            tick: int_of(&state.get_or_nil(&k.recoil_tick)),
            ax: float_of(&state.get_or_nil(&k.recoil_ax)),
            ay: float_of(&state.get_or_nil(&k.recoil_ay)),
        };
        let kick = recoil(&r, &p, &mut s);
        state.set(&k.recoil_n, s.n.unwrap_or(0));
        state.set(&k.recoil_tick, s.tick.unwrap_or(0));
        state.set(&k.recoil_ax, s.ax.unwrap_or(0.0));
        state.set(&k.recoil_ay, s.ay.unwrap_or(0.0));
        let d = offset_direction(vec3(direction), kick.offset.0, kick.offset.1);
        PackedFloat64Array::from(
            &[
                kick.view.0 as f64,
                kick.view.1 as f64,
                d.x as f64,
                d.y as f64,
                d.z as f64,
            ][..],
        )
    }

    /// `GSShot.scatter()` for a shot of [param pellet_count] pellets along [param direction]: its `pellets`, as the
    /// typed array the script keeps.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn scatter(
        &mut self,
        direction: Vector3,
        spread: f64,
        entity: i64,
        tick: i64,
        index: i64,
        pellet_count: i64,
        fixed: bool,
    ) -> Array<Vector3> {
        let shot = Shot {
            direction: vec3(direction),
            spread,
            entity,
            tick,
            index,
            pellet_count,
            fixed,
        };
        scatter(&shot, &mut self.pellets);
        let mut out = Array::<Vector3>::new();
        for p in &self.pellets {
            out.push(Vector3::new(p.x, p.y, p.z));
        }
        out
    }
}
