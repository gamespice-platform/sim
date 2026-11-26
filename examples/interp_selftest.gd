extends Node

## The interp module's suite: GameSpice Net's [code]GSNetInterpolator.apply[/code] in GDScript and [code]GSSimInterpolator[/code] on the same seeded streams (loss, reordering, duplicate and partial ticks, stalls, extrapolation past its cap,
## resumes eased and teleports not, vectors, rotations, colours, discrete and type-switching values, a write hook, a plan that changes, forget and prune, an entity turned predicted and back), frame for frame, odd seeds through [code]apply_frame[/code] (one library call a frame) and even seeds through [code]apply[/code] per entity.
## Every property written, every [code]_net_interpolated[/code] tick and every counter must be equal to the bit. With no library it checks the fallback and says the native half was skipped; with [code]GS_SIM_REQUIRE_NATIVE=1[/code] (CI) a missing library fails instead.

const ENTITIES := 4
const FRAMES := 2400

## Every kind of value the interpolator draws, interpolated; one plain property it must not touch.
class Body extends GSNetBehaviour:
	var position: Variant = Vector3.ZERO
	var aim: Variant = Vector2.ZERO
	var yaw: Variant = 0.0
	var rot: Variant = Quaternion.IDENTITY
	var tint: Variant = Color.WHITE
	var stance: Variant = 0
	var alive: Variant = true
	var mixed: Variant = 0.0
	var health: Variant = 100
	var ticks: Array = []

	func _register_net_vars() -> void:
		replicate(&"position", GSNetVar.Type.VECTOR3_POSITION).interpolated()
		replicate(&"aim", GSNetVar.Type.FLOAT).interpolated()
		replicate(&"yaw", GSNetVar.Type.FLOAT).interpolated()
		replicate(&"rot", GSNetVar.Type.QUATERNION).interpolated()
		replicate(&"tint", GSNetVar.Type.FLOAT).interpolated()
		replicate(&"stance", GSNetVar.Type.UINT).bits(3).interpolated()
		replicate(&"alive", GSNetVar.Type.BOOL).interpolated()
		replicate(&"mixed", GSNetVar.Type.FLOAT).interpolated()
		replicate(&"health", GSNetVar.Type.UINT).bits(8)

	func _net_interpolated(tick: int) -> void:
		ticks.append(tick)

	## This frame's values and notifications; the notifications are then cleared.
	func state() -> PackedByteArray:
		var out := var_to_bytes([position, aim, yaw, rot, tint, stance, alive, mixed, health, ticks])
		ticks.clear()
		return out


## A behaviour that overrides the write: each value goes through the hook, which keeps its own copy.
class Hooked extends GSNetBehaviour:
	var velocity: Variant = Vector3.ZERO
	var spin: Variant = 0.0
	var writes: Array = []
	var write_count := 0

	func _register_net_vars() -> void:
		replicate(&"spin", GSNetVar.Type.FLOAT).interpolated()
		replicate(&"velocity", GSNetVar.Type.VECTOR3_VELOCITY).interpolated()

	func _net_write_property(property: StringName, value: Variant) -> void:
		writes.append([property, value])
		write_count += 1
		set(property, value)

	## This frame's values and hook calls; the calls are then cleared.
	func state() -> PackedByteArray:
		var out := var_to_bytes([velocity, spin, writes])
		writes.clear()
		return out


## FC's remote player for the mirror test: a node whose controller holds the typed state the hook copies onto.
class MirrorState extends RefCounted:
	var position := Vector3.ZERO
	var yaw := 0.0
	var pitch := 0.0
	var crouch_fraction := 0.0


class MirrorController extends RefCounted:
	var state := MirrorState.new()


class MirrorPlayer extends Node3D:
	var controller := MirrorController.new()


## FC's [code]FcPlayerNet[/code]: typed net vars, and a [code]_net_interpolated[/code] that copies them onto the
## player's state and moves the player. [method GSNetInterpolator.bind_mirror] does the same copy natively.
class Present extends GSNetBehaviour:
	var net_position := Vector3.ZERO
	var net_yaw := 0.0
	var net_pitch := 0.0
	var net_crouch := 0.0
	var player: MirrorPlayer
	var hooks := 0

	func _register_net_vars() -> void:
		replicate(&"net_position", GSNetVar.Type.VECTOR3_POSITION).interpolated()
		replicate(&"net_yaw", GSNetVar.Type.FLOAT).interpolated()
		replicate(&"net_pitch", GSNetVar.Type.FLOAT).interpolated()
		replicate(&"net_crouch", GSNetVar.Type.FLOAT).interpolated()

	func _net_interpolated(_tick: int) -> void:
		hooks += 1
		if player == null:
			return
		var state := player.controller.state
		state.position = net_position
		state.yaw = net_yaw
		state.pitch = net_pitch
		state.crouch_fraction = net_crouch
		player.global_position = state.position

	## The player's transform and state, to the bit.
	func state() -> PackedByteArray:
		var s := player.controller.state
		return var_to_bytes([player.global_transform, s.position, s.yaw, s.pitch, s.crouch_fraction, net_position, net_yaw, net_pitch, net_crouch])


var _passed := 0
var _failed := 0
var _native := false


func _ready() -> void:
	GSLog.set_level(GSLog.Level.ERROR)
	_run.call_deferred()


func _run() -> void:
	print("gs-sim interp self-test")
	_native = GSSim.has(&"interp")
	print("  native interp module: %s" % ("loaded" if _native else "absent (script interpolator only)"))
	if OS.get_environment("GS_SIM_REQUIRE_NATIVE") == "1":
		_check(_native, "the library is loaded (GS_SIM_REQUIRE_NATIVE=1)")
	_test_fallback()
	if _native:
		for run_seed in [1, 2, 3, 4, 5, 6]:
			_test_stream(run_seed)
		for run_seed in [7, 9]:
			_test_stream(run_seed, true)
		_test_settings_off()
		_bench()
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


func _config() -> GSNetConfig:
	var config := GSNetConfig.new()
	config.tick_rate = 64
	config.snapshot_rate = 32
	config.max_extrapolation_sec = 0.1
	config.resume_ease_max = 6.0
	config.resume_ease_sec = 0.12
	return config


func _interpolator(config: GSNetConfig, native: bool) -> GSNetInterpolator:
	var was := GSNetInterpolator.native_enabled
	GSNetInterpolator.native_enabled = native
	var interpolator := GSNetInterpolator.new(config)
	GSNetInterpolator.native_enabled = was
	return interpolator


## One side of a run: an interpolator and its entities.
class Rig:
	var interpolator: GSNetInterpolator
	var identities: Array[GSNetIdentity] = []

	func _init(p_interpolator: GSNetInterpolator, config: GSNetConfig, count: int) -> void:
		interpolator = p_interpolator
		for i in range(count):
			var identity := GSNetIdentity.new()
			identity.net_id = 10 + i
			var body := Body.new()
			var hooked := Hooked.new()
			body._bind(config)
			hooked._bind(config)
			var list: Array[GSNetBehaviour] = [body, hooked]
			identity.behaviours = list
			identities.append(identity)

	func state() -> PackedByteArray:
		var out := PackedByteArray()
		for identity in identities:
			for behaviour in identity.behaviours:
				out.append_array(behaviour.call(&"state"))
		return out

	func free_all() -> void:
		for identity in identities:
			for behaviour in identity.behaviours:
				behaviour.free()
			identity.free()


func _test_fallback() -> void:
	var config := _config()
	var script_only := _interpolator(config, false)
	_check(not script_only.is_native(), "native_enabled = false keeps the GDScript")
	config.sim_interp = false
	_check(not _interpolator(config, true).is_native(), "sim_interp = false keeps the GDScript")
	config.sim_interp = true
	_check(_interpolator(config, true).is_native() == _native, "the library is used exactly when loaded")


func _test_settings_off() -> void:
	var config := _config()
	config.sim_interp = false
	var interpolator := _interpolator(config, true)
	_check(interpolator.describe()["native"] == false, "describe() says when the GDScript draws")


## A random value of [param property] near [param base] (an entity's path), sometimes far.
func _values(rng: RandomNumberGenerator, tick: int, entity: int, partial: bool, teleport: bool) -> Dictionary:
	var t := float(tick)
	var r := 3.0 + float(entity)
	var centre := Vector3(100.0, 0.0, 0.0) * float(entity)
	var pos := centre + Vector3(cos(t * 0.05) * r, sin(t * 0.03), sin(t * 0.05) * r)
	if teleport:
		pos += Vector3(40.0, 0.0, -25.0)
	var all := {
		"position": pos,
		&"aim": Vector2(sin(t * 0.07), cos(t * 0.11)) * 2.5,
		&"yaw": fmod(t * 0.04, TAU) + rng.randf() * 0.01,
		&"rot": Quaternion(Vector3(rng.randf() - 0.5, 1.0, rng.randf() - 0.5).normalized(), rng.randf() * TAU),
		&"tint": Color(rng.randf(), rng.randf(), rng.randf(), 1.0),
		&"stance": rng.randi_range(0, 3),
		&"alive": rng.randf() < 0.9,
		# A float most of the time, an int now and then: a kind change between samples holds.
		&"mixed": rng.randi_range(0, 9) if rng.randf() < 0.2 else rng.randf() * 10.0,
		&"health": rng.randi_range(0, 100),
		&"spin": rng.randf_range(-3.0, 3.0),
		&"velocity": Vector3(rng.randf() - 0.5, 0.0, rng.randf() - 0.5) * 9.0,
		&"unknown": 7,
	}
	if not partial:
		return all
	var out := {}
	for key in all:
		if rng.randf() < 0.4:
			out[key] = all[key]
	return out


## Both sides fed the same pushes, frames and housekeeping; compared after every frame.
## With [param settle], [member GSNetConfig.skip_settled_interp] on both sides (odd seeds: [method GSNetInterpolator.apply_frame]).
func _test_stream(run_seed: int, settle: bool = false) -> void:
	var config := _config()
	config.skip_settled_interp = settle
	var rng := RandomNumberGenerator.new()
	rng.seed = run_seed
	var a := Rig.new(_interpolator(config, false), config, ENTITIES)
	var b := Rig.new(_interpolator(config, true), config, ENTITIES)
	var sides: Array[Rig] = [a, b]
	var loss := 0.05 * float(run_seed % 3)
	var queue: Array = []
	var next_send := 0
	var stalled_until := -1
	var teleported := {}
	var now := 0.0
	var first_bad := -1
	var bad_what := ""
	var drew := 0
	for frame in range(FRAMES):
		var dt: float = [1.0 / 144.0, 1.0 / 60.0, 1.0 / 30.0, 0.0][rng.randi_range(0, 3)] if run_seed % 2 == 0 else 1.0 / 144.0
		now += dt * float(config.tick_rate) if dt > 0.0 else 0.25
		# The server sends every other tick; some are lost, some late, some stalled for a while.
		while next_send <= int(now):
			if rng.randf() < 0.01:
				stalled_until = next_send + rng.randi_range(8, 30)
			if next_send >= stalled_until and rng.randf() >= loss:
				for e in range(ENTITIES):
					var teleport := rng.randf() < 0.004
					if teleport:
						teleported[e] = not teleported.get(e, false)
					var partial := rng.randf() < 0.3
					var vals := _values(rng, next_send, e, partial, teleported.get(e, false))
					var delay := rng.randi_range(0, 4) if rng.randf() < 0.3 else 0
					queue.append([int(now) + delay, next_send, 10 + e, vals])
					if rng.randf() < 0.05:
						# A retransmit of part of it, merged into the tick already held.
						queue.append([int(now) + delay + 1, next_send, 10 + e, _values(rng, next_send, e, true, teleported.get(e, false))])
			next_send += 2
		var due: Array = []
		var rest: Array = []
		for item in queue:
			(due if int(item[0]) <= int(now) else rest).append(item)
		queue = rest
		if run_seed % 3 == 1:
			due.shuffle()
		for item in due:
			for side in sides:
				side.interpolator.push(item[2], item[1], (item[3] as Dictionary).duplicate())
		for item in due:
			for side in sides:
				side.interpolator.note_lateness(item[1], int(now))
		for side in sides:
			side.interpolator.advance(dt)
		_stream_housekeeping(sides, frame)
		for side in sides:
			if run_seed % 2 == 1:
				side.interpolator.apply_frame(side.identities, now)
			else:
				for identity in side.identities:
					side.interpolator.apply(identity, now)
		if first_bad < 0:
			var diff := _first_difference(a, b)
			if a.state() != b.state():
				first_bad = frame
				bad_what = diff
		drew += 1
	var label := "seed %d (loss %.2f%s%s): %d frames" % [run_seed, loss, ", reordered" if run_seed % 3 == 1 else "", ", settled skip" if settle else "", drew]
	if settle:
		var settled := [a.interpolator.settled_count(), b.interpolator.settled_count()]
		_check(settled[0] == settled[1] and int(settled[0]) > 0, "seed %d settled skips %s" % [run_seed, settled])
	_check(first_bad < 0, label + " write for write", "first difference at frame %d: %s" % [first_bad, bad_what])
	_check_stream_counters(a, b, run_seed)
	a.free_all()
	b.free_all()


## Counters, the write hook and reset agree between the two interpolators after a stream.
func _check_stream_counters(a: Rig, b: Rig, run_seed: int) -> void:
	var counts_a := [a.interpolator.sample_count(), a.interpolator.stall_count(), a.interpolator.extrapolation_count()]
	var counts_b := [b.interpolator.sample_count(), b.interpolator.stall_count(), b.interpolator.extrapolation_count()]
	_check(counts_a == counts_b and int(counts_a[2]) > 0, "seed %d counters %s" % [run_seed, counts_a], str(counts_b))
	var eased := 0
	for identity in a.identities:
		eased += (identity.behaviours[1] as Hooked).write_count
	_check(eased > 0, "seed %d write hook called %d times on both" % [run_seed, eased])
	a.interpolator.reset_counts()
	b.interpolator.reset_counts()
	_check(b.interpolator.sample_count() == 0 and b.interpolator.track_count() == a.interpolator.track_count(), "seed %d reset and track count" % run_seed)


## Housekeeping now and then: an entity forgotten, the set pruned, a behaviour's plan grown.
func _stream_housekeeping(sides: Array[Rig], frame: int) -> void:
	if frame == 700:
		for side in sides:
			side.interpolator.forget(11)
	if frame == 1100:
		for side in sides:
			side.interpolator.prune(PackedInt64Array([10, 11, 13]))
	if frame == 1500:
		for side in sides:
			var body := side.identities[0].behaviours[0] as Body
			body.replicate(&"health", GSNetVar.Type.UINT).interpolated()
	if frame == 1900:
		for side in sides:
			side.interpolator.clear()
	# An entity the local peer predicts for a while: neither path may write it.
	if frame == 300 or frame == 900:
		for side in sides:
			var identity := side.identities[2]
			identity.authority = GSNetIdentity.Authority.SHARED if frame == 300 else GSNetIdentity.Authority.SERVER
			identity.is_owner = frame == 300


func _first_difference(a: Rig, b: Rig) -> String:
	for i in range(a.identities.size()):
		for j in range(a.identities[i].behaviours.size()):
			var x: Object = a.identities[i].behaviours[j]
			var y: Object = b.identities[i].behaviours[j]
			for property in [&"position", &"aim", &"yaw", &"rot", &"tint", &"stance", &"alive", &"mixed", &"health", &"ticks", &"velocity", &"spin", &"writes"]:
				if property in x and var_to_bytes(x.get(property)) != var_to_bytes(y.get(property)):
					return "entity %d behaviour %d %s: %s vs %s" % [i, j, property, str(x.get(property)).left(160), str(y.get(property)).left(160)]
	return "?"


## What one frame of [code]apply[/code] costs for 64 entities on each side (not a pass condition).
func _bench() -> void:
	var config := _config()
	var rng := RandomNumberGenerator.new()
	rng.seed = 99
	for native in [false, true]:
		var side := Rig.new(_interpolator(config, native), config, 64)
		for tick in range(0, 64, 2):
			for e in range(64):
				side.interpolator.push(10 + e, tick, _values(rng, tick, e, false, false))
		var now := 40.0
		var started := Time.get_ticks_usec()
		for frame in range(400):
			now += 64.0 / 144.0 * 0.05
			side.interpolator.advance(1.0 / 144.0)
			for identity in side.identities:
				side.interpolator.apply(identity, now)
		var ms := float(Time.get_ticks_usec() - started) / 1000.0 / 400.0
		started = Time.get_ticks_usec()
		for frame in range(400):
			now += 64.0 / 144.0 * 0.05
			side.interpolator.advance(1.0 / 144.0)
			side.interpolator.apply_frame(side.identities, now)
		var frame_ms := float(Time.get_ticks_usec() - started) / 1000.0 / 400.0
		print("  bench  64 entities, %s: apply %.3f ms, apply_frame %.3f ms per frame" % ["native" if native else "GDScript", ms, frame_ms])
		side.free_all()


const MIRROR_PAIRS := {&"net_position": &"position", &"net_yaw": &"yaw", &"net_pitch": &"pitch", &"net_crouch": &"crouch_fraction"}


## One side of the mirror test: [param count] players under a moved parent, each with a [Present].
func _mirror_side(config: GSNetConfig, native: bool, count: int) -> Array:
	var interpolator := _interpolator(config, native)
	var root := Node3D.new()
	root.position = Vector3(3.25, -1.5, 7.0)
	root.rotation = Vector3(0.0, 0.7, 0.0)
	add_child(root)
	var identities: Array[GSNetIdentity] = []
	for i in range(count):
		var player := MirrorPlayer.new()
		root.add_child(player)
		var identity := GSNetIdentity.new()
		identity.net_id = 10 + i
		var behaviour := Present.new()
		behaviour._bind(config)
		behaviour.player = player
		var list: Array[GSNetBehaviour] = [behaviour]
		identity.behaviours = list
		identities.append(identity)
		if native:
			interpolator.bind_mirror(behaviour, player, PackedStringArray(["controller", "state"]), MIRROR_PAIRS, &"position")
	return [interpolator, identities, root]


func _mirror_values(rng: RandomNumberGenerator, tick: int, entity: int, partial: bool) -> Dictionary:
	var t := float(tick)
	var all := {
		&"net_position": Vector3(cos(t * 0.05) * 4.0, sin(t * 0.03), sin(t * 0.05) * 4.0) + Vector3(20.0, 0.0, 0.0) * float(entity),
		&"net_yaw": fmod(t * 0.04, TAU) + rng.randf() * 0.01,
		&"net_pitch": rng.randf_range(-1.2, 1.2),
		&"net_crouch": rng.randf(),
	}
	if not partial:
		return all
	var out := {}
	for key in all:
		if rng.randf() < 0.5:
			out[key] = all[key]
	return out


func _mirror_state(identities: Array[GSNetIdentity]) -> PackedByteArray:
	var out := PackedByteArray()
	for identity in identities:
		out.append_array((identity.behaviours[0] as Present).state())
	return out


## [method GSNetInterpolator.bind_mirror] against the script hook it replaces: the same seeded stream (loss, stalls,
## partial ticks, a player freed, a mirror unbound) into a script interpolator running the hook and a native one
## running the mirror; every player's global transform and state must be equal to the bit after every frame.
func _test_mirror(run_seed: int) -> void:
	var config := _config()
	var rng := RandomNumberGenerator.new()
	rng.seed = run_seed
	var a := _mirror_side(config, false, ENTITIES)
	var b := _mirror_side(config, true, ENTITIES)
	var ia: GSNetInterpolator = a[0]
	var ib: GSNetInterpolator = b[0]
	_check(ib.mirror_count() == ENTITIES and ia.mirror_count() == 0, "seed %d mirrors bound only natively (%d)" % [run_seed, ib.mirror_count()])
	var now := 0.0
	var next_send := 0
	var first_bad := -1
	for frame in range(FRAMES):
		var dt: float = [1.0 / 144.0, 1.0 / 60.0, 1.0 / 30.0][rng.randi_range(0, 2)]
		now += dt * float(config.tick_rate)
		while next_send <= int(now):
			if rng.randf() >= 0.05:
				for e in range(ENTITIES):
					var vals := _mirror_values(rng, next_send, e, rng.randf() < 0.3)
					ia.push(10 + e, next_send, vals.duplicate())
					ib.push(10 + e, next_send, vals.duplicate())
			next_send += 2
		if frame == 800:
			# A mirror unbound: the native side goes back to the script hook.
			ib.unbind_mirror(b[1][1].behaviours[0])
		for side in [a, b]:
			var interpolator: GSNetInterpolator = side[0]
			interpolator.advance(dt)
			if frame % 2 == 0:
				interpolator.apply_frame(side[1], now)
			else:
				for identity in side[1]:
					interpolator.apply(identity, now)
		if first_bad < 0 and _mirror_state(a[1]) != _mirror_state(b[1]):
			first_bad = frame
	var hooks := [0, 0]
	for i in range(ENTITIES):
		hooks[0] += (a[1][i].behaviours[0] as Present).hooks
		hooks[1] += (b[1][i].behaviours[0] as Present).hooks
	var mirrored: int = ib._native.mirrored_count()
	_check(first_bad < 0, "seed %d mirror: %d frames, transforms and state to the bit" % [run_seed, FRAMES], "first difference at frame %d" % first_bad)
	_check(mirrored > 0 and hooks[1] > 0 and hooks[0] == hooks[1] + mirrored, "seed %d mirror replaced %d hook calls, %d left to the script after unbind" % [run_seed, mirrored, hooks[1]], str([hooks, mirrored]))
	ib.prune(PackedInt64Array([10, 12, 13]))
	for side in [a, b]:
		for identity in side[1]:
			identity.behaviours[0].free()
			identity.free()
		side[2].free()
	ib.clear()
	_check(ib.mirror_count() == 0, "seed %d clear drops the mirrors" % run_seed)


## What the remote-present copy costs per frame for 32 players: script hook against mirror (not a pass condition).
func _bench_mirror() -> void:
	var config := _config()
	var rng := RandomNumberGenerator.new()
	rng.seed = 98
	for mode in ["script interp + hook", "native interp + hook", "native interp + mirror"]:
		var side := _mirror_side(config, mode != "script interp + hook", 32)
		var interpolator: GSNetInterpolator = side[0]
		if mode == "native interp + hook":
			for identity in side[1]:
				interpolator.unbind_mirror(identity.behaviours[0])
		for tick in range(0, 64, 2):
			for e in range(32):
				interpolator.push(10 + e, tick, _mirror_values(rng, tick, e, false))
		var now := 40.0
		var started := Time.get_ticks_usec()
		for frame in range(400):
			now += 64.0 / 144.0 * 0.05
			interpolator.advance(1.0 / 144.0)
			interpolator.apply_frame(side[1], now)
		var ms := float(Time.get_ticks_usec() - started) / 1000.0 / 400.0
		print("  bench  32 remote players, %s: %.3f ms per frame" % [mode, ms])
		for identity in side[1]:
			identity.behaviours[0].free()
			identity.free()
		side[2].free()
