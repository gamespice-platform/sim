extends RefCounted

## sim_selftest's checks of the exact net record: GSSimFpsNetPack and GSSimNetRest with fourteen and sixteen floats.
## Each check goes to the suite's [param check] (ok, what, detail).


static func run(check: Callable) -> void:
	if ClassDB.class_exists(&"GSSimFpsNetPack"):
		_exact_timed(check)
	if ClassDB.class_exists(&"GSSimNetRest"):
		_rest_exact_floats(check)


## GSSimFpsNetPack.unpack_exact with a sixteen-float record (GSFpsNetSync.EXACT_FLOATS_TIMED, API 58) against the same call with its fourteen-float prefix: the same columns to the bit, but slide time (17) and wall-run time (22) widened from floats 14 and 15, as push_exact's setters do. Lengths 13 and 15 write nothing past unpack.
static func _exact_timed(check: Callable) -> void:
	const SLIDE_TIME := 17
	const WALL_RUN_TIME := 22
	const STRIDE := 33
	var pack: Object = ClassDB.instantiate(&"GSSimFpsNetPack")
	check.call(pack.has_method(&"exact_floats_timed") and int(pack.call(&"exact_floats_timed")) == 16, "GSSimFpsNetPack takes a sixteen-float exact record")
	var cols := PackedFloat64Array()
	cols.resize(STRIDE)
	for i in STRIDE:
		cols[i] = 0.25 * i
	var exact := PackedFloat32Array([1.5, -2.25, 3.1, 0.1, -9.8, 7.0, 0.5, 0.33, 0.07, 1.9, 0.6, 0.4, 1.0, 0.2, 0.125, 7.0 / 66.0])
	var args := [cols, Vector3(1, 2, 3), Vector3(0, 1, 0), 0.5, 0, -1, 1.0]
	var timed: PackedFloat64Array = pack.callv(&"unpack_exact", args + [exact])
	var fourteen: PackedFloat64Array = pack.callv(&"unpack_exact", args + [exact.slice(0, 14)])
	var plain: PackedFloat64Array = pack.callv(&"unpack", args)
	check.call(timed.size() == plain.size() and fourteen.size() == plain.size(), "unpack_exact returns a whole state", "%d %d" % [timed.size(), fourteen.size()])
	if timed.size() != plain.size() or fourteen.size() != plain.size():
		return
	check.call(timed[SLIDE_TIME] == float(exact[14]) and timed[WALL_RUN_TIME] == float(exact[15]), "a timed record writes slide and wall-run time", "%s %s" % [timed[SLIDE_TIME], timed[WALL_RUN_TIME]])
	check.call(fourteen[SLIDE_TIME] == plain[SLIDE_TIME] and fourteen[WALL_RUN_TIME] == plain[WALL_RUN_TIME], "a fourteen-float record leaves them to unpack")
	var same := true
	for i in plain.size():
		if i != SLIDE_TIME and i != WALL_RUN_TIME and timed[i] != fourteen[i]:
			same = false
	check.call(same, "a timed record writes every other column as the fourteen do")
	for n: int in [13, 15]:
		var odd: PackedFloat64Array = pack.callv(&"unpack_exact", args + [exact.slice(0, n)])
		check.call(odd == plain, "a %d-float record writes nothing past unpack" % n)


## GSSimNetRest: fourteen floats until told sixteen; no other length.
static func _rest_exact_floats(check: Callable) -> void:
	var rest: Object = ClassDB.instantiate(&"GSSimNetRest")
	check.call(int(rest.call(&"exact_floats")) == 14, "GSSimNetRest writes fourteen exact floats by default")
	if not rest.has_method(&"set_exact_floats"):
		check.call(false, "GSSimNetRest has set_exact_floats")
		return
	check.call(bool(rest.call(&"set_exact_floats", 16)) and int(rest.call(&"exact_floats")) == 16, "GSSimNetRest switches to sixteen")
	check.call(not bool(rest.call(&"set_exact_floats", 15)) and int(rest.call(&"exact_floats")) == 16, "GSSimNetRest refuses fifteen")
	check.call(bool(rest.call(&"set_exact_floats", 14)) and int(rest.call(&"exact_floats")) == 14, "GSSimNetRest switches back to fourteen")
