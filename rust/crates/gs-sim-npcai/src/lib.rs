//! GameSpice NPC AI's per-NPC senses state in Rust, so a brain's think does not rebuild it in script.
//!
//! [`memory::Memory`] is `GSNpcAiMemory` (the enemies an NPC knows of, how, and where they went) and
//! [`damage::DamageLog`] is `GSNpcAiBrain._sense_damage`'s window of recent hurt. Both follow the
//! script in its order and precision: positions and velocities are engine `f32` vectors, clocks and
//! amounts the script's `f64`. The key type is generic so this crate needs no engine type.

pub mod damage;
pub mod memory;

/// `GSNpcAiConditions.LIGHT_DAMAGE`: hurt since the last think.
pub const LIGHT_DAMAGE: i64 = 1 << 4;
/// `GSNpcAiConditions.HEAVY_DAMAGE`: the window's hurt reached the heavy fraction of its health.
pub const HEAVY_DAMAGE: i64 = 1 << 5;

/// One NPC's senses state: what it remembers and what has hurt it lately.
#[derive(Clone, Debug)]
pub struct Mind<K> {
    pub memory: memory::Memory<K>,
    pub damage: damage::DamageLog,
}

impl<K> Default for Mind<K> {
    fn default() -> Self {
        Self {
            memory: memory::Memory::default(),
            damage: damage::DamageLog::default(),
        }
    }
}
