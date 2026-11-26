//! `GSNpcAiMemory`: what an NPC knows about its enemies, how it knows, and where they went.
//!
//! Entries keep the script `Dictionary`'s order (insertion; an expired entry relearned keeps its
//! place; one dropped on read and learned again goes last), since [`Memory::freshest`] takes the
//! first of equals. Forgetting is on read, from the last time anything was learned.

use gs_sim_math::{Vec3, clampf, lerp_f32, maxf};

/// `GSNpcAiMemory.Source`: how an entry was last learned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Sight = 0,
    Sound = 1,
    Squad = 2,
    Damage = 3,
}

/// One remembered enemy, `GSNpcAiMemory.Entry`.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<K> {
    pub id: K,
    pub position: Vec3,
    /// Smoothed from successive sightings; zero until two of them.
    pub velocity: Vec3,
    pub first_seen: f64,
    /// When anything was last learned about it: what forgetting counts from.
    pub last_known: f64,
    /// When this NPC last saw it itself; `-inf` if never.
    pub last_sighted: f64,
    pub source: Source,
    /// Damage it has done to this NPC, ever.
    pub hurt_me: f64,
}

impl<K> Entry<K> {
    fn new(id: K, now: f64) -> Self {
        Self {
            id,
            position: Vec3::default(),
            velocity: Vec3::default(),
            first_seen: now,
            last_known: 0.0,
            last_sighted: f64::NEG_INFINITY,
            source: Source::Sight,
            hurt_me: 0.0,
        }
    }
}

/// The memory's tuning, the script's exported fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Seconds an entry is kept after the last thing learned about it; 0 or less remembers nothing.
    pub forget_after: f64,
    /// Longest an extrapolation runs for, seconds.
    pub max_lead: f64,
    /// Farthest an extrapolation may move a position, metres.
    pub max_lead_distance: f64,
    /// Sightings closer together than this are averaged for velocity.
    pub velocity_window: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            forget_after: 5.0,
            max_lead: 1.5,
            max_lead_distance: 10.0,
            velocity_window: 0.6,
        }
    }
}

/// Every enemy one NPC remembers.
#[derive(Clone, Debug)]
pub struct Memory<K> {
    pub settings: Settings,
    entries: Vec<Entry<K>>,
}

impl<K> Default for Memory<K> {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            entries: Vec::new(),
        }
    }
}

/// `Vector3.lerp` in `f32`.
fn lerp(from: Vec3, to: Vec3, weight: f32) -> Vec3 {
    Vec3::new(
        lerp_f32(from.x, to.x, weight),
        lerp_f32(from.y, to.y, weight),
        lerp_f32(from.z, to.z, weight),
    )
}

impl<K: Clone + PartialEq> Memory<K> {
    fn find(&self, id: &K) -> Option<usize> {
        self.entries.iter().position(|e| e.id == *id)
    }

    fn expired(&self, e: &Entry<K>, now: f64) -> bool {
        self.settings.forget_after <= 0.0 || now - e.last_known > self.settings.forget_after
    }

    /// `_touch`: the entry for `id`, new when missing or expired, with `last_known` raised to `now`.
    fn touch(&mut self, id: &K, now: f64) -> usize {
        let i = match self.find(id) {
            Some(i) if !self.expired(&self.entries[i], now) => i,
            Some(i) => {
                self.entries[i] = Entry::new(id.clone(), now);
                i
            }
            None => {
                self.entries.push(Entry::new(id.clone(), now));
                self.entries.len() - 1
            }
        };
        let e = &mut self.entries[i];
        e.last_known = maxf(e.last_known, now);
        i
    }

    /// `get_entry`'s index: `None` once forgotten, dropping the expired entry.
    fn index(&mut self, id: &K, now: f64) -> Option<usize> {
        let i = self.find(id)?;
        if self.expired(&self.entries[i], now) {
            self.entries.remove(i);
            return None;
        }
        Some(i)
    }

    /// What this NPC's own eyes saw: `id` at `position`. Two sightings inside the velocity window
    /// average a velocity half old, half new; further apart the velocity is unknown again.
    pub fn sighted(&mut self, id: &K, position: Vec3, now: f64) -> &Entry<K> {
        let window = self.settings.velocity_window;
        let i = self.touch(id, now);
        let e = &mut self.entries[i];
        if e.last_sighted > f64::NEG_INFINITY {
            let dt = now - e.last_sighted;
            if dt > 0.0001 && dt <= window {
                let measured = (position - e.position).div_scalar(dt);
                e.velocity = lerp(e.velocity, measured, 0.5);
            } else if dt > window {
                e.velocity = Vec3::default();
            }
        }
        e.position = position;
        e.last_sighted = now;
        e.source = Source::Sight;
        e
    }

    /// Something heard at `position`, attributed to `id`; never over a sighting from the same moment.
    pub fn heard(&mut self, id: &K, position: Vec3, now: f64) -> &Entry<K> {
        if let Some(i) = self.index(id, now)
            && self.entries[i].last_sighted >= now
        {
            return &self.entries[i];
        }
        let i = self.touch(id, now);
        let e = &mut self.entries[i];
        e.position = position;
        e.source = Source::Sound;
        e
    }

    /// A squadmate's report, taken only when newer than what this NPC knows: the entry after when
    /// taken. Answered as it was written, even when `seen_at` is already too old for a read at `now`.
    pub fn informed(
        &mut self,
        id: &K,
        position: Vec3,
        velocity: Vec3,
        seen_at: f64,
        now: f64,
    ) -> Option<&Entry<K>> {
        if let Some(i) = self.index(id, now)
            && self.entries[i].last_known >= seen_at
        {
            return None;
        }
        let i = self.touch(id, seen_at);
        let e = &mut self.entries[i];
        e.position = position;
        e.velocity = velocity;
        e.source = Source::Squad;
        Some(e)
    }

    /// `id` hurt this NPC from `position`.
    pub fn hurt_by(&mut self, id: &K, position: Vec3, amount: f64, now: f64) -> &Entry<K> {
        let i = self.touch(id, now);
        let e = &mut self.entries[i];
        e.hurt_me += maxf(amount, 0.0);
        if e.last_sighted < now {
            e.position = position;
            e.source = Source::Damage;
        }
        e
    }

    /// The entry for `id`, or `None` once it is forgotten (which drops it).
    pub fn get_entry(&mut self, id: &K, now: f64) -> Option<&Entry<K>> {
        self.index(id, now).map(|i| &self.entries[i])
    }

    /// Drops every expired entry, keeping the order of the rest: what reading each one does.
    fn purge(&mut self, now: f64) {
        let s = self.settings;
        self.entries
            .retain(|e| !(s.forget_after <= 0.0 || now - e.last_known > s.forget_after));
    }

    /// The most recently updated entry; the first of equals.
    pub fn freshest(&mut self, now: f64) -> Option<&Entry<K>> {
        self.purge(now);
        let mut best: Option<&Entry<K>> = None;
        for e in &self.entries {
            if best.is_none_or(|b| e.last_known > b.last_known) {
                best = Some(e);
            }
        }
        best
    }

    /// Every entry still remembered, in order.
    pub fn entries(&mut self, now: f64) -> &[Entry<K>] {
        self.purge(now);
        &self.entries
    }

    /// `predict_entry`: `entry`'s position led horizontally by its velocity for up to
    /// `max_lead` seconds times `lead` (0..1), moved at most `max_lead_distance`.
    pub fn predict_entry(&self, entry: &Entry<K>, now: f64, lead: f64) -> Vec3 {
        let s = &self.settings;
        let flat = Vec3::new(entry.velocity.x, 0.0, entry.velocity.z);
        let seconds = clampf(now - entry.last_known, 0.0, s.max_lead) * clampf(lead, 0.0, 1.0);
        let mut offset = flat.scale(seconds);
        if f64::from(offset.length()) > s.max_lead_distance {
            offset = offset.normalized().scale(s.max_lead_distance);
        }
        entry.position + offset
    }

    /// Forgets `id`.
    pub fn forget(&mut self, id: &K) {
        if let Some(i) = self.find(id) {
            self.entries.remove(i);
        }
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Entries held, expired or not (the script's `size`).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is held.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, z: f32) -> Vec3 {
        Vec3::new(x, 0.0, z)
    }

    #[test]
    fn sightings_give_a_velocity_and_a_prediction() {
        let mut m: Memory<&str> = Memory::default();
        m.sighted(&"a", v(0.0, 0.0), 0.0);
        m.sighted(&"a", v(1.0, 0.0), 0.25);
        let e = m.sighted(&"a", v(2.0, 0.0), 0.5).clone();
        // 4 m/s measured twice from zero: 2 then 3.
        assert_eq!(e.velocity.x, 3.0);
        assert_eq!(m.predict_entry(&e, 1.5, 1.0).x, 2.0 + 3.0);
        assert_eq!(m.predict_entry(&e, 1.5, 0.0).x, 2.0);
        assert_eq!(m.predict_entry(&e, 9.0, 1.0).x, 2.0 + 4.5);
        // Too far apart for a velocity.
        assert_eq!(m.sighted(&"a", v(3.0, 0.0), 1.5).velocity, Vec3::default());
    }

    #[test]
    fn the_prediction_is_capped_in_distance() {
        let mut m: Memory<&str> = Memory::default();
        m.sighted(&"a", v(0.0, 0.0), 0.0);
        let e = m.sighted(&"a", v(0.0, 10.0), 0.25).clone();
        assert_eq!(e.velocity.z, 20.0);
        let p = m.predict_entry(&e, 5.0, 1.0);
        assert_eq!(p.z, 20.0);
    }

    #[test]
    fn sounds_squad_and_damage() {
        let mut m: Memory<&str> = Memory::default();
        m.sighted(&"a", v(2.0, 0.0), 0.5);
        assert_eq!(m.heard(&"a", v(50.0, 0.0), 0.5).position.x, 2.0);
        let (at, moving) = (v(9.0, 0.0), v(0.0, 3.0));
        assert!(m.informed(&"a", at, moving, 0.2, 0.6).is_none());
        assert!(m.informed(&"b", at, moving, 0.6, 0.6).is_some());
        assert_eq!(
            m.get_entry(&"b", 0.6).map(|e| e.source),
            Some(Source::Squad)
        );
        assert_eq!(m.freshest(0.6).map(|e| e.id), Some("b"));
        let c = m.hurt_by(&"c", v(-4.0, 0.0), 12.0, 0.7);
        assert_eq!((c.hurt_me, c.source), (12.0, Source::Damage));
        assert!(m.get_entry(&"a", 6.0).is_none());
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn order_is_the_dictionarys() {
        let mut m: Memory<&str> = Memory::default();
        m.sighted(&"a", v(0.0, 0.0), 1.0);
        m.sighted(&"b", v(0.0, 0.0), 1.0);
        // Equals: the first inserted wins.
        assert_eq!(m.freshest(1.0).map(|e| e.id), Some("a"));
        // An expired entry relearned keeps its place.
        m.sighted(&"b", v(0.0, 0.0), 7.0);
        m.sighted(&"a", v(0.0, 0.0), 7.0);
        assert_eq!(
            m.entries(7.0).iter().map(|e| e.id).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(m.entries(7.0)[0].first_seen, 7.0);
        // Dropped on read and learned again: last.
        m.sighted(&"b", v(0.0, 0.0), 9.0);
        assert!(m.get_entry(&"a", 12.5).is_none());
        m.heard(&"a", v(1.0, 1.0), 12.5);
        assert_eq!(
            m.entries(12.5).iter().map(|e| e.id).collect::<Vec<_>>(),
            ["b", "a"]
        );
    }

    #[test]
    fn a_stale_report_is_still_taken() {
        let mut m: Memory<&str> = Memory::default();
        m.settings.forget_after = 0.8;
        // Seen 0.9 s ago: taken and answered, though a read now has already forgotten it.
        let e = m.informed(&"a", v(1.0, 0.0), Vec3::default(), 0.1, 1.0);
        assert_eq!(e.map(|e| e.last_known), Some(0.1));
        assert!(m.get_entry(&"a", 1.0).is_none());
    }

    #[test]
    fn zero_forget_remembers_nothing() {
        let mut m: Memory<&str> = Memory::default();
        m.settings.forget_after = 0.0;
        m.sighted(&"x", Vec3::default(), 1.0);
        assert!(m.get_entry(&"x", 1.0).is_none());
        assert!(m.is_empty());
    }

    #[test]
    fn a_negative_clock_keeps_last_known_at_zero() {
        let mut m: Memory<&str> = Memory::default();
        let e = m.sighted(&"x", Vec3::default(), -2.0);
        assert_eq!((e.first_seen, e.last_known), (-2.0, 0.0));
    }
}
