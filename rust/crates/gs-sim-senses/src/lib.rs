//! gs-npc's perception pass, native: `GSNpcSenses.rank_many` (who is within reach, nearest first) and
//! `GSNpcSenses.decide` (sight, hearing, cone, smoke, line of sight, then the commitment with its
//! stickiness and grace), over the packed arrays the script's `GSNpcPerception` already builds.
//!
//! The script's arithmetic in the script's order and precision (engine vectors `f32`, script floats
//! `f64`), so an NPC deciding natively commits to the same target at the same moment as the GDScript it
//! replaces. The line-of-sight ray is the caller's: [`decide`] asks it in the order the script casts.

use gs_sim_math::{Vec3, clampf, maxf, minf};

#[cfg(test)]
mod tests;

/// The settings of a `GSNpcSenses` that a pass reads.
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    /// `switch_ratio`: how much nearer a rival must be before an NPC switches to it.
    pub switch_ratio: f64,
    /// `commitment_grace`: seconds a target is kept after it was last perceived.
    pub grace: f64,
    /// `line_of_sight_enabled`.
    pub los_enabled: bool,
    /// `prune_by_held`.
    pub prune_by_held: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            switch_ratio: 0.6,
            grace: 3.0,
            los_enabled: true,
            prune_by_held: true,
        }
    }
}

/// The candidates of a pass, as `GSNpcSenses.Packed` holds them.
#[derive(Clone, Copy, Debug)]
pub struct Cands<'a> {
    pub pos: &'a [Vec3],
    pub loud: &'a [f64],
    /// Faction numbers; -2 is a missing candidate.
    pub faction: &'a [i32],
    /// Each candidate's id as the index of the first candidate with that id: equal ids, equal numbers.
    pub first: &'a [i32],
}

impl Cands<'_> {
    fn len(&self) -> usize {
        self.pos
            .len()
            .min(self.loud.len())
            .min(self.faction.len())
            .min(self.first.len())
    }
}

/// A sight-blocking sphere (smoke): `GSNpcSenses.add_sight_blocker`.
#[derive(Clone, Copy, Debug)]
pub struct Blocker {
    pub center: Vec3,
    pub radius: f64,
    /// Simulated second it stops blocking; infinity for never.
    pub until: f64,
}

/// What a pass reads of one NPC (`GSNpcInstance` and its `GSNpcDef`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Npc {
    /// `position()`.
    pub here: Vec3,
    /// `facing()`.
    pub facing: Vec3,
    /// `def.eye_height`.
    pub eye: f64,
    /// `GSNpcSenses.reach_of`: `def.sight_range * maxf(sight_scale, 0.0)`.
    pub reach: f64,
    /// `def.hearing_range`.
    pub hearing: f64,
    /// `def.sight_half_angle_deg`.
    pub half_deg: f64,
    /// `def.sight_half_angle()`.
    pub half_rad: f64,
    pub blind_until: f64,
    pub target_seen_at: f64,
    /// Its faction's number among the candidates, -1 when none has it.
    pub own: i32,
    /// `Packed.index_of(target_id)`: the first candidate with its target's id, or -1.
    pub held: i32,
    /// `has_target()`.
    pub has_target: bool,
    /// `def.require_line_of_sight`.
    pub require_los: bool,
}

/// Where a pass leaves an NPC's target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// `target_id` unchanged.
    Keep,
    /// `target_id` emptied.
    Clear,
    /// `target_id` set to this candidate's id.
    Set(i32),
}

/// What a pass does to one NPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub target: Target,
    /// `target_seen_at` and `engaged_at` set to now.
    pub seen: bool,
    /// `target_since` set to now.
    pub since: bool,
}

impl Outcome {
    const KEEP: Self = Self {
        target: Target::Keep,
        seen: false,
        since: false,
    };
}

#[inline]
fn d2(a: Vec3, b: Vec3) -> f32 {
    (b - a).length_squared()
}

/// `GSNpcSenses.rank_many` for one NPC: the candidates it could perceive by range alone, nearest first,
/// at most `cap` (0 or less is all of them), into `out`. `keys` is scratch.
#[allow(clippy::too_many_arguments)]
pub fn rank(
    here: Vec3,
    sight: f64,
    hearing: f64,
    own: i32,
    c: &Cands,
    cap: i64,
    keys: &mut Vec<i64>,
    out: &mut Vec<i32>,
) {
    keys.clear();
    out.clear();
    for i in 0..c.len() {
        let f = c.faction[i];
        if f == own || f == -2 {
            continue;
        }
        let d = d2(here, c.pos[i]) as f64;
        let reach = maxf(sight, minf(hearing, c.loud[i]));
        if d > reach * reach && d > 0.00000001 {
            continue;
        }
        keys.push((((d * 1024.0) as i64) << 20) | i as i64);
    }
    keys.sort_unstable();
    let n = if cap <= 0 {
        keys.len()
    } else {
        keys.len().min(cap as usize)
    };
    out.extend(keys[..n].iter().map(|k| (k & 0xFFFFF) as i32));
}

/// One NPC's decide, with the line of sight asked of `los(from, to)` (true: clear).
struct Pass<'a, L: FnMut(Vec3, Vec3) -> bool> {
    npc: &'a Npc,
    c: &'a Cands<'a>,
    blockers: &'a [Blocker],
    t: &'a Tuning,
    now: f64,
    checks: &'a mut i64,
    los: &'a mut L,
}

impl<L: FnMut(Vec3, Vec3) -> bool> Pass<'_, L> {
    #[inline]
    fn eye(&self) -> Vec3 {
        self.npc.here + Vec3::new(0.0, 1.0, 0.0).scale(self.npc.eye)
    }

    /// `GSNpcSenses.sight_blocked`; expired blockers never block.
    fn sight_blocked(&self, from: Vec3, to: Vec3) -> bool {
        if self.blockers.is_empty() {
            return false;
        }
        let seg = to - from;
        let len2 = seg.length_squared() as f64;
        for b in self.blockers.iter().rev() {
            if self.now >= b.until {
                continue;
            }
            let c = b.center;
            let t = if len2 < 0.000001 {
                0.0
            } else {
                clampf((c - from).dot(seg) as f64 / len2, 0.0, 1.0)
            };
            if d2(from + seg.scale(t), c) as f64 <= b.radius * b.radius {
                return true;
            }
        }
        false
    }

    /// `GSNpcSenses._perceives_at` for candidate `i`.
    fn perceives(&mut self, i: usize) -> bool {
        *self.checks += 1;
        let npc = self.npc;
        let to = self.c.pos[i];
        let to_target = to - npc.here;
        let distance = to_target.length() as f64;
        if distance <= 0.0001 {
            return true;
        }
        if distance <= minf(npc.hearing, self.c.loud[i]) {
            return true;
        }
        if self.now < npc.blind_until {
            return false;
        }
        if distance > npc.reach {
            return false;
        }
        let from = self.eye();
        if self.sight_blocked(from, to) {
            return false;
        }
        if npc.half_deg < 180.0 {
            let angle = npc.facing.angle_to(to_target.div_scalar(distance)) as f64;
            if angle > npc.half_rad {
                return false;
            }
        }
        if npc.require_los && self.t.los_enabled {
            return (self.los)(from, to);
        }
        true
    }

    #[inline]
    fn dist(&self, i: usize) -> f64 {
        self.npc.here.distance_to(self.c.pos[i]) as f64
    }

    /// `GSNpcSenses.decide`.
    fn decide(&mut self, order: &[i32]) -> Outcome {
        let npc = self.npc;
        let held = npc.held;
        let n = self.c.len();
        let held_ok = held >= 0
            && (held as usize) < n
            && self.c.faction[held as usize] != -2
            && self.c.faction[held as usize] != npc.own;
        let mut held_tested = false;
        if self.t.prune_by_held && held_ok {
            if self.perceives(held as usize) {
                return self.decide_held(order, held as usize);
            }
            held_tested = true;
        }

        let mut best: Option<(usize, f64)> = None;
        let mut committed: Option<(usize, f64)> = None;
        for &index in order {
            if index == held {
                if held_tested && self.t.prune_by_held {
                    continue;
                }
                held_tested = true;
            }
            let i = index as usize;
            if self.perceives(i) {
                let d = self.dist(i);
                best = Some((i, d));
                if index == held {
                    committed = Some((i, d));
                }
                break;
            }
        }
        if !held_tested && held_ok && self.perceives(held as usize) {
            committed = Some((held as usize, self.dist(held as usize)));
        }
        self.commit(best, committed)
    }

    /// `GSNpcSenses._commit`.
    fn commit(&self, best: Option<(usize, f64)>, committed: Option<(usize, f64)>) -> Outcome {
        let npc = self.npc;
        let first = self.c.first;
        if let Some((ci, cd)) = committed {
            if let Some((bi, bd)) = best
                && first[bi] != first[ci]
                && bd < cd * self.t.switch_ratio
            {
                return Outcome {
                    target: Target::Set(bi as i32),
                    seen: true,
                    since: true,
                };
            }
            return Outcome {
                target: Target::Keep,
                seen: true,
                since: false,
            };
        }
        if npc.has_target {
            if let Some((bi, _)) = best {
                let same = npc.held >= 0 && first[bi] == npc.held;
                return Outcome {
                    target: Target::Set(bi as i32),
                    seen: true,
                    since: !same,
                };
            }
            if self.now - npc.target_seen_at < self.t.grace {
                return Outcome::KEEP;
            }
            return Outcome {
                target: Target::Clear,
                seen: false,
                since: false,
            };
        }
        if let Some((bi, _)) = best {
            return Outcome {
                target: Target::Set(bi as i32),
                seen: true,
                since: true,
            };
        }
        Outcome::KEEP
    }

    /// `GSNpcSenses._decide_held`.
    fn decide_held(&mut self, order: &[i32], held: usize) -> Outcome {
        let limit = self.dist(held) * self.t.switch_ratio;
        let mut last: isize = -1;
        for (r, &index) in order.iter().enumerate() {
            if self.dist(index as usize) < limit {
                last = r as isize;
            }
        }
        let mut out = Outcome {
            target: Target::Keep,
            seen: true,
            since: false,
        };
        for &index in order.iter().take((last + 1) as usize) {
            if index == held as i32 {
                break;
            }
            let i = index as usize;
            if self.perceives(i) {
                if self.c.first[i] != self.c.first[held] && self.dist(i) < limit {
                    out.target = Target::Set(index);
                    out.since = true;
                }
                break;
            }
        }
        out
    }
}

/// `GSNpcSenses.decide` for `npc` over `order` (from [`rank`]) at simulated second `now`. Every
/// perception test adds one to `checks` (`sight_checks`); `los(from, to)` is asked exactly where the script
/// casts its occlusion ray, in the same order, and answers whether the line is clear.
#[allow(clippy::too_many_arguments)]
pub fn decide<L: FnMut(Vec3, Vec3) -> bool>(
    npc: &Npc,
    c: &Cands,
    blockers: &[Blocker],
    order: &[i32],
    t: &Tuning,
    now: f64,
    checks: &mut i64,
    los: &mut L,
) -> Outcome {
    Pass {
        npc,
        c,
        blockers,
        t,
        now,
        checks,
        los,
    }
    .decide(order)
}
