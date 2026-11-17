extends Node

## dot-sim's suite (`godot --headless --path . res://examples/sim_selftest.tscn`): each native module against the script it replaces, same inputs, same answers.
## With no library installed it checks that the script fallback runs and says the native half was skipped; CI's native-suites job installs it and sets , so a missing library fails.

const CHANNEL := "dot.sim.selftest"

## Random sight scans compared one by one.
const SCANS := 3000

## Random think-tier ticks (settings, people and bots) compared query by query.
const TIER_ROUNDS := 400

## Bots and ticks of the whole-brain run, played once in script and once native.
const BOTS := 24
const TICKS := 600
const RATE := 30.0

## Line-of-sight calls of the last [method _play].
var _rays := 0
var _passed := 0
var _failed := 0


func _ready() -> void:
	DotLog.set_level(DotLog.Level.ERROR)
	_run.call_deferred()


func _run() -> void:
	print("dot-sim self-test")
	var native := DotSim.has(&"bots")
	print("  native bots module: %s" % ("loaded" if native else "absent (script fallback only)"))
	if (OS.get_environment("GS_SIM_REQUIRE_NATIVE") if OS.has_environment("GS_SIM_REQUIRE_NATIVE") else OS.get_environment("DOT_SIM_REQUIRE_NATIVE")) == "1":
		_check(native, "the library is loaded (GS_SIM_REQUIRE_NATIVE=1)")
	_test_fallback()
	if native:
		_test_scan_parity()
		_test_brain_parity()
		_test_tier_parity()
		_bench_scan()
	if DotSim.has(&"arms"):
		_test_arms_parity()
	if DotSim.has(&"weapon"):
		_test_weapon_parity()
	if DotSim.has(&"sight"):
		await _test_sight_ray_parity()
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


## Random sight rays through DotSimSightRay against the script's PhysicsRayQueryParameters3D ray
## (the world's line_of_sight): same mask, exclusions and end point, on the main thread and on a
## worker holding the lock, and again after a rebind changes the mask; then with a jail cell bound
## (dot_map's DotMapGlassSight.blocks in front, written out here: glass on its own layer).
func _test_sight_ray_parity() -> void:
	var root := Node3D.new()
	add_child(root)
	var rng := RandomNumberGenerator.new()
	rng.seed = 4242
	var rids: Array[RID] = []
	for i in 60:
		var body := StaticBody3D.new()
		body.collision_layer = 4 if i % 7 == 3 else (2 if i % 4 == 0 else 1)
		var shape := CollisionShape3D.new()
		var box := BoxShape3D.new()
		box.size = Vector3(rng.randf_range(0.3, 4.0), rng.randf_range(0.3, 4.0), rng.randf_range(0.3, 4.0))
		shape.shape = box
		body.add_child(shape)
		var spread := 10.0 if body.collision_layer == 4 else 25.0
		body.position = Vector3(rng.randf_range(-spread, spread), rng.randf_range(-3.0, 3.0), rng.randf_range(-spread, spread))
		root.add_child(body)
		rids.append(body.get_rid())
	await get_tree().physics_frame
	await get_tree().physics_frame
	var space := root.get_world_3d().direct_space_state
	var exclude: Array[RID] = [rids[1], rids[2], rids[3]]
	var ray: Object = ClassDB.instantiate(&"DotSimSightRay")
	var lock := Mutex.new()
	var cases: Array = []
	for i in 4000:
		var reach := 30.0 if i % 2 == 0 else 9.0
		var from := Vector3(rng.randf_range(-reach, reach), rng.randf_range(-4.0, 4.0), rng.randf_range(-reach, reach))
		var to := Vector3(rng.randf_range(-30.0, 30.0), rng.randf_range(-4.0, 4.0), rng.randf_range(-30.0, 30.0))
		if i % 50 == 0:
			to = from + Vector3(0.00003, 0.0, 0.00004)
		cases.append([from, to])
	var blocked := 0
	var centre := Vector3(0.5, 0.0, -0.5)
	var radius := 8.0
	var margin := 2.0
	var by_glass := 0
	for config: Array in [[1, false], [3, false], [1, true]]:
		var mask: int = config[0]
		var jail: bool = config[1]
		var what := "mask %d%s" % [mask, " with a jail cell" if jail else ""]
		if jail:
			ray.call(&"bind", space, mask, exclude, lock, space, 4, centre, radius, radius + margin)
		else:
			ray.call(&"bind", space, mask, exclude, lock, null, 0, Vector3.ZERO, 0.0, 0.0)
		var q := PhysicsRayQueryParameters3D.new()
		q.collision_mask = mask
		q.exclude = exclude
		q.collide_with_areas = false
		q.collide_with_bodies = true
		q.hit_from_inside = false
		var want: Array[bool] = []
		for c: Array in cases:
			if jail and (c[0] as Vector3).distance_to(centre) < radius:
				var g := PhysicsRayQueryParameters3D.create(c[0], c[1], 4)
				var hit := space.intersect_ray(g)
				if not hit.is_empty() and (hit["position"] as Vector3).distance_to(centre) < radius + margin:
					by_glass += 1
					want.append(false)
					continue
			var offset: Vector3 = c[1] - c[0]
			var distance := offset.length()
			if distance <= 0.0001:
				want.append(true)
				continue
			q.from = c[0]
			q.to = c[0] + offset / distance * distance
			want.append(space.intersect_ray(q).is_empty())
		var main_miss := -1
		for i in cases.size():
			var got: bool = ray.call(&"clear", cases[i][0], cases[i][1])
			if not got:
				blocked += 1
			if got != want[i] and main_miss < 0:
				main_miss = i
		_check(main_miss < 0, "%s: %d native sight rays match the script ray" % [what, cases.size()], "" if main_miss < 0 else "case %d %s" % [main_miss, cases[main_miss]])
		var thread := Thread.new()
		thread.start(func() -> int:
			var miss := -1
			for i in cases.size():
				if bool(ray.call(&"clear", cases[i][0], cases[i][1])) != want[i] and miss < 0:
					miss = i
			return miss)
		var worker_miss: int = thread.wait_to_finish()
		_check(worker_miss < 0, "%s: the same rays on a worker under the lock match" % what, str(worker_miss))
	_check(blocked > 600 and blocked < 11400, "the rays mix blocked and clear", str(blocked))
	_check(by_glass > 20, "the jail glass blocks some rays from inside the cell", str(by_glass))
	ray.call(&"bind", null, 1, exclude, null, null, 0, Vector3.ZERO, 0.0, 0.0)
	_check(bool(ray.call(&"clear", Vector3.ZERO, Vector3(100, 0, 0))), "with no space bound every ray is clear")
	root.queue_free()


func _test_fallback() -> void:
	print("fallback")
	var world := _world(_bodies(RandomNumberGenerator.new(), 12))
	world.contacts_version = -1
	_check(world.senses(world.contacts()) == null, "a world that does not bump contacts_version scans in script")
	DotBotWorld.native_enabled = false
	world.contacts_version = 1
	_check(world.senses(world.contacts()) == null, "native_enabled off scans in script")
	DotBotWorld.native_enabled = true
	var thread := Thread.new()
	thread.start(func() -> bool: return world.senses(world.contacts()) == null)
	_check(thread.wait_to_finish(), "a brain on a worker thread scans in script")
	var brain := DotBotBrain.new(1)
	var intent := brain.think(world.contacts()[0], world, 1.0)
	_check(intent != null, "a brain thinks with the script scan")


## Every scan of a random world: the same candidates in the same order, scores equal to the bit, the same target forgotten.
func _test_scan_parity() -> void:
	print("sight scan: native against script")
	var rng := RandomNumberGenerator.new()
	rng.seed = 7
	var mismatches := 0
	var first := ""
	var nonempty := 0
	for n in SCANS:
		var bodies := _bodies(rng, rng.randi_range(2, 64))
		var world := _world(bodies)
		world.contacts_version = n
		var me: DotBotContact = bodies[rng.randi_range(0, bodies.size() - 1)]
		var p := DotBotProfile.new()
		p.fov = rng.randf_range(40.0, 200.0)
		p.sight = rng.randf_range(5.0, 120.0)
		var forward := DotBotAim.direction(rng.randf_range(-180.0, 180.0), rng.randf_range(-85.0, 85.0))
		var target: int = (bodies[rng.randi_range(0, bodies.size() - 1)] as DotBotContact).id if rng.randf() < 0.6 else -1
		var visible := rng.randf() < 0.5
		var a := DotBotBrain.new(me.id)
		var b := DotBotBrain.new(me.id)
		for brain: DotBotBrain in [a, b]:
			brain.target_id = target
			brain._visible = visible
		var sa := PackedFloat32Array()
		var sb := PackedFloat32Array()
		var ca := a._candidates(me, bodies, p, me.eye(), forward, p.fov * 0.5, sa)
		var cb := b._candidates_native(world.senses(bodies), world, me, p, me.eye(), forward, p.fov * 0.5, sb)
		if not cb.is_empty():
			nonempty += 1
		var same := ca.size() == cb.size() and sa == sb and a.target_id == b.target_id and a._visible == b._visible
		if same:
			for i in ca.size():
				same = same and ca[i] == cb[i]
		if not same:
			mismatches += 1
			if first == "":
				first = "scan %d: script %s %s, native %s %s" % [n, _ids(ca), sa, _ids(cb), sb]
	_check(mismatches == 0, "%d random scans agree to the bit" % SCANS, first)
	_check(nonempty > SCANS / 4, "the scans are not trivially empty", str(nonempty))


## Whole brains over a moving fight, once in script and once native: every intent of every tick the same.
func _test_brain_parity() -> void:
	print("brains: native against script")
	DotBotWorld.native_enabled = false
	var script_run := _play()
	DotBotWorld.native_enabled = true
	var native_run := _play()
	var first := -1
	for t in mini(script_run.size(), native_run.size()):
		if script_run[t] != native_run[t]:
			first = t
			break
	_check(script_run.size() == TICKS and native_run.size() == TICKS, "both runs played every tick")
	_check(first < 0, "%d bots x %d ticks: every intent the same" % [BOTS, TICKS], "" if first < 0 else "tick %d: %s vs %s" % [first, script_run[first], native_run[first]])
	var fights := 0
	for line: String in script_run:
		fights += line.count("fight")
	_check(fights > BOTS * TICKS / 20, "the run has fights in it", str(fights))
	_test_sight_lod_parity()


## The same fight with the sight LOD on (far 20 m, hold 0.15 s): every intent equal, script against native, and fewer rays than without it.
func _test_sight_lod_parity() -> void:
	print("sight LOD: native against script")
	DotBotWorld.native_enabled = false
	var script_run := _play(20.0, 0.15)
	var rays_script_lod := _rays
	var plain := _play()
	var rays_script_plain := _rays
	DotBotWorld.native_enabled = true
	var native_run := _play(20.0, 0.15)
	var rays_lod := _rays
	var first := -1
	for t in mini(script_run.size(), native_run.size()):
		if script_run[t] != native_run[t]:
			first = t
			break
	_check(first < 0, "%d bots x %d ticks with sight_far on: every intent the same" % [BOTS, TICKS], "" if first < 0 else "tick %d: %s vs %s" % [first, script_run[first], native_run[first]])
	print("  rays over the run: script plain %d, script LOD %d, native LOD %d" % [rays_script_plain, rays_script_lod, rays_lod])
	_check(rays_lod == rays_script_lod, "native and script trace the same number of rays under the LOD", "%d vs %d" % [rays_lod, rays_script_lod])
	_check(rays_script_lod < rays_script_plain, "the LOD traces fewer rays than plain reuse", "%d vs %d" % [rays_script_lod, rays_script_plain])
	_bench_sight_lod()
	_check(plain.size() == TICKS, "the plain run still plays every tick")


## One fight of [constant BOTS] bots, two teams, pillars that block sight; one line of intents per tick.
## [param think_every] 2 or more thinks a bot on every n-th tick only (a crowd's think tier), its last intent kept between; [param cover] is the number of sight-blocking pillars.
func _play(sight_far: float = 0.0, sight_far_reuse: float = 0.15, think_every: int = 1, cover: int = 10) -> PackedStringArray:
	var rng := RandomNumberGenerator.new()
	rng.seed = 11
	var bodies := []
	for i in BOTS:
		var c := DotBotContact.make(100 + i, Vector3(rng.randf_range(-30, 30), 0.0, rng.randf_range(-30, 30)), 1 + i % 2)
		c.view_yaw = rng.randf_range(-180.0, 180.0)
		c.view_pitch = 0.0
		bodies.append(c)
	var world := _world(bodies)
	var pillars: Array[Vector3] = []
	for i in cover:
		pillars.append(Vector3(rng.randf_range(-25, 25), 1.4, rng.randf_range(-25, 25)))
	world.sight_far = sight_far
	world.sight_far_reuse = sight_far_reuse
	_rays = 0
	world.line_of_sight_fn = func(a: Vector3, b: Vector3) -> bool:
		_rays += 1
		for q in pillars:
			if DotBotWorld.segment_hits_sphere(a, b, q, 2.0):
				return false
		return true
	world.waypoints_fn = func() -> PackedVector3Array: return PackedVector3Array([Vector3(-20, 0, -20), Vector3(20, 0, 20), Vector3(-20, 0, 20), Vector3(20, 0, -20)])
	var brains: Array[DotBotBrain] = []
	for c: DotBotContact in bodies:
		brains.append(DotBotBrain.new(c.id))
	var out := PackedStringArray()
	var last := {}
	var dt := 1.0 / RATE
	for t in TICKS:
		var now := t * dt
		world.contacts_version = t
		var line := ""
		# Every bot thinks over the same bodies, then the tick applies them all, as a game does
		# (the bodies may not change between bumps of contacts_version).
		var intents: Array[DotBotIntent] = []
		for i in BOTS:
			var me: DotBotContact = bodies[i]
			var it: DotBotIntent = brains[i].think(me, world, now) if (t + i) % think_every == 0 or t == 0 else last[i]
			last[i] = it
			intents.append(it)
			line += "%d:%s %.4f %.4f %s %s %d|" % [me.id, it.state, it.yaw, it.pitch, it.move, it.fire, it.target_id]
		for i in BOTS:
			var me: DotBotContact = bodies[i]
			var it := intents[i]
			me.view_yaw = it.yaw
			me.view_pitch = it.pitch
			var step := Vector3(it.move.x, 0.0, -it.move.y).rotated(Vector3.UP, deg_to_rad(it.yaw)) * 5.0 * dt
			me.velocity = step / dt
			me.position += step
			if it.fire and it.target_id >= 0:
				var hit := DotBotWorld.find(bodies, it.target_id)
				if hit != null and hit.alive:
					hit.health -= 0.02
					hit.alive = hit.health > 0.0
		for c: DotBotContact in bodies:
			if not c.alive and rng.randf() < 0.02:
				c.alive = true
				c.health = 1.0
		out.append(line)
	return out


## What the LOD saves for a crowd that thinks every third tick: rays and wall time of the native mind, off against on.
func _bench_sight_lod() -> void:
	DotBotWorld.native_enabled = true
	var off_us := 0
	var on_us := 0
	var off_rays := 0
	var on_rays := 0
	for rep in 3:
		var t0 := Time.get_ticks_usec()
		_play(0.0, 0.15, 3, 80)
		off_us += Time.get_ticks_usec() - t0
		off_rays = _rays
		t0 = Time.get_ticks_usec()
		_play(20.0, 0.15, 3, 80)
		on_us += Time.get_ticks_usec() - t0
		on_rays = _rays
	print("  thinking every 3rd tick, 80 pillars of cover, native: rays %d -> %d (%.1f%%), wall %.0f -> %.0f ms per run" % [off_rays, on_rays, 100.0 * (on_rays - off_rays) / maxf(off_rays, 1), off_us / 3000.0, on_us / 3000.0])
	_check(on_rays < off_rays, "the LOD saves rays for a crowd thinking every third tick", "%d vs %d" % [on_rays, off_rays])


## dot-bot's think tiers (DotSimThinkTier): random settings, people and bots; every schedule, period and body period equal, and the same counts.
func _test_tier_parity() -> void:
	print("think tiers: native against script")
	var rng := RandomNumberGenerator.new()
	var gd := DotBotThinkTier.new()
	gd.native = false
	var nat := DotBotThinkTier.new()
	var mismatches := 0
	var queries := 0
	var on := 0
	var first := ""
	for round in TIER_ROUNDS:
		for t: DotBotThinkTier in [gd, nat]:
			rng.seed = round * 7919 + 1
			t.above = rng.randi_range(0, 8)
			t.near_every = rng.randi_range(1, 4)
			t.far_every = rng.randi_range(1, 8)
			t.far_range = rng.randf_range(5.0, 60.0)
			t.close_range = rng.randf_range(1.0, 20.0)
			t.nearest = rng.randi_range(0, 4)
			t.engaged_far_every = rng.randi_range(1, 4)
			t.engaged_range = rng.randf_range(5.0, 40.0)
			t.idle_view_every = rng.randi_range(1, 4)
			t.load_scale = rng.randf_range(0.5, 2.5)
			t.view_range = rng.randf_range(10.0, 90.0)
			t.view_half_angle = rng.randf_range(10.0, 120.0)
			t.body_near_every = rng.randi_range(1, 3)
			t.body_far_every = rng.randi_range(1, 6)
			t.body_above = rng.randi_range(0, 10)
			t.max_period = [8, 0, 3, 16][rng.randi_range(0, 3)]
			t.load_scale *= [1.0, 1.0, 6.0][rng.randi_range(0, 2)]
		rng.seed = round * 104729 + 3
		var people := rng.randi_range(0, 6)
		var eyes := PackedVector3Array()
		var looks := PackedVector3Array()
		for i in people:
			eyes.append(_spot(rng))
			looks.append(Vector3(rng.randf_range(-1, 1), rng.randf_range(-0.3, 0.3), rng.randf_range(-1, 1)).normalized())
		var bots := rng.randi_range(1, 16)
		var ids := PackedInt32Array()
		var spots := PackedVector3Array()
		for i in bots:
			ids.append(100 + i)
			spots.append(_spot(rng))
		var players := people + bots + rng.randi_range(0, 8)
		gd.begin(players, eyes, looks, ids, spots)
		nat.begin(players, eyes, looks, ids, spots)
		if not nat.is_native() or gd.is_native():
			mismatches += 1
			first = first if first != "" else "round %d: native %s, script %s" % [round, nat.is_native(), gd.is_native()]
		if gd.is_on() != nat.is_on():
			mismatches += 1
			first = first if first != "" else "round %d: is_on %s vs %s" % [round, gd.is_on(), nat.is_on()]
		on += int(gd.is_on())
		var tick := rng.randi_range(0, 100000)
		for i in bots:
			var seed := rng.randi_range(-50, 5000)
			var engaged := rng.randf() < 0.3
			var idle := not engaged and rng.randf() < 0.4
			var floor_every := rng.randi_range(1, 3)
			var coastable := rng.randf() < 0.75
			var a := [gd.schedule(ids[i], spots[i], engaged, idle, tick, seed, floor_every, coastable), gd.period(ids[i], spots[i], engaged, idle), gd.body_period(ids[i], spots[i], engaged, idle)]
			var b := [nat.schedule(ids[i], spots[i], engaged, idle, tick, seed, floor_every, coastable), nat.period(ids[i], spots[i], engaged, idle), nat.body_period(ids[i], spots[i], engaged, idle)]
			queries += 1
			if a != b:
				mismatches += 1
				first = first if first != "" else "round %d bot %d: %s vs %s" % [round, i, a, b]
		var da := gd.describe()
		var db := nat.describe()
		if da.counts != db.counts or da.body_counts != db.body_counts or da.pinned != db.pinned:
			mismatches += 1
			first = first if first != "" else "round %d counts: %s vs %s" % [round, da, db]
	_check(mismatches == 0, "%d rounds, %d bots: schedules, periods, body periods, pins and counts the same, every tick native (native=false keeps the script)" % [TIER_ROUNDS, queries], first)
	_check(on > TIER_ROUNDS / 4 and on < TIER_ROUNDS, "the rounds mix tiers on and off", str(on))


func _spot(rng: RandomNumberGenerator) -> Vector3:
	return Vector3(rng.randf_range(-70, 70), rng.randf_range(-4, 4), rng.randf_range(-70, 70))


## The cost of one scan, script and native, over the same worlds.
func _bench_scan() -> void:
	print("sight scan cost")
	var rng := RandomNumberGenerator.new()
	rng.seed = 3
	var bodies := _bodies(rng, 32)
	var world := _world(bodies)
	world.contacts_version = 1
	var p := DotBotProfile.new()
	var brain := DotBotBrain.new(1)
	var forward := DotBotAim.direction(30.0, 0.0)
	var reps := 20000
	var sc := PackedFloat32Array()
	var t0 := Time.get_ticks_usec()
	for i in reps:
		sc.clear()
		brain._candidates(bodies[i % 32], bodies, p, (bodies[i % 32] as DotBotContact).eye(), forward, p.fov * 0.5, sc)
	var gd := float(Time.get_ticks_usec() - t0) / reps
	var senses := world.senses(bodies)
	t0 = Time.get_ticks_usec()
	for i in reps:
		sc.clear()
		brain._candidates_native(senses, world, bodies[i % 32], p, (bodies[i % 32] as DotBotContact).eye(), forward, p.fov * 0.5, sc)
	var nat := float(Time.get_ticks_usec() - t0) / reps
	print("  32 bodies: script %.2f us/scan, native %.2f us/scan (%.1fx)" % [gd, nat, gd / maxf(nat, 0.001)])
	_check(nat < gd, "the native scan is cheaper than the script one")


func _bodies(rng: RandomNumberGenerator, n: int) -> Array:
	var out := []
	for i in n:
		var c := DotBotContact.make(i + 1, Vector3(rng.randf_range(-40, 40), rng.randf_range(-2, 6), rng.randf_range(-40, 40)), rng.randi_range(0, 2))
		c.alive = rng.randf() > 0.1
		c.aim_height = rng.randf_range(0.8, 1.4)
		c.eye_height = rng.randf_range(1.2, 1.8)
		out.append(c)
	return out


func _world(bodies: Array) -> DotBotWorld:
	var w := DotBotWorld.new()
	w.contacts_fn = func() -> Array: return bodies
	return w


func _ids(list: Array) -> String:
	var ids := PackedStringArray()
	for c: DotBotContact in list:
		ids.append(str(c.id))
	return ",".join(ids)


## dot-player-char's clearance scan, linked for the arms parity test (see README); absent, the test is skipped.
const ARMS_SCRIPT := "res://examples/linked/dot_view_arms_clear.gd"

## Random arm poses compared one by one.
const ARM_CASES := 4000


func _test_arms_parity() -> void:
	print("arms")
	if not ResourceLoader.exists(ARMS_SCRIPT):
		print("  skip  %s not linked" % ARMS_SCRIPT)
		return
	var clear: Script = load(ARMS_SCRIPT)
	var native: Object = ClassDB.instantiate(&"DotSimViewArms")
	var rng := RandomNumberGenerator.new()
	rng.seed = 20261113
	var same := 0
	var buried := 0
	var first_miss := ""
	for i: int in ARM_CASES:
		var c := _arm_case(rng)
		var want: PackedFloat64Array = clear.scan(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8])
		var got: PackedFloat64Array = native.clear_pole(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8])
		if got == want:
			same += 1
		elif first_miss == "":
			first_miss = "case %d: script %s native %s" % [i, want, got]
		if want[4] > 0.0 or want[3] != c[8] + (0.0 - c[8]) * 0.35:
			buried += 1
	_check(same == ARM_CASES, "%d random arm poses: the native scan matches the script to the bit" % ARM_CASES, first_miss)
	_check(buried > ARM_CASES / 4, "the poses reach the box (%d of %d turn or stay buried)" % [buried, ARM_CASES])
	var t0 := Time.get_ticks_usec()
	for i: int in 2000:
		var c := _arm_case(rng)
		clear.scan(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8])
	var t1 := Time.get_ticks_usec()
	for i: int in 2000:
		var c := _arm_case(rng)
		native.clear_pole(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8])
	var t2 := Time.get_ticks_usec()
	print("  bench 2000 scans (with case setup): script %d us, native %d us" % [t1 - t0, t2 - t1])


## One random arm: shoulder, hand target, bone lengths, pole, unit shoulder-to-hand axis, a rigid
## anchor transform, and a box in anchor space set around the arm so most poses touch it.
func _arm_case(rng: RandomNumberGenerator) -> Array:
	var shoulder := Vector3(rng.randf_range(-0.3, 0.3), rng.randf_range(1.2, 1.6), rng.randf_range(-0.2, 0.2))
	var lengths := Vector2(rng.randf_range(0.2, 0.35), rng.randf_range(0.2, 0.35))
	var reach := rng.randf_range(0.05, 1.05) * (lengths.x + lengths.y)
	var aim := Vector3(rng.randf_range(-1, 1), rng.randf_range(-1, 1), rng.randf_range(-1, 1))
	if aim.length_squared() < 0.01:
		aim = Vector3.FORWARD
	var target := shoulder + aim.normalized() * reach
	var dir := (target - shoulder).normalized()
	var pole := Vector3(rng.randf_range(-1, 1), rng.randf_range(-1, 1), rng.randf_range(-1, 1))
	var axis := Vector3(rng.randf_range(-1, 1), rng.randf_range(-1, 1), rng.randf_range(-1, 1))
	axis = axis.normalized() if axis.length_squared() > 0.01 else Vector3.UP
	var to_anchor := Transform3D(Basis(axis, rng.randf_range(-PI, PI)), Vector3(rng.randf_range(-2, 2), rng.randf_range(-2, 2), rng.randf_range(-2, 2)))
	var mid := to_anchor * shoulder.lerp(target, rng.randf_range(0.0, 1.0))
	var size := Vector3(rng.randf_range(0.02, 0.4), rng.randf_range(0.02, 0.4), rng.randf_range(0.02, 0.4))
	var box := AABB(mid - size * rng.randf_range(0.0, 1.0), size)
	var up := Vector3(rng.randf_range(-0.2, 0.2), 1.0, rng.randf_range(-0.2, 0.2)).normalized()
	return [shoulder, target, lengths, pole, dir, to_anchor, box, up, rng.randf_range(-80.0, 80.0)]


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
