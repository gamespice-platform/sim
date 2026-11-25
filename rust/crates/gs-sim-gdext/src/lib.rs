//! The GDExtension: thin engine bindings over the pure crates, one cargo feature per module.

use godot::prelude::*;

#[cfg(feature = "arms")]
mod arms;
#[cfg(feature = "audio")]
mod audio;
#[cfg(feature = "motor")]
mod batch;
#[cfg(feature = "seek")]
mod body_sight;
#[cfg(feature = "bots")]
mod bots;
#[cfg(feature = "brain")]
mod brain;
mod cell;
#[cfg(feature = "crc")]
mod crc;
#[cfg(feature = "crowd")]
mod crowd;
#[cfg(feature = "doors")]
mod doors;
#[cfg(feature = "doubt")]
mod doubt;
#[cfg(feature = "trace")]
mod flat_trace;
#[cfg(feature = "grid2d")]
mod grid2d;
#[cfg(feature = "hazards")]
mod hazards;
#[cfg(feature = "motor")]
mod hulls;
#[cfg(feature = "interest")]
mod interest;
#[cfg(feature = "interp")]
mod interp;
#[cfg(feature = "killview")]
mod killview;
#[cfg(feature = "motor")]
mod motor;
#[cfg(feature = "zombie")]
mod pace;
#[cfg(feature = "players")]
mod players;
#[cfg(feature = "pose")]
mod pose;
#[cfg(feature = "present")]
mod present;
#[cfg(feature = "pvs")]
mod pvs;
#[cfg(feature = "ragdoll")]
mod ragdoll;
#[cfg(feature = "road")]
mod road;
#[cfg(feature = "route")]
mod route;
#[cfg(feature = "seek")]
mod seek;
#[cfg(feature = "senses")]
mod senses;
#[cfg(feature = "brain")]
mod sight;
#[cfg(feature = "snapshot")]
mod snapshot;
#[cfg(feature = "snapshot_batch")]
mod snapshot_batch;
#[cfg(feature = "bots")]
mod tier;
#[cfg(feature = "weapon")]
mod weapon;
#[cfg(feature = "witches")]
mod witches;
#[cfg(feature = "zombie")]
mod zombie;

struct GSSimExtension;

#[gdextension]
unsafe impl ExtensionLibrary for GSSimExtension {}
