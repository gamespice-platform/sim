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

    /// [method idle]'s body on a posture row slice (its first 9 floats are read).
    fn idle_on(&self, state: &mut VarDictionary, posture: &[f64], rest_ok: bool) -> f64 {
        let (Some(t), Some(p)) = (self.tuning.as_ref(), Posture::from_row(posture)) else {
            return -1.0;
        };
        let mut s = self.read(state);
        if rest_ok && rests(t, &self.frame, &s) {
            rest(&mut s);
            state.set(&self.keys.since, s.since.unwrap_or(0));
            return -2.0;
        }
        self.run(t, Some(&p), &mut s, state)
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
        self.idle_on(&mut state, posture.as_slice(), rest_ok)
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

/// Values per [method GSSimWeaponIdleBatch.run] entry: the weapon's [GSSimWeaponSpread], its behaviour's `state`
/// dictionary, the behaviour (told the result), the posture row index and whether a rest tick may be taken.
const ENTRY: usize = 5;

/// Many idle weapons' [method GSSimWeaponSpread.idle] in one call: a server with every player idle hands each weapon in
/// hand and its row here instead of one script call chain per player. Same bits as one [method GSSimWeaponSpread.idle]
/// per entry, in entry order.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimWeaponIdleBatch {
    took: StringName,
}

#[godot_api]
impl IRefCounted for GSSimWeaponIdleBatch {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            took: StringName::from("idle_native_took"),
        }
    }
}

#[godot_api]
impl GSSimWeaponIdleBatch {
    /// Runs every entry of [param entries], flat groups of five: `[spread: GSSimWeaponSpread, state: Dictionary,
    /// behaviour: Object, k: int, rest_ok: bool]`. Entry k's posture is `rows[k * stride, k * stride + 9)`, its still flag
    /// `rows[k * stride + 9]` (non-zero: settled), so `stride` is at least 10; the rest tick is tried when `rest_ok` and
    /// still. A taken tick calls `behaviour.idle_native_took(shown)` with [method GSSimWeaponSpread.idle]'s answer (-2
    /// rested, else the cone). Returns the entry numbers (0, 1, ...) that declined: -1 from the twin, a malformed
    /// entry or a row out of range. Those changed nothing and the caller ticks them its own way.
    #[func]
    fn run(&self, entries: VarArray, rows: PackedFloat64Array, stride: i64) -> PackedInt32Array {
        let r = rows.as_slice();
        let n = entries.len() / ENTRY;
        let mut declined = PackedInt32Array::new();
        for e in 0..n {
            let shown = self.entry(&entries, e * ENTRY, r, stride);
            if shown == -1.0 {
                declined.push(e as i32);
            }
        }
        declined
    }
}

impl GSSimWeaponIdleBatch {
    /// One entry at [param at]: the twin's answer, -1 (nothing changed) when it is malformed or declines.
    fn entry(&self, entries: &VarArray, at: usize, rows: &[f64], stride: i64) -> f64 {
        let get = |i: usize| entries.get(at + i).unwrap_or_default();
        let (Ok(spread), Ok(mut state), Ok(mut behaviour)) = (
            get(0).try_to::<Gd<GSSimWeaponSpread>>(),
            get(1).try_to::<VarDictionary>(),
            get(2).try_to::<Gd<Object>>(),
        ) else {
            return -1.0;
        };
        let Some(row) = row_of(rows, int_of(&get(3)), stride) else {
            return -1.0;
        };
        let rest_ok = get(4).try_to::<bool>().unwrap_or(false) && row[9] != 0.0;
        let shown = spread.bind().idle_on(&mut state, &row[..9], rest_ok);
        if shown != -1.0 {
            behaviour.call(&self.took, &[shown.to_variant()]);
        }
        shown
    }
}

/// Row [param k] of [param rows] at [param stride] (at least 10: 9 posture floats and the still flag), if whole.
fn row_of(rows: &[f64], k: Option<i64>, stride: i64) -> Option<&[f64]> {
    let (k, stride) = (usize::try_from(k?).ok()?, usize::try_from(stride).ok()?);
    if stride < 10 {
        return None;
    }
    let from = k.checked_mul(stride)?;
    rows.get(from..from.checked_add(stride)?)
}

#[cfg(test)]
mod tests {
    use super::row_of;

    #[test]
    fn rows_are_whole_or_none() {
        let rows: Vec<f64> = (0..30).map(f64::from).collect();
        assert_eq!(row_of(&rows, Some(1), 10).map(|r| r[0]), Some(10.0));
        assert_eq!(row_of(&rows, Some(2), 10).map(|r| r[9]), Some(29.0));
        assert!(row_of(&rows, Some(3), 10).is_none());
        assert!(row_of(&rows, Some(-1), 10).is_none());
        assert!(row_of(&rows, None, 10).is_none());
        assert!(row_of(&rows, Some(0), 9).is_none());
        assert!(row_of(&rows, Some(i64::MAX), 10).is_none());
    }
}
