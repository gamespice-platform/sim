extends RefCounted

## sim_selftest's checks of GSSimShots' rows. Each check goes to the suite's [param check] (ok, what, detail).


## GSSimShots' rows, end to end through the class: a cached box set posed for a shot, a world wall answered by a
## Callable, a round that stops at it and one that passes it (the reverse ray finds the exit). The walk's rules
## themselves are gs-sim-weapon's cargo tests and FC's headless_native_shots (against GSPelletTracer, to the bit).
static func run(check: Callable) -> void:
	var shots: Object = ClassDB.instantiate(&"GSSimShots")
	var ident := [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
	# One set (entity 7, one box): bounds at the root, radius 2, local transform identity.
	var set_row := PackedFloat64Array([7.0, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0] + ident + [0.0, 0.0, 0.0, 0.0])
	# A half-metre cube (shape 1), group 0, never crouch-fitted (stand_y, stand_h NaN), at the set's origin.
	var box_row := PackedFloat64Array([1.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 0.0, 0.0, NAN, NAN] + ident + [0.0, 0.0, 0.0, 0.0])
	check.call(not bool(shots.call(&"set_boxes", set_row, PackedFloat64Array([1.0]))), "GSSimShots refuses box rows that do not add up")
	check.call(bool(shots.call(&"set_boxes", set_row, box_row)) and int(shots.call(&"set_count")) == 1, "GSSimShots caches one set of one box")
	check.call(bool(shots.call(&"set_poses", PackedFloat64Array([1.0] + ident + [0.0, 1.0, -10.0, -1.0]))), "and poses it 10 m ahead")
	var stride := int(ClassDB.class_get_integer_constant(&"GSSimShots", &"STRIDE"))
	var origin := Vector3(0.0, 1.0, 0.0)

	var rows: PackedFloat64Array = shots.call(&"walk", 3, origin, Vector3.FORWARD, 100.0, PackedInt32Array())
	check.call(rows.size() == 2 * stride and rows[0] == 2.0 and rows[1] == 3.0 and rows[8] == 0.0 and is_equal_approx(rows[4], -9.5) and is_equal_approx(rows[10], 9.5)
			and rows[stride] == 1.0 and is_equal_approx(rows[stride + 4], -9.5),
		"an open shot is a body row (pellet 3, set 0, the near face at 9.5 m), then its impact there", str(rows))
	rows = shots.call(&"walk", 0, origin, Vector3.FORWARD, 100.0, PackedInt32Array([0]))
	check.call(rows.size() == stride and rows[0] == 1.0 and is_equal_approx(rows[4], -100.0), "a skipped set is not hit: an impact at full range", str(rows))

	shots.call(&"set_world", _slab)
	shots.call(&"clear_pen")
	rows = shots.call(&"walk", 0, origin, Vector3.FORWARD, 100.0, PackedInt32Array())
	check.call(rows.size() == stride and rows[0] == 1.0 and is_equal_approx(rows[4], -5.0), "a round that cannot penetrate stops at the wall's face", str(rows))

	# power, max_depth, body_cost, speed, reference_speed, min_damage, max_passes, ricochet angle, keep, max, on, players
	shots.call(&"set_pen", PackedFloat64Array([2.0, 1.0, 0.3, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 1.0]), PackedFloat64Array([1.0]))
	rows = shots.call(&"walk", 0, origin, Vector3.FORWARD, 100.0, PackedInt32Array())
	var passed := rows.size() == 3 * stride and rows[0] == 3.0 and is_equal_approx(rows[7], -5.2) and is_equal_approx(rows[15], 0.2)
	var body := rows.size() == 3 * stride and rows[stride] == 2.0 and rows[stride + 12] == 1.0 and rows[stride + 13] == 1.0 and rows[2 * stride] == 1.0
	check.call(passed and body, "a penetrating round passes the 0.2 m wall (exit from the reverse ray), then hits the box after one wall, and stops there", str(rows))


## A 0.2 m wall from z = -5 to -5.2 as GSSimShots' world ray: `[distance, point, normal, material 4, resistance 1,
## ricochet 0, sheet 0, pane 0]`, a face only from outside (as a ray starting inside a convex body sees none).
static func _slab(from: Vector3, dir: Vector3, max_distance: float) -> PackedFloat64Array:
	var plane := 0.0
	var normal := 0.0
	if dir.z < 0.0 and from.z > -5.0:
		plane = -5.0
		normal = 1.0
	elif dir.z > 0.0 and from.z < -5.2:
		plane = -5.2
		normal = -1.0
	else:
		return PackedFloat64Array()
	var t := (plane - from.z) / dir.z
	if t < 0.0 or t > max_distance:
		return PackedFloat64Array()
	var p := from + dir * t
	return PackedFloat64Array([t, p.x, p.y, p.z, 0.0, 0.0, normal, 4.0, 1.0, 0.0, 0.0, 0.0])
