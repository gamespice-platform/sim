//! `GSSimCrc`: gs-map's `GSMapVpk.crc32` natively (slicing-by-8 in Rust, a GDScript one
//! stays the fallback). Stateless; the same value as the script.

use godot::prelude::*;

/// CRC-32 (zlib's) of a byte array.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimCrc {}

#[godot_api]
impl GSSimCrc {
    /// The CRC-32 of [param data], as the script returns it (an unsigned 32-bit value in an int).
    #[func]
    fn crc32(&self, data: PackedByteArray) -> i64 {
        i64::from(gs_sim_crc::crc32(data.as_slice()))
    }
}
