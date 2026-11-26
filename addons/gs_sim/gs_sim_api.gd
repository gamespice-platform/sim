extends RefCounted

## GameSpice Sim's API level. The rule for bumping it is on [GSAddonApi].
##
## LEVEL rises by one for anything a game could call that did not exist before. OLDEST is
## raised to LEVEL when something a game could have called is removed or changes meaning,
## because every pack built before that no longer compiles against this addon.

# 1: everything before this file existed.
# 2: GSSimHitboxRay.nearest (one set's hitbox pick) and nearest_in_sets (a whole ray's walk
#    over every set: bounds, enabled, exclusion, hitboxes, ties to the wall).
const LEVEL := 2
const OLDEST := 1
