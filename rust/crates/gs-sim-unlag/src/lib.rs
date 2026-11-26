//! Lag compensation's position history: per player, a ring of the feet and crouch fraction it
//! recorded each tick, read back between two ticks. FC's `fc_game_unlag.gd` ring in packed
//! columns, pure Rust in the script's order and precision.
//!
//! A ring's slot for tick `t` is `t mod len`, tagged with `t`; an untagged slot holds -1. A
//! record whose previous tick is tagged and lies more than the teleport cut away clears the
//! ring first, so a rewind never blends across a respawn. Positions are the engine's f32
//! vectors, lerped in f32 as `Vector3.lerp` does; crouch fractions are f32, lerped in f64 as
//! the script's `lerpf` does.

use std::collections::HashMap;

/// One player's history.
#[derive(Clone, Debug)]
struct Ring {
    pos: Vec<[f32; 3]>,
    ticks: Vec<i32>,
    crouch: Vec<f32>,
}

impl Ring {
    fn new(len: usize) -> Self {
        Ring {
            pos: vec![[0.0; 3]; len],
            ticks: vec![-1; len],
            crouch: vec![0.0; len],
        }
    }

    /// GDScript's `i % len` for an index into a packed array: a negative one counts from the end.
    fn slot(&self, i: i64) -> usize {
        i.rem_euclid(self.ticks.len() as i64) as usize
    }

    /// The two slots either side of fractional tick `at` and the weight between them; None when
    /// `at` is below 0 or its floor is not recorded. `b` is None when the tick after is not.
    fn bracket(&self, at: f64) -> Option<(usize, Option<usize>, f64)> {
        if at < 0.0 {
            return None;
        }
        let lo = at.floor() as i64;
        let a = self.slot(lo);
        if i64::from(self.ticks[a]) != lo {
            return None;
        }
        let b = self.slot(lo + 1);
        let b = (i64::from(self.ticks[b]) == lo + 1).then_some(b);
        Some((a, b, at - lo as f64))
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (x, y, z) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    (x * x + y * y + z * z).sqrt()
}

/// Every player's ring, keyed by player id.
#[derive(Clone, Debug, Default)]
pub struct Rings {
    len: usize,
    rows: HashMap<i64, Ring>,
}

impl Rings {
    /// Rings of `len` slots (at least 1). A ring keeps the length it was made with.
    pub fn new(len: usize) -> Self {
        Rings {
            len: len.max(1),
            rows: HashMap::new(),
        }
    }

    /// The length a ring made from now on gets; existing rings keep theirs.
    pub fn set_len(&mut self, len: usize) {
        self.len = len.max(1);
    }

    /// Records player `id` at `tick` (not negative): its feet and crouch fraction. Clears its
    /// ring first when the previous tick is recorded more than `cut` away (a respawn or teleport).
    pub fn record(&mut self, tick: i64, id: i64, pos: [f32; 3], crouch: f32, cut: f64) {
        let len = self.len;
        let ring = self.rows.entry(id).or_insert_with(|| Ring::new(len));
        let slot = ring.slot(tick);
        let before = ring.slot(tick - 1);
        if i64::from(ring.ticks[before]) == tick - 1 && f64::from(dist(ring.pos[before], pos)) > cut
        {
            ring.ticks.fill(-1);
        }
        ring.pos[slot] = pos;
        ring.ticks[slot] = tick as i32;
        ring.crouch[slot] = crouch;
    }

    /// Where player `id` was at fractional tick `at`: lerped between the recorded ticks either
    /// side, the floor's alone when the next is not recorded, None when the floor is not.
    pub fn position_at(&self, id: i64, at: f64) -> Option<[f32; 3]> {
        let ring = self.rows.get(&id)?;
        let (a, b, w) = ring.bracket(at)?;
        let pa = ring.pos[a];
        let Some(b) = b else { return Some(pa) };
        let pb = ring.pos[b];
        let w = w as f32;
        Some([
            pa[0] + (pb[0] - pa[0]) * w,
            pa[1] + (pb[1] - pa[1]) * w,
            pa[2] + (pb[2] - pa[2]) * w,
        ])
    }

    /// Player `id`'s crouch fraction at `at`, bracketed as [`Rings::position_at`]; -1 when not recorded.
    pub fn crouch_at(&self, id: i64, at: f64) -> f64 {
        let Some(ring) = self.rows.get(&id) else {
            return -1.0;
        };
        let Some((a, b, w)) = ring.bracket(at) else {
            return -1.0;
        };
        let ca = f64::from(ring.crouch[a]);
        match b {
            Some(b) => ca + (f64::from(ring.crouch[b]) - ca) * w,
            None => ca,
        }
    }

    /// Whether player `id` has a ring.
    pub fn has(&self, id: i64) -> bool {
        self.rows.contains_key(&id)
    }

    /// Drops player `id`'s ring.
    pub fn forget(&mut self, id: i64) {
        self.rows.remove(&id);
    }

    /// Drops every ring.
    pub fn clear(&mut self) {
        self.rows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerps_between_recorded_ticks() {
        let mut r = Rings::new(8);
        r.record(3, 1, [0.0, 0.0, 0.0], 0.0, 10.0);
        r.record(4, 1, [2.0, 0.0, -2.0], 1.0, 10.0);
        assert_eq!(r.position_at(1, 3.5), Some([1.0, 0.0, -1.0]));
        assert_eq!(r.crouch_at(1, 3.25), 0.25);
        assert_eq!(r.position_at(1, 4.5), Some([2.0, 0.0, -2.0]));
        assert_eq!(r.position_at(1, 2.5), None);
        assert_eq!(r.crouch_at(1, 5.0), -1.0);
        assert_eq!(r.position_at(2, 3.0), None);
        assert_eq!(r.position_at(1, -0.5), None);
    }

    #[test]
    fn a_teleport_clears_the_ring() {
        let mut r = Rings::new(8);
        r.record(1, 1, [0.0; 3], 0.0, 10.0);
        r.record(2, 1, [1.0, 0.0, 0.0], 0.0, 10.0);
        r.record(3, 1, [50.0, 0.0, 0.0], 0.0, 10.0);
        assert_eq!(r.position_at(1, 2.0), None);
        assert_eq!(r.position_at(1, 3.0), Some([50.0, 0.0, 0.0]));
    }

    #[test]
    fn wraps_and_overwrites() {
        let mut r = Rings::new(4);
        for t in 0..10 {
            r.record(t, 7, [t as f32, 0.0, 0.0], 0.0, 10.0);
        }
        assert_eq!(r.position_at(7, 5.0), None);
        assert_eq!(r.position_at(7, 8.5), Some([8.5, 0.0, 0.0]));
        r.forget(7);
        assert!(!r.has(7));
    }

    #[test]
    fn tick_zero_reads_the_last_slot_as_its_previous() {
        // GDScript's ticks[-1] is the last slot: untagged (-1), so a far first record clears an empty ring.
        let mut r = Rings::new(4);
        r.record(0, 1, [100.0, 0.0, 0.0], 0.5, 10.0);
        assert_eq!(r.position_at(1, 0.0), Some([100.0, 0.0, 0.0]));
        assert_eq!(r.crouch_at(1, 0.0), 0.5);
    }
}
