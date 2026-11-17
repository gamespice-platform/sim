//! The GDExtension: thin engine bindings over the pure crates, one cargo feature per module.

use godot::prelude::*;

#[cfg(feature = "arms")]
mod arms;
#[cfg(feature = "audio")]
mod audio;
#[cfg(feature = "motor")]
mod batch;
#[cfg(feature = "bots")]
mod bots;
#[cfg(feature = "brain")]
mod brain;
#[cfg(feature = "crowd")]
mod crowd;
#[cfg(feature = "doors")]
mod doors;
#[cfg(feature = "grid2d")]
mod grid2d;
#[cfg(feature = "motor")]
mod hulls;
#[cfg(feature = "interp")]
mod interp;
#[cfg(feature = "killview")]
mod killview;
#[cfg(feature = "motor")]
mod motor;
#[cfg(feature = "zombie")]
mod pace;
#[cfg(feature = "ragdoll")]
mod ragdoll;
#[cfg(feature = "senses")]
mod senses;
#[cfg(feature = "snapshot")]
mod snapshot;
#[cfg(feature = "bots")]
mod tier;
#[cfg(feature = "weapon")]
mod weapon;
#[cfg(feature = "zombie")]
mod zombie;

struct DotSimExtension;

#[gdextension]
unsafe impl ExtensionLibrary for DotSimExtension {}
