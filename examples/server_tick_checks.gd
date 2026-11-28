extends RefCounted

## sim_selftest's checks of GSSimServerTick.pre. Each check goes to the suite's [param check] (ok, what, detail).


## GSSimServerTick.pre against FC's body gate and ladder wish written in script (FcPlayerBody.body_delta,
## tick_pre with GSFpsMotor._view_basis at pitch 0): every row the same to the bit.
static func run(check: Callable) -> void:
	const ROWS := 4000
	const MAX_GAP := 8
	const GROUND := 0
	var st: Object = ClassDB.instantiate(&"GSSimServerTick")
	st.call(&"configure", GROUND, 0.4)
	var stride: int = st.get(&"PRE_IN")
	var rng := RandomNumberGenerator.new()
	rng.seed = 2611
	var rows := PackedFloat64Array()
	rows.resize(ROWS * stride)
	var want := PackedFloat64Array()
	for i: int in ROWS:
		var tick := rng.randi_range(0, 40)
		var last := rng.randi_range(-2, 44)
		var every := rng.randi_range(1, 4)
		var delta := 1.0 / rng.randi_range(30, 128)
		var buttons := rng.randi_range(0, 3)
		var prev := buttons if rng.randf() < 0.8 else rng.randi_range(0, 3)
		var ext := 1.0 if rng.randf() < 0.1 else 0.0
		var mode := GROUND if rng.randf() < 0.8 else rng.randi_range(1, 4)
		var slide := -1.0 if rng.randf() < 0.9 else rng.randf()
		var wall := -1.0 if rng.randf() < 0.9 else rng.randf()
		var yaw := rng.randf_range(-720.0, 720.0)
		var mv := Vector2(rng.randf_range(-1.0, 1.0), rng.randf_range(-1.0, 1.0))
		if rng.randf() < 0.1:
			mv = Vector2.ZERO
		var row := [tick, last, every, delta, buttons, prev, ext, mode, slide, wall, yaw, mv.x, mv.y]
		for j: int in stride:
			rows[i * stride + j] = float(row[j])
		# FcPlayerBody.body_delta.
		var gap := tick - last
		var kind := 2.0
		var d := 0.0
		if last < 0 or gap <= 0 or gap > MAX_GAP:
			kind = 0.0
			d = delta
		elif gap >= every or buttons != prev or ext != 0.0 or mode != GROUND or slide >= 0.0 or wall >= 0.0:
			kind = 1.0
			d = delta * float(gap)
		# FcPlayerBody.tick_pre's ladder wish.
		var r := deg_to_rad(yaw)
		var forward := Vector3(-sin(r) * cos(0.0), sin(0.0), -cos(r) * cos(0.0))
		var right := Vector3(cos(r), 0.0, -sin(r))
		var intent: Vector3 = right * mv.x + forward * mv.y
		var w := Vector3(intent.x, 0.0, intent.z).normalized()
		want.append_array([kind, d, w.x, w.z])
	var got: PackedFloat64Array = st.call(&"pre", rows, ROWS, MAX_GAP)
	var first := -1
	for i: int in want.size():
		if got.size() != want.size() or got[i] != want[i]:
			first = i
			break
	check.call(first < 0, "GSSimServerTick.pre: %d rows of body gate and ladder wish match the script to the bit" % ROWS,
			"" if first < 0 else "value %d: %s vs %s" % [first, got[first] if first < got.size() else "missing", want[first]])
	var kinds := [0, 0, 0]
	for i: int in ROWS:
		kinds[int(want[i * 4])] += 1
	check.call(kinds[0] > 0 and kinds[1] > 0 and kinds[2] > 0, "the rows reset, run and skip", str(kinds))
	var counters: PackedInt64Array = st.call(&"counters")
	# pre rows and skips: values 9 and 10 (GSSimServerTick.counters).
	check.call(counters[9] == ROWS and counters[10] == kinds[2],
			"the counters count the rows and the skips", str(counters))
