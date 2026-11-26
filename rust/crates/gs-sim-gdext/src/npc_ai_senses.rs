//! `GSSimNpcAiSenses`: one NPC's senses state ([`gs_sim_npcai::Mind`]), held natively.
//!
//! A `GSNpcAiMemory` owns one, so a brain's think hands over only what changed (the target's
//! position, the hurt since the last think) and not the state it is compared with. One object per
//! NPC rather than one shared store: brains may think on worker threads, and a shared object would
//! be bound by two of them at once.
//! Entries come back as [code][position.x, .y, .z, velocity.x, .y, .z, first_seen, last_known,
//! last_sighted, source, hurt_me][/code], which the memory copies into its `Entry`.

use godot::prelude::*;
use gs_sim_math::Vec3;
use gs_sim_npcai::Mind;
use gs_sim_npcai::damage::DamageSense;
use gs_sim_npcai::memory::{Entry, Settings};

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn push_entry(out: &mut Vec<f64>, e: &Entry<StringName>) {
    out.extend_from_slice(&[
        f64::from(e.position.x),
        f64::from(e.position.y),
        f64::from(e.position.z),
        f64::from(e.velocity.x),
        f64::from(e.velocity.y),
        f64::from(e.velocity.z),
        e.first_seen,
        e.last_known,
        e.last_sighted,
        e.source as i64 as f64,
        e.hurt_me,
    ]);
}

fn snapshot(e: Option<&Entry<StringName>>) -> PackedFloat64Array {
    let mut out = Vec::with_capacity(11);
    if let Some(e) = e {
        push_entry(&mut out, e);
    }
    PackedFloat64Array::from(out.as_slice())
}

/// One NPC's memory of its enemies and its window of recent hurt.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimNpcAiSenses {
    mind: Mind<StringName>,
}

#[godot_api]
impl GSSimNpcAiSenses {
    /// The memory's tuning: `forget_after`, `max_lead`, `max_lead_distance`, `velocity_window`.
    #[func]
    fn settings(
        &mut self,
        forget_after: f64,
        max_lead: f64,
        max_lead_distance: f64,
        velocity_window: f64,
    ) {
        self.mind.memory.settings = Settings {
            forget_after,
            max_lead,
            max_lead_distance,
            velocity_window,
        };
    }

    /// One think's senses, `GSNpcAiBrain._sense`'s native part: [param target] sighted at [param at]
    /// (skipped when [param at] is [code]Vector3.INF[/code]), then the damage window with [param pending]
    /// hurt since the last think. Answers the LIGHT_DAMAGE and HEAVY_DAMAGE bits first, then the
    /// sighted entry when there was one.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn think(
        &mut self,
        target: StringName,
        at: Vector3,
        now: f64,
        pending: f64,
        max_health: f64,
        window: f64,
        fraction: f64,
    ) -> PackedFloat64Array {
        let mut out = Vec::with_capacity(12);
        out.push(0.0);
        if at != Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY) {
            let e = self.mind.memory.sighted(&target, v3(at), now);
            push_entry(&mut out, e);
        }
        let bits = self.mind.damage.sense(DamageSense {
            now,
            pending,
            window,
            max_health,
            fraction,
        });
        out[0] = bits as f64;
        PackedFloat64Array::from(out.as_slice())
    }

    /// `GSNpcAiMemory.sighted`; the entry after.
    #[func]
    fn sighted(&mut self, id: StringName, position: Vector3, now: f64) -> PackedFloat64Array {
        snapshot(Some(self.mind.memory.sighted(&id, v3(position), now)))
    }

    /// `GSNpcAiMemory.heard`; the entry after.
    #[func]
    fn heard(&mut self, id: StringName, position: Vector3, now: f64) -> PackedFloat64Array {
        snapshot(Some(self.mind.memory.heard(&id, v3(position), now)))
    }

    /// `GSNpcAiMemory.informed`: the entry after when the report was news, else empty.
    #[func]
    fn informed(
        &mut self,
        id: StringName,
        position: Vector3,
        velocity: Vector3,
        seen_at: f64,
        now: f64,
    ) -> PackedFloat64Array {
        let memory = &mut self.mind.memory;
        snapshot(memory.informed(&id, v3(position), v3(velocity), seen_at, now))
    }

    /// `GSNpcAiMemory.hurt_by`; the entry after.
    #[func]
    fn hurt_by(
        &mut self,
        id: StringName,
        position: Vector3,
        amount: f64,
        now: f64,
    ) -> PackedFloat64Array {
        let memory = &mut self.mind.memory;
        snapshot(Some(memory.hurt_by(&id, v3(position), amount, now)))
    }

    /// `GSNpcAiMemory.get_entry`: empty once forgotten.
    #[func]
    fn get_entry(&mut self, id: StringName, now: f64) -> PackedFloat64Array {
        snapshot(self.mind.memory.get_entry(&id, now))
    }

    /// The ids of every entry still remembered, in order.
    #[func]
    fn ids(&mut self, now: f64) -> Array<StringName> {
        let mut out = Array::new();
        for e in self.mind.memory.entries(now) {
            out.push(&e.id);
        }
        out
    }

    /// `GSNpcAiMemory.forget`.
    #[func]
    fn forget(&mut self, id: StringName) {
        self.mind.memory.forget(&id);
    }

    /// `GSNpcAiMemory.clear`: the entries and the damage window.
    #[func]
    fn clear(&mut self) {
        self.mind.memory.clear();
        self.mind.damage.clear();
    }

    /// Entries held, expired or not.
    #[func]
    fn size(&self) -> i64 {
        self.mind.memory.len() as i64
    }

    /// Hits still in the damage window.
    #[func]
    fn hits(&self) -> i64 {
        self.mind.damage.len() as i64
    }
}
