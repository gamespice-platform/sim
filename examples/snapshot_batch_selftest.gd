extends "res://examples/snapshot_selftest.gd"
## GameSpice Net's snapshot batch (GSSimSnapshotBatch) against the script's per-entity apply,
## with the snapshot suite's wire helpers. Needs GameSpice Net linked at `addons/gs_net`.


## A garden toy as Prop Hunt sends it: data only, so the snapshot batch holds it.
class Toy extends GSNetBehaviour:
	var pos := Vector3.ZERO
	var count := 0
	var applied := 0

	func _register_net_vars() -> void:
		replicate(&"pos", GSNetVar.Type.VECTOR3_POSITION).interpolated()
		replicate(&"count", GSNetVar.Type.UINT).bits(8)

	func _net_state_applied(_tick: int) -> void:
		applied += 1


func _run() -> void:
	print("gs-sim snapshot batch self-test")
	if ClassDB.class_exists(&"GSSimSnapshotBatch") and ClassDB.class_exists(&"GSSimInterpolator"):
		_test_batch_reregistered()
	else:
		print("  GSSimSnapshotBatch or GSSimInterpolator absent: nothing to compare")
	print("")
	print("%d passed, %d failed" % [_passed, _failed])
	get_tree().quit(1 if _failed > 0 else 0)


## A client drops two held entities and registers new ones under their net ids without the spawner
## (Prop Hunt's toy mirrors on a new round), one freed at once, one still alive: the batch must read
## the registry's identity as the script does, or the new copies are never told anything.
func _test_batch_reregistered() -> void:
	print("snapshot batch: an entity registered again under its net id")
	var off := _batch_run(false)
	var on := _batch_run(true)
	_check(int(on["held"]) > 0 and int(off["held"]) == 0, "the batch held the toys (%d), the script run none" % on["held"])
	_check(bool(on["told"]) and bool(off["told"]), "both runs tell the new copies where the toys are")
	_check(on["state"] == off["state"], "every toy's received values, properties and hook count equal, batch on and off",
		"off %s on %s" % [off["state"], on["state"]])


func _toy_body(_context: Dictionary) -> Node:
	var body := Node3D.new()
	body.name = "ToyBody"
	var identity := GSNetIdentity.new()
	identity.name = "NetIdentity"
	identity.prefab_id = &"toy"
	identity.authority = GSNetIdentity.Authority.SERVER
	identity.interest_linger_sec = 0.0
	body.add_child(identity)
	var toy := Toy.new()
	toy.name = "Toy"
	body.add_child(toy)
	identity.entity = body
	identity.behaviours = [toy]
	toy.identity = identity
	toy._bind(GSNetConfig.new())
	return body


func _batch_run(batch: bool) -> Dictionary:
	const TOYS := 4
	const TICKS := 60
	const AGAIN := 20
	GSNetSnapshotBatch.enabled = batch
	var tag := "On" if batch else "Off"
	var host := _wire_manager("BatchServer" + tag, true, 1, 30, 30)
	var client := _wire_manager("BatchClient" + tag, false, 2, 30, 30)
	var _s := host.setup()
	var _c := client.setup()
	host.send_fn = func(peer_id: int, payload: PackedByteArray, d: int) -> void:
		if peer_id != 2:
			return
		if d != GSNetMessage.Delivery.UNRELIABLE:
			var _r := client.receive(payload, 1)
			return
		var result := client.receive_snapshot(payload)
		if result.ok:
			var _a := host.receive_ack_payload(2, client.encode_ack())
	host.start()
	client.start()
	client.spawner.register_factory(&"toy", _toy_body)
	host.spawner.register_factory(&"toy", _toy_body)
	host.add_peer(2)
	var toys: Array[Toy] = []
	for i in TOYS:
		var made: GSNetIdentity = host.spawner.spawn(&"toy", 0, Transform3D.IDENTITY, 0).value
		toys.append(made.behaviours[0] as Toy)
		var _m := client.spawner.spawn_remote(&"toy", made.net_id, 0, Transform3D.IDENTITY, 0)
	var held := 0
	var kept: Array[Node] = []
	for t in range(1, TICKS + 1):
		for i in toys.size():
			toys[i].pos = Vector3(float(i) + float(t) * 0.25, 0.5, -float(t) * 0.125)
			if (t + i) % 7 == 0:
				toys[i].count = (toys[i].count + 1) & 255
		host.server_tick(t)
		if t == AGAIN:
			if client.interpolator.snapshot_batch != null:
				held = client.interpolator.snapshot_batch.size()
			# Toy 0 freed at once, toy 1 left alive out of the registry: both registered anew.
			for i in 2:
				var net_id: int = toys[i].identity.net_id
				var old := client.registry.get_identity(net_id).entity as Node
				var _u := client.registry.unregister(net_id)
				old.get_parent().remove_child(old)
				if i == 0:
					old.free()
				else:
					kept.append(old)
				var _m := client.spawner.spawn_remote(&"toy", net_id, 0, Transform3D.IDENTITY, t)
	var state := []
	var told := true
	for i in toys.size():
		var toy := client.registry.get_identity(toys[i].identity.net_id).behaviours[0] as Toy
		state.append([toy.received_values(), toy.pos, toy.count, toy.applied])
		told = told and toy.pos.distance_to(toys[i].pos) < 0.5
	for node in kept:
		node.free()
	_wire_free([host, client])
	GSNetSnapshotBatch.enabled = true
	return {"held": held, "state": state, "told": told}
