extends Node

## gs-sim's suite (`godot --headless --path . res://examples/sim_selftest.tscn`): each native module against the script it replaces, same inputs, same answers.
## With no library installed it checks that the script fallback runs and says the native half was skipped; CI's native-suites job installs it and sets , so a missing library fails.

const CHANNEL := "gs.sim.selftest"

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
	GSLog.set_level(GSLog.Level.ERROR)
	_run.call_deferred()


func _run() -> void:
	print("gs-sim self-test")
	var native := GSSim.has(&"bots")
	print("  native bots module: %s" % ("loaded" if native else "absent (script fallback only)"))
	if OS.get_environment("GS_SIM_REQUIRE_NATIVE") == "1":
		_check(native, "the library is loaded (GS_SIM_REQUIRE_NATIVE=1)")
	_test_fallback()
	if native:
		_test_scan_parity()
		_test_brain_parity()
		_test_tier_parity()
		_bench_scan()
	if GSSim.has(&"arms"):
		_test_arms_parity()
	if GSSim.has(&"sight"):
		await _test_sight_ray_parity()
		await _test_sight_clear_to_parity()
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


## Random sight rays through GSSimSightRay against the script's PhysicsRayQueryParameters3D ray (same mask, exclusions
## and end point), on the main thread and on a worker holding the lock, after a rebind changes the mask, and with a jail
## cell bound (gs_map's GSMapGlassSight.blocks in front, written out here: glass on its own layer).
func _test_sight_ray_parity() -> void:
	var root := Node3D.new()
	add_child(root)
	var rng := RandomNumberGenerator.new()
	rng.seed = 4242
	var rids := _sight_boxes(root, rng)
	await get_tree().physics_frame
	await get_tree().physics_frame
	var space := root.get_world_3d().direct_space_state
	var exclude: Array[RID] = [rids[1], rids[2], rids[3]]
	var ray: Object = ClassDB.instantiate(&"GSSimSightRay")
	var lock := Mutex.new()
	var cases := _sight_cases(rng)
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


## GSSimSightRay.clear_to against a game's `_clear_between` ray (PhysicsRayQueryParameters3D.create(from, to) with the
## mask set, no exclusions): the end point as given and no short-ray answer, so every ray agrees to the bit.
func _test_sight_clear_to_parity() -> void:
	var root := Node3D.new()
	add_child(root)
	var rng := RandomNumberGenerator.new()
	rng.seed = 5151
	var _rids := _sight_boxes(root, rng)
	await get_tree().physics_frame
	await get_tree().physics_frame
	var space := root.get_world_3d().direct_space_state
	var ray: Object = ClassDB.instantiate(&"GSSimSightRay")
	var cases := _sight_cases(rng)
	var blocked := 0
	for mask: int in [1, 3, 7]:
		ray.call(&"bind", space, mask, [] as Array[RID], null, null, 0, Vector3.ZERO, 0.0, 0.0)
		var miss := -1
		for i in cases.size():
			var q := PhysicsRayQueryParameters3D.create(cases[i][0], cases[i][1])
			q.collision_mask = mask
			var want := space.intersect_ray(q).is_empty()
			var got: bool = ray.call(&"clear_to", cases[i][0], cases[i][1])
			blocked += 0 if got else 1
			if got != want and miss < 0:
				miss = i
		_check(miss < 0, "clear_to, mask %d: %d rays match the script's created ray" % [mask, cases.size()], "" if miss < 0 else "case %d %s" % [miss, cases[miss]])
	_check(blocked > 600, "the clear_to rays mix blocked and clear", str(blocked))
	root.queue_free()


## Sixty static boxes on three layers (1 walls, 2 more walls, 4 glass), scattered over the scene; their RIDs.
func _sight_boxes(root: Node3D, rng: RandomNumberGenerator) -> Array[RID]:
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
	return rids


## 4000 random [from, to] pairs, half starting near the middle, a few too short to cast.
func _sight_cases(rng: RandomNumberGenerator) -> Array:
	var cases: Array = []
	for i in 4000:
		var reach := 30.0 if i % 2 == 0 else 9.0
		var from := Vector3(rng.randf_range(-reach, reach), rng.randf_range(-4.0, 4.0), rng.randf_range(-reach, reach))
		var to := Vector3(rng.randf_range(-30.0, 30.0), rng.randf_range(-4.0, 4.0), rng.randf_range(-30.0, 30.0))
		if i % 50 == 0:
			to = from + Vector3(0.00003, 0.0, 0.00004)
		cases.append([from, to])
	return cases


func _test_fallback() -> void:
	print("fallback")
	var world := _world(_bodies(RandomNumberGenerator.new(), 12))
	world.contacts_version = -1
	_check(world.senses(world.contacts()) == null, "a world that does not bump contacts_version scans in script")
	GSBotWorld.native_enabled = false
	world.contacts_version = 1
	_check(world.senses(world.contacts()) == null, "native_enabled off scans in script")
	GSBotWorld.native_enabled = true
	var thread := Thread.new()
	thread.start(func() -> bool: return world.senses(world.contacts()) == null)
	_check(thread.wait_to_finish(), "a brain on a worker thread scans in script")
	var brain := GSBotBrain.new(1)
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
		var me: GSBotContact = bodies[rng.randi_range(0, bodies.size() - 1)]
		var p := GSBotProfile.new()
		p.fov = rng.randf_range(40.0, 200.0)
		p.sight = rng.randf_range(5.0, 120.0)
		var forward := GSBotAim.direction(rng.randf_range(-180.0, 180.0), rng.randf_range(-85.0, 85.0))
		var target: int = (bodies[rng.randi_range(0, bodies.size() - 1)] as GSBotContact).id if rng.randf() < 0.6 else -1
		var visible := rng.randf() < 0.5
		var a := GSBotBrain.new(me.id)
		var b := GSBotBrain.new(me.id)
		for brain: GSBotBrain in [a, b]:
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
	GSBotWorld.native_enabled = false
	var script_run := _play()
	GSBotWorld.native_enabled = true
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


## The same fight with the sight LOD on (far 20 m, hold 0.15 s): every intent equal, script against native, and the same ray count; the saving is checked on a crowd.
func _test_sight_lod_parity() -> void:
	print("sight LOD: native against script")
	GSBotWorld.native_enabled = false
	var script_run := _play(20.0, 0.15)
	var rays_script_lod := _rays
	var plain := _play()
	var rays_script_plain := _rays
	GSBotWorld.native_enabled = true
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
	# The two fights above play different games once the LOD changes what a bot sees, so their ray totals are not a like-for-like
	# count (it can go either way); the saving is measured on the crowd: here in script, in the bench below native.
	_play(0.0, 0.15, 3, 80)
	var crowd_plain := _rays
	_play(20.0, 0.15, 3, 80)
	_check(_rays < crowd_plain, "the script LOD traces fewer rays than plain reuse for a crowd thinking every third tick", "%d vs %d" % [_rays, crowd_plain])
	_bench_sight_lod()
	_check(plain.size() == TICKS, "the plain run still plays every tick")


## One fight of [constant BOTS] bots, two teams, pillars that block sight; one line of intents per tick.
## [param think_every] 2 or more thinks a bot on every n-th tick only (a crowd's think tier), its last intent kept between; [param cover] is the number of sight-blocking pillars.
func _play(sight_far: float = 0.0, sight_far_reuse: float = 0.15, think_every: int = 1, cover: int = 10) -> PackedStringArray:
	var rng := RandomNumberGenerator.new()
	rng.seed = 11
	var bodies := []
	for i in BOTS:
		var c := GSBotContact.make(100 + i, Vector3(rng.randf_range(-30, 30), 0.0, rng.randf_range(-30, 30)), 1 + i % 2)
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
			if GSBotWorld.segment_hits_sphere(a, b, q, 2.0):
				return false
		return true
	world.waypoints_fn = func() -> PackedVector3Array: return PackedVector3Array([Vector3(-20, 0, -20), Vector3(20, 0, 20), Vector3(-20, 0, 20), Vector3(20, 0, -20)])
	var brains: Array[GSBotBrain] = []
	for c: GSBotContact in bodies:
		brains.append(GSBotBrain.new(c.id))
	var out := PackedStringArray()
	var last := {}
	var dt := 1.0 / RATE
	for t in TICKS:
		var now := t * dt
		world.contacts_version = t
		var line := ""
		# Every bot thinks over the same bodies, then the tick applies them all, as a game does
		# (the bodies may not change between bumps of contacts_version).
		var intents: Array[GSBotIntent] = []
		for i in BOTS:
			var me: GSBotContact = bodies[i]
			var it: GSBotIntent = brains[i].think(me, world, now) if (t + i) % think_every == 0 or t == 0 else last[i]
			last[i] = it
			intents.append(it)
			line += "%d:%s %.4f %.4f %s %s %d|" % [me.id, it.state, it.yaw, it.pitch, it.move, it.fire, it.target_id]
		for i in BOTS:
			var me: GSBotContact = bodies[i]
			var it := intents[i]
			me.view_yaw = it.yaw
			me.view_pitch = it.pitch
			var step := Vector3(it.move.x, 0.0, -it.move.y).rotated(Vector3.UP, deg_to_rad(it.yaw)) * 5.0 * dt
			me.velocity = step / dt
			me.position += step
			if it.fire and it.target_id >= 0:
				var hit := GSBotWorld.find(bodies, it.target_id)
				if hit != null and hit.alive:
					hit.health -= 0.02
					hit.alive = hit.health > 0.0
		for c: GSBotContact in bodies:
			if not c.alive and rng.randf() < 0.02:
				c.alive = true
				c.health = 1.0
		out.append(line)
	return out


## What the LOD saves for a crowd that thinks every third tick: rays and wall time of the native mind, off against on.
func _bench_sight_lod() -> void:
	GSBotWorld.native_enabled = true
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


## GameSpice Bot's think tiers (GSSimThinkTier): random settings, people and bots; every schedule, period and body period equal, and the same counts.
func _test_tier_parity() -> void:
	print("think tiers: native against script")
	var rng := RandomNumberGenerator.new()
	var gd := GSBotThinkTier.new()
	gd.native = false
	var nat := GSBotThinkTier.new()
	var mismatches := 0
	var queries := 0
	var on := 0
	var first := ""
	for round in TIER_ROUNDS:
		for t: GSBotThinkTier in [gd, nat]:
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
	var p := GSBotProfile.new()
	var brain := GSBotBrain.new(1)
	var forward := GSBotAim.direction(30.0, 0.0)
	var reps := 20000
	var sc := PackedFloat32Array()
	var t0 := Time.get_ticks_usec()
	for i in reps:
		sc.clear()
		brain._candidates(bodies[i % 32], bodies, p, (bodies[i % 32] as GSBotContact).eye(), forward, p.fov * 0.5, sc)
	var gd := float(Time.get_ticks_usec() - t0) / reps
	var senses := world.senses(bodies)
	t0 = Time.get_ticks_usec()
	for i in reps:
		sc.clear()
		brain._candidates_native(senses, world, bodies[i % 32], p, (bodies[i % 32] as GSBotContact).eye(), forward, p.fov * 0.5, sc)
	var nat := float(Time.get_ticks_usec() - t0) / reps
	print("  32 bodies: script %.2f us/scan, native %.2f us/scan (%.1fx)" % [gd, nat, gd / maxf(nat, 0.001)])
	_check(nat < gd, "the native scan is cheaper than the script one")


func _bodies(rng: RandomNumberGenerator, n: int) -> Array:
	var out := []
	for i in n:
		var c := GSBotContact.make(i + 1, Vector3(rng.randf_range(-40, 40), rng.randf_range(-2, 6), rng.randf_range(-40, 40)), rng.randi_range(0, 2))
		c.alive = rng.randf() > 0.1
		c.aim_height = rng.randf_range(0.8, 1.4)
		c.eye_height = rng.randf_range(1.2, 1.8)
		out.append(c)
	return out


func _world(bodies: Array) -> GSBotWorld:
	var w := GSBotWorld.new()
	w.contacts_fn = func() -> Array: return bodies
	return w


func _ids(list: Array) -> String:
	var ids := PackedStringArray()
	for c: GSBotContact in list:
		ids.append(str(c.id))
	return ",".join(ids)


## GameSpice Player Char's clearance scan, linked for the arms parity test (see README); absent, the test is skipped.
const ARMS_SCRIPT := "res://examples/linked/gs_view_arms_clear.gd"

## Random arm poses compared one by one.
const ARM_CASES := 4000


func _test_arms_parity() -> void:
	print("arms")
	if not ResourceLoader.exists(ARMS_SCRIPT):
		print("  skip  %s not linked" % ARMS_SCRIPT)
		return
	var clear: Script = load(ARMS_SCRIPT)
	var native: Object = ClassDB.instantiate(&"GSSimViewArms")
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
	_test_arms_hull_parity(clear, native, rng)
	print("  bench 2000 scans (with case setup): script %d us, native %d us" % [t1 - t0, t2 - t1])


## The same comparison with the weapon's convex fit (GSViewArmsClear.dop_planes of a random point cloud), through clear_pole_hull.
func _test_arms_hull_parity(clear: Script, native: Object, rng: RandomNumberGenerator) -> void:
	if not native.has_method(&"clear_pole_hull"):
		_check(false, "the native arms class has clear_pole_hull")
		return
	var same := 0
	var shallower := 0
	var first_miss := ""
	for i: int in ARM_CASES:
		var c := _arm_case(rng)
		var box: AABB = c[6]
		var pts := PackedVector3Array()
		for k in 12:
			pts.append(box.position + box.size * Vector3(rng.randf(), rng.randf(), rng.randf()))
		var planes: PackedFloat32Array = clear.grown(clear.dop_planes(pts), rng.randf_range(0.0, 0.05))
		c[6] = box.grow(0.05)
		var want: PackedFloat64Array = clear.scan(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], planes)
		var got: PackedFloat64Array = native.clear_pole_hull(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], planes)
		if got == want:
			same += 1
		elif first_miss == "":
			first_miss = "case %d: script %s native %s" % [i, want, got]
		var boxed: PackedFloat64Array = clear.scan(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8])
		if want[4] < boxed[4]:
			shallower += 1
	_check(same == ARM_CASES, "%d random arm poses against a convex fit: the native scan matches the script to the bit" % ARM_CASES, first_miss)
	_check(shallower > 0, "the convex fit is shallower than the box on some poses (%d of %d)" % [shallower, ARM_CASES])


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
