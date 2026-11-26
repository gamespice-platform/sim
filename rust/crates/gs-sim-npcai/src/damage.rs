//! `GSNpcAiBrain._sense_damage`: light damage for any hurt since the last think, heavy when the hurt
//! inside a window adds up to a fraction of the NPC's health.

use crate::{HEAVY_DAMAGE, LIGHT_DAMAGE};

/// The hurt an NPC took inside the heavy-damage window, oldest first, as `(when, amount)`.
#[derive(Clone, Debug, Default)]
pub struct DamageLog {
    hits: Vec<(f64, f64)>,
}

/// What one think's damage sense is told.
#[derive(Clone, Copy, Debug)]
pub struct DamageSense {
    /// The context's clock.
    pub now: f64,
    /// Hurt taken since the last think (`_pending_damage`).
    pub pending: f64,
    /// `heavy_damage_window`, seconds.
    pub window: f64,
    /// The definition's `max_health`, 0 with no definition.
    pub max_health: f64,
    /// `heavy_damage_fraction`.
    pub fraction: f64,
}

impl DamageLog {
    /// One think's damage sense: logs [`DamageSense::pending`], drops hits older than the window and
    /// answers the condition bits, [`LIGHT_DAMAGE`] and [`HEAVY_DAMAGE`].
    pub fn sense(&mut self, s: DamageSense) -> i64 {
        let mut bits = 0;
        if s.pending > 0.0 {
            bits |= LIGHT_DAMAGE;
            self.hits.push((s.now, s.pending));
        }
        if self.hits.is_empty() {
            return bits;
        }
        let mut total = 0.0;
        self.hits.retain(|&(at, amount)| {
            let keep = s.now - at <= s.window;
            if keep {
                total += amount;
            }
            keep
        });
        if s.max_health > 0.0 && total >= s.max_health * s.fraction {
            bits |= HEAVY_DAMAGE;
        }
        bits
    }

    /// Hits still in the log.
    pub fn len(&self) -> usize {
        self.hits.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// Forgets every hit.
    pub fn clear(&mut self) {
        self.hits.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sense(log: &mut DamageLog, now: f64, pending: f64) -> i64 {
        log.sense(DamageSense {
            now,
            pending,
            window: 1.5,
            max_health: 100.0,
            fraction: 0.25,
        })
    }

    #[test]
    fn light_for_any_hurt_heavy_for_the_window() {
        let mut log = DamageLog::default();
        assert_eq!(sense(&mut log, 0.0, 10.0), LIGHT_DAMAGE);
        assert_eq!(sense(&mut log, 0.5, 10.0), LIGHT_DAMAGE);
        assert_eq!(sense(&mut log, 1.0, 5.0), LIGHT_DAMAGE | HEAVY_DAMAGE);
        // Still heavy with nothing new while the hits are inside the window.
        assert_eq!(sense(&mut log, 1.5, 0.0), HEAVY_DAMAGE);
        // The first hit leaves the window.
        assert_eq!(sense(&mut log, 1.6, 0.0), 0);
        assert_eq!(log.len(), 2);
        assert_eq!(sense(&mut log, 9.0, 0.0), 0);
        assert!(log.is_empty());
    }

    #[test]
    fn no_health_is_never_heavy() {
        let mut log = DamageLog::default();
        let bits = log.sense(DamageSense {
            now: 0.0,
            pending: 1000.0,
            window: 1.5,
            max_health: 0.0,
            fraction: 0.25,
        });
        assert_eq!(bits, LIGHT_DAMAGE);
        // Negative hurt is not logged; the earlier hit still counts against a real health.
        assert_eq!(sense(&mut log, 0.1, -3.0), HEAVY_DAMAGE);
        assert_eq!(log.len(), 1);
    }
}
