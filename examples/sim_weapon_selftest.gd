extends Node

## dot-sim's weapon parity suite (`godot --headless --path . res://examples/sim_weapon_selftest.tscn`): DotSimWeaponSpread against dot-weapon's script tick, same inputs, same answers.
## Needs dot-weapon linked and the library installed; without either it says so and passes.

var _passed := 0
var _failed := 0


func _ready() -> void:
	DotLog.set_level(DotLog.Level.ERROR)
	_run.call_deferred()


func _run() -> void:
	print("dot-sim weapon self-test")
	var native := DotSim.has(&"weapon")
	if (OS.get_environment("GS_SIM_REQUIRE_NATIVE") if OS.has_environment("GS_SIM_REQUIRE_NATIVE") else OS.get_environment("DOT_SIM_REQUIRE_NATIVE")) == "1":
		_check(native, "the library is loaded (GS_SIM_REQUIRE_NATIVE=1)")
	if native:
		_test_weapon_parity()
	print("")
	print("%d passed, %d failed" % [_passed, _failed])
	get_tree().quit(1 if _failed > 0 else 0)


func _check(ok: bool, what: String, detail: String = "") -> void:
	if ok:
		_passed += 1
		print("  ok    %s" % what)
	else:
		_failed += 1
		print("  FAIL  %s%s" % [what, "" if detail == "" else " (%s)" % detail])


## dot-weapon's addon, linked for the weapon parity test (see README); absent, the test is skipped. Loaded by path, so
## this file parses without it.
const WEAPON_DIR := "res://addons/dot_weapon/"

## Random weapons (tuning, tick rate, rules) and context ticks per weapon.
const WEAPON_CASES := 300
const WEAPON_TICKS := 120


## The arsenal side a behaviour reads: `spread_rules`, `crosshair_cone` and `tick_rate`.
class WeaponArsenal:
	extends RefCounted
	var spread_rules: Variant = null
	var crosshair_cone: bool = false
	var tick_rate: Variant = 0


func _test_weapon_parity() -> void:
	print("weapon")
	if not ResourceLoader.exists(WEAPON_DIR + "behaviour/dot_weapon_hitscan.gd"):
		print("  skip  %s not linked" % WEAPON_DIR)
		return
	# The script side is the GDScript tick: with dot-sim loaded the behaviour would otherwise run the native twin itself.
	var hitscan: Script = load(WEAPON_DIR + "behaviour/dot_weapon_hitscan.gd")
	var was_native: bool = hitscan.get(&"native_tick")
	hitscan.set(&"native_tick", false)
	var rng := RandomNumberGenerator.new()
	rng.seed = 20261116
	var ticks := 0
	var same_state := 0
	var shown := 0
	var same_shown := 0
	var same_spread := 0
	var same_factor := 0
	var moved := [0, 0, 0, 0]  # ticks that settled motion, recovered a factor, shed bloom, wrote the cone
	var first_miss := ""
	for i: int in WEAPON_CASES:
		var w := _weapon_case(rng)
		var h: Object = w[0]
		var native: Object = w[1]
		var ctx: Object = load(WEAPON_DIR + "core/dot_weapon_context.gd").new()
		var mirror := {}
		var hold := 0
		for t: int in WEAPON_TICKS:
			if hold <= 0:
				_weapon_posture(rng, ctx, h)
				hold = rng.randi_range(1, 24)
			hold -= 1
			ctx.tick = t
			if rng.randf() < 0.05:
				# A use between ticks: both states take the same writes.
				var bloom := float(h.state.get(&"bloom", 0.0)) + rng.randf_range(0.0, 1.5)
				for d: Dictionary in [h.state, mirror]:
					d[&"since_use"] = 0
					d[&"bloom"] = bloom
			var none := rng.randf() < 0.1
			var before: Dictionary = h.state.duplicate()
			h._tick(null if none else ctx)
			var row := PackedFloat64Array() if none else _weapon_row(ctx)
			var got: float = native.tick(mirror, row)
			ticks += 1
			var miss := _weapon_state_diff(h.state, mirror)
			if miss == "":
				same_state += 1
			elif first_miss == "":
				first_miss = "case %d tick %d: %s" % [i, t, miss]
			if none:
				continue
			shown += 1
			if got == h._shown:
				same_shown += 1
			elif first_miss == "":
				first_miss = "case %d tick %d: shown script %s native %s" % [i, t, h._shown, got]
			var rules: Object = h._rules()
			if native.spread(mirror, row) == h._spread_with(h._ballistics(), ctx, rules):
				same_spread += 1
			var want_factor: float = h._rules_object().target_factor(ctx) if h._rules_object() != null else 1.0
			if native.target_factor(row) == want_factor:
				same_factor += 1
			if float(h.state.get(&"motion", 0.0)) < float(before.get(&"motion", 0.0)):
				moved[0] += 1
			if float(h.state.get(&"spread_factor", 1.0)) < float(before.get(&"spread_factor", 1.0)):
				moved[1] += 1
			if float(h.state.get(&"bloom", 0.0)) < float(before.get(&"bloom", 0.0)):
				moved[2] += 1
			if h.state.has(&"spread_now"):
				moved[3] += 1
	_check(same_state == ticks, "%d ticks of %d random weapons: the native tick leaves the state the script does, key for key and type for type" % [ticks, WEAPON_CASES], first_miss)
	_check(same_shown == shown, "%d context ticks: the native cone matches the script's to the bit" % shown, first_miss)
	_check(same_spread == shown and same_factor == shown, "the cone and target factor asked between ticks match (%d, %d of %d)" % [same_spread, same_factor, shown])
	_check(moved[0] > 100 and moved[1] > 100 and moved[2] > 100 and moved[3] > shown / 4, "the ticks reach every branch (settle %d, recover %d, shed %d, cone %d)" % moved)
	_test_weapon_idle_parity(rng)
	_bench_weapon(rng)
	hitscan.set(&"native_tick", was_native)


## `DotSimWeaponSpread.idle` against the arsenal's idle tick in script: `rest_tick(idle_still)` and, when it declines,
## `_tick(idle_posture)`. Postures are often settled (the rest path) and never aiming, as an idle posture is.
func _test_weapon_idle_parity(rng: RandomNumberGenerator) -> void:
	if not ClassDB.class_has_method(&"DotSimWeaponSpread", &"idle"):
		_check(false, "DotSimWeaponSpread has idle")
		return
	var ticks := 0
	var same_state := 0
	var computed := 0
	var same_shown := 0
	var rested := [0, 0]  # script rest ticks, native rest ticks agreeing with them
	var first_miss := ""
	for i: int in WEAPON_CASES:
		var w := _weapon_case(rng)
		var h: Object = w[0]
		var native: Object = w[1]
		var arsenal: Object = w[2]
		var ctx: Object = load(WEAPON_DIR + "core/dot_weapon_context.gd").new()
		var mirror := {}
		var hold := 0
		var cone: bool = arsenal.spread_rules != null and arsenal.crosshair_cone
		for t: int in WEAPON_TICKS:
			if hold <= 0:
				if rng.randf() < 0.5:
					ctx.speed = 0.0
					ctx.airborne = false
					ctx.crouched = false
					ctx.sliding = false
					ctx.sprinting = false
					ctx.wall_running = false
					ctx.hurt_tick = t - rng.randi_range(0, 80) if rng.randf() < 0.3 else -1
				else:
					_weapon_posture(rng, ctx, h)
				ctx.aiming = false
				hold = rng.randi_range(1, 30)
			hold -= 1
			ctx.tick = t
			if rng.randf() < 0.04:
				var bloom := float(h.state.get(&"bloom", 0.0)) + rng.randf_range(0.0, 1.5)
				for d: Dictionary in [h.state, mirror]:
					d[&"since_use"] = 0
					d[&"bloom"] = bloom
			var still := _weapon_still(ctx, arsenal)
			var script_rested: bool = h.rest_tick(func() -> bool: return still)
			if not script_rested:
				h._tick(ctx)
			var got: float = native.idle(mirror, _weapon_row(ctx), still and not cone)
			ticks += 1
			var miss := _weapon_state_diff(h.state, mirror)
			if miss == "":
				same_state += 1
			elif first_miss == "":
				first_miss = "case %d tick %d: %s" % [i, t, miss]
			if script_rested and not cone:
				rested[0] += 1
				if got == -2.0:
					rested[1] += 1
				elif first_miss == "":
					first_miss = "case %d tick %d: script rested, native %s" % [i, t, got]
				continue
			if got == -2.0:
				if first_miss == "":
					first_miss = "case %d tick %d: native rested, script ticked" % [i, t]
				continue
			computed += 1
			if got == h._shown:
				same_shown += 1
			elif first_miss == "":
				first_miss = "case %d tick %d: shown script %s native %s" % [i, t, h._shown, got]
	_check(same_state == ticks, "%d idle ticks: the native idle leaves the state the script's rest tick or tick does" % ticks, first_miss)
	_check(rested[0] > ticks / 10 and rested[1] == rested[0], "it rests exactly when the script does (%d of %d rest ticks)" % [rested[1], rested[0]], first_miss)
	_check(computed > ticks / 4 and same_shown == computed, "%d computed idle ticks: the native cone matches the script's to the bit" % computed, first_miss)


## The game's `idle_still` for [param ctx]: settled (no speed, grounded, standing, no slide, sprint or wall run) and no
## flinch left at the arsenal's rate (at least 64).
func _weapon_still(ctx: Object, arsenal: Object) -> bool:
	if ctx.speed != 0.0 or ctx.airborne or ctx.crouched or ctx.sliding or ctx.sprinting or ctx.wall_running:
		return false
	if ctx.hurt_tick < 0 or ctx.tick < ctx.hurt_tick:
		return true
	return ctx.tick - ctx.hurt_tick > ceili(0.4 * float(maxi(int(arsenal.tick_rate), 64)))


## One random weapon: a hitscan behaviour on a def with random ballistics (fields or class presets), on an arsenal with
## a random tick rate, rules (none, off or on) and crosshair setting, and the native twin set from the same numbers.
func _weapon_case(rng: RandomNumberGenerator) -> Array:
	var b: Object = load(WEAPON_DIR + "tuning/dot_weapon_ballistics.gd").new()
	b.spread = rng.randf_range(0.0, 3.0)
	b.spread_moving = rng.randf_range(0.0, 8.0)
	b.spread_speed_ref = [0.1, 3.0, 6.0, rng.randf_range(0.5, 12.0)][rng.randi_range(0, 3)]
	b.spread_airborne = rng.randf_range(0.0, 10.0)
	b.spread_crouched = rng.randf_range(0.2, 1.0)
	b.bloom = 0.0 if rng.randf() < 0.15 else rng.randf_range(0.05, 1.0)
	b.bloom_max = rng.randf_range(0.5, 6.0)
	b.bloom_recovery = rng.randf_range(0.01, 0.5)
	b.accuracy_class = ["", "rifle", "smg", "lmg", "sniper", "shotgun", "pistol"][rng.randi_range(0, 6)]
	b.bloom_delay_ticks = -1 if rng.randf() < 0.5 else rng.randi_range(0, 20)
	b.settle_ticks = -1 if rng.randf() < 0.5 else rng.randi_range(0, 12)
	for f: StringName in [&"spread_aim", &"spread_hip", &"spread_sprint", &"spread_slide", &"spread_flinch", &"bloom_crouch"]:
		b.set(f, -1.0 if rng.randf() < 0.5 else rng.randf_range(0.0, 2.0))
	var def: Object = load(WEAPON_DIR + "core/dot_weapon_def.gd").new()
	def.id = StringName(["ak", "mp5", "awp", "deagle", "m3", "x"][rng.randi_range(0, 5)])
	def.tuning = b
	def.authored_tick_rate = [0, 64, 128][rng.randi_range(0, 2)]
	def.use_interval_ticks = rng.randi_range(0, 20)
	var arsenal := WeaponArsenal.new()
	arsenal.tick_rate = [0, 33, 64, 128][rng.randi_range(0, 3)]
	arsenal.crosshair_cone = rng.randf() < 0.5
	var pick := rng.randi_range(0, 2)
	if pick > 0:
		var r: Object = load(WEAPON_DIR + "math/dot_weapon_spread_rules.gd").new()
		r.enabled = pick == 2
		r.scale = rng.randf_range(0.5, 2.0)
		for f: StringName in [&"mult_hip", &"mult_aim", &"mult_stand", &"mult_walk", &"mult_run", &"mult_air", &"mult_sprint", &"mult_slide", &"mult_crouch"]:
			r.set(f, rng.randf_range(0.3, 2.5))
		r.walk_speed = rng.randf_range(0.1, 2.0)
		r.run_speed = rng.randf_range(2.0, 6.0)
		r.recovery = 0.0 if rng.randf() < 0.1 else rng.randf_range(0.5, 12.0)
		r.max_spread = 0.0 if rng.randf() < 0.2 else rng.randf_range(2.0, 20.0)
		arsenal.spread_rules = r
	var h: Object = load(WEAPON_DIR + "behaviour/dot_weapon_hitscan.gd").new()
	h.setup(def, arsenal)
	var native: Object = ClassDB.instantiate(&"DotSimWeaponSpread")
	var ok: bool = native.set_tuning(_weapon_tuning(h, b))
	var rules_row := PackedFloat64Array()
	var on: Object = arsenal.spread_rules
	if on != null:
		rules_row = PackedFloat64Array([
			1.0 if on.enabled else 0.0, on.scale, on.mult_hip, on.mult_aim, on.mult_stand, on.mult_walk, on.mult_run,
			on.mult_air, on.mult_sprint, on.mult_slide, on.mult_crouch, on.walk_speed, on.run_speed,
			on.recovery_per_tick(h.running_tick_rate()), on.max_spread,
		])
	ok = native.set_frame(rules_row, arsenal.crosshair_cone, h.running_tick_rate()) and ok
	if not ok:
		_check(false, "the native weapon takes the tuning and frame rows")
	return [h, native, arsenal]


## The tuning row [method DotSimWeaponSpread.set_tuning] takes, from the behaviour's own resolution of its class.
func _weapon_tuning(h: Object, b: Object) -> PackedFloat64Array:
	h._class_of(b)
	return PackedFloat64Array([
		b.spread, b.spread_moving, b.spread_speed_ref, b.spread_airborne, b.spread_crouched, b.bloom, b.bloom_max,
		h.scaled_per_tick(b.bloom_recovery), h._bloom_delay_of(b), h._settle_ticks_of(b),
		b.spread_hip if b.spread_hip >= 0.0 else h._pre_hip,
		b.spread_slide if b.spread_slide >= 0.0 else h._pre_slide,
		b.spread_sprint if b.spread_sprint >= 0.0 else h._pre_sprint,
		b.spread_flinch if b.spread_flinch >= 0.0 else h._pre_flinch,
		b.bloom_crouch if b.bloom_crouch >= 0.0 else h._pre_crouch,
		b.spread_aim if b.spread_aim >= 0.0 else h._pre_aim,
	])


## A random posture on [param ctx]: often still, at the rules' thresholds, or flinching.
func _weapon_posture(rng: RandomNumberGenerator, ctx: Object, h: Object) -> void:
	var r: Object = h._rules_object()
	var speeds := [0.0, 0.0, rng.randf_range(0.0, 10.0), rng.randf_range(0.0, 1.0)]
	if r != null:
		speeds.append_array([r.walk_speed, r.run_speed])
	ctx.speed = speeds[rng.randi_range(0, speeds.size() - 1)]
	ctx.airborne = rng.randf() < 0.2
	ctx.crouched = rng.randf() < 0.25
	ctx.aiming = rng.randf() < 0.3
	ctx.sliding = rng.randf() < 0.1
	ctx.sprinting = rng.randf() < 0.2
	ctx.wall_running = rng.randf() < 0.05
	ctx.hurt_tick = ctx.tick - rng.randi_range(-2, 40) if rng.randf() < 0.3 else -1


## The posture row [method DotSimWeaponSpread.tick] takes.
func _weapon_row(ctx: Object) -> PackedFloat64Array:
	return PackedFloat64Array([
		ctx.speed, 1.0 if ctx.airborne else 0.0, 1.0 if ctx.crouched else 0.0, 1.0 if ctx.aiming else 0.0,
		1.0 if ctx.sliding else 0.0, 1.0 if ctx.sprinting else 0.0, 1.0 if ctx.wall_running else 0.0, ctx.tick, ctx.hurt_tick,
	])


## "" when the two states hold the same keys with the same types and values, else the first difference.
func _weapon_state_diff(want: Dictionary, got: Dictionary) -> String:
	if want.size() != got.size():
		return "keys script %s native %s" % [want.keys(), got.keys()]
	for k: Variant in want:
		if not got.has(k):
			return "native has no %s" % k
		if typeof(want[k]) != typeof(got[k]) or want[k] != got[k]:
			return "%s script %s (%s) native %s (%s)" % [k, want[k], type_string(typeof(want[k])), got[k], type_string(typeof(got[k]))]
	return ""


## One weapon carried by a moving, aiming, sometimes hurt player: the script's context tick against the native tick
## with the posture row built from the same context each tick (what a caller pays).
func _bench_weapon(rng: RandomNumberGenerator) -> void:
	var w := _weapon_case(rng)
	var h: Object = w[0]
	var native: Object = w[1]
	var ctx: Object = load(WEAPON_DIR + "core/dot_weapon_context.gd").new()
	var mirror := {}
	const N := 20000
	var t0 := Time.get_ticks_usec()
	for t: int in N:
		ctx.tick = t
		ctx.speed = float(t % 7)
		h._tick(ctx)
	var t1 := Time.get_ticks_usec()
	for t: int in N:
		ctx.tick = t
		ctx.speed = float(t % 7)
		native.tick(mirror, _weapon_row(ctx))
	var t2 := Time.get_ticks_usec()
	print("  bench %d moving ticks: script %d us, native (with the row) %d us" % [N, t1 - t0, t2 - t1])
