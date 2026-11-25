class_name GSSim
extends RefCounted

## Which GameSpice Sim modules are loaded. Nothing here needs the native library: on the web, or with
## no library installed, every answer is "absent" and callers keep their script path.

# No log channel: it only answers questions.

## Module name to the native class it registers.
const MODULES := {
	&"bots": &"GSSimBotSenses",
	&"snapshot": &"GSSimSnapshotCodec",
	&"interp": &"GSSimInterpolator",
	&"motor": &"GSSimFpsBody",
	&"arms": &"GSSimViewArms",
	&"audio": &"GSSimAudioSteps",
	&"ragdoll": &"GSSimRagdollPose",
	&"killview": &"GSSimKillViewTrack",
	&"crowd": &"GSSimCrowdGrid",
	&"weapon": &"GSSimWeaponSpread",
	&"senses": &"GSSimSenses",
	&"zombie": &"GSSimZombieBrains",
	&"doors": &"GSSimDoorReach",
	&"grid2d": &"GSSimGrid2D",
	&"witches": &"GSSimWitches",
	&"doubt": &"GSSimPropDoubt",
	&"seek": &"GSSimHunterSeek",
	&"body_sight": &"GSSimBodySight",
	&"hazards": &"GSSimHazards",
	&"road": &"GSSimRoad",
	&"sight": &"GSSimSightRay",
	&"pvs": &"GSSimPvs",
	&"route": &"GSSimRouteBots",
	&"crc": &"GSSimCrc",
	&"interest": &"GSSimInterestGrid",
	&"trace": &"GSSimFlatTrace",
	&"players": &"GSSimPlayerPost",
	&"present": &"GSSimPresentLod",
}


## Whether module [param module] (a key of [constant MODULES]) is loaded.
static func has(module: StringName) -> bool:
	var cls: StringName = MODULES.get(module, &"")
	return cls != &"" and ClassDB.class_exists(cls)


## The loaded modules' names.
static func loaded() -> Array[StringName]:
	var out: Array[StringName] = []
	for m: StringName in MODULES:
		if has(m):
			out.append(m)
	return out
