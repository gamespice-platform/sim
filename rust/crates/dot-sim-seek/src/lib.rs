//! Prop Hunt's hunter sensing for one tick, every hunter at once: game-prop-hunt's `PhBots._prey`,
//! `DotBotSearch.observe` and the rows of `PhBots._doubt`, to the bit and in the script's order.
//!
//! The script asks each hunter's senses on its own (a players list, a dictionary per place, a
//! dictionary per doubt row and a ray built in script for every line of sight); this answers
//! them for all hunters in one pass over flat tables: the props as the tick has them, the map's
//! pieces (written once a map), and per hunter a mirror of its search's places and cleared list.
//! What a hunter decides with the answer (chase, test, walk, look) stays the script's.
//!
//! Precision follows the script: engine vectors are `f32` ([`Vec3`]), script floats `f64`; a
//! distance or angle is taken in `f32` and compared in `f64`; `Vector3(0, h, 0)` rounds `h` to
//! `f32` first. The line of sight is the caller's (`los(from, to)`), asked in the script's
//! order for the same pairs, so a ray is cast exactly when the script would cast it.

use dot_sim_doubt::{Piece as DoubtPiece, in_view};
use dot_sim_math::{Vec3, clampf, minf, rad_to_deg};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

pub mod body;

/// The doubt reach (metres): the script's `DOUBT_REACH`.
pub const DOUBT_REACH: f64 = 12.0;
/// A taunt this near a doubted thing (metres) makes it odder: `TAUNT_NEAR`.
pub const TAUNT_NEAR: f64 = 5.0;
/// Doubt that makes a hunter test: `SUSPECT_AT`.
pub const SUSPECT_AT: f64 = 1.0;
/// Kin within this (metres) of a prop row count: `_kin_count`'s default `within`.
pub const KIN_WITHIN: f64 = 3.5;
/// The search's head height over a place: `_seek`'s `observe(..., 0.5)`.
pub const HEAD: f64 = 0.5;

/// The rules every hunter shares this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rules {
    /// The game clock (`now()`).
    pub now: f64,
    /// `4.0 * dt()`.
    pub step: f64,
    /// `bot_suspicion / 100.0`.
    pub base: f64,
    pub fov: f64,
    pub sight: f64,
    pub memory: f64,
    pub map_knowledge: f64,
    pub aggression: f64,
    pub round: i64,
}

/// One of the map's pieces.
#[derive(Clone, Copy, Debug, Default)]
pub struct Piece {
    /// `origin + Vector3(0, size.y * 0.5, 0)`.
    pub middle: Vec3,
    /// `transform.origin`.
    pub feet: Vec3,
    /// The piece's kind (its `id`), as a code the caller gives each kind.
    pub kind: i32,
    pub broken: bool,
}

/// A player on the props' side, as `players_on(PROPS)` lists them this tick.
#[derive(Clone, Debug, Default)]
pub struct Prop {
    pub id: String,
    /// `controller.state.position`.
    pub at: Vec3,
    pub velocity: Vec3,
    /// `hull_height()` (`f64`, a script float).
    pub hull: f64,
    pub alive: bool,
    pub watching: bool,
    pub marked: bool,
    pub disguised: bool,
    /// `abs(disguise.pitch) > 1 or abs(disguise.roll) > 1`.
    pub tilted: bool,
    /// The disguise's kind code; a kind no piece has gets a code no piece has.
    pub kind: i32,
}

/// A taunt heard lately: where and when.
#[derive(Clone, Copy, Debug, Default)]
pub struct Taunt {
    pub at: Vec3,
    pub t: f64,
}

/// A hunter's search places as the script holds them (`DotBotSearch`): what observe reads.
#[derive(Clone, Debug, Default)]
pub struct Search {
    pub at: Vec<Vec3>,
    pub seen_at: Vec<f64>,
    pub recheck: f64,
    pub look_reach: f64,
}

/// What persists for a hunter between ticks: its search mirror and its cleared list.
#[derive(Clone, Debug, Default)]
pub struct Mind {
    pub search: Search,
    /// `state["cleared"]`: key (`p<index>` or `u<id>`) to when.
    pub cleared: HashMap<String, f64>,
}

/// One hunter's ask this tick.
#[derive(Clone, Debug, Default)]
pub struct Hunter {
    pub id: String,
    pub eye: Vec3,
    pub forward: Vec3,
    pub careful: bool,
    /// The props-list index of `state["found"]`, or -1.
    pub found: i32,
    pub found_at: f64,
    /// Whether, with no prey in sight, this hunter reaches its observe and doubt this tick (a
    /// doubt tick, no prey remembered, no test holding the turn).
    pub sense: bool,
    /// Whether the search is observed here (false: the script observes, a taunt may move it first).
    pub observe: bool,
    /// The test that ends this tick when the hunter gets that far: its key is cleared first.
    pub clearing: Option<String>,
}

/// A doubt row's addition: `doubt[key] += delta`, then capped under [`SUSPECT_AT`] when `capped`.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubtAdd {
    /// The piece's index, or `-1 - prop index` for a prop.
    pub key: i32,
    pub delta: f64,
    pub capped: bool,
}

/// One hunter's answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sensed {
    /// The props-list index of the prey in sight, or -1.
    pub prey: i32,
    /// Whether observe and doubt ran.
    pub sensed: bool,
    /// The places crossed off, in the search's order.
    pub crossed: Vec<i32>,
    pub doubt: Vec<DoubtAdd>,
}

/// Godot's `String.hash()` (djb2 over the code points, `u32` wrapping), which the script's
/// `hash(string)` returns.
pub fn godot_string_hash(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for c in s.chars() {
        h = (h << 5).wrapping_add(h).wrapping_add(c as u32);
    }
    h
}

/// `_memory_draw`: `hash("%s|%d|%s|%d" % [hunter, index, prop id, round]) & 0xFFFF` over 65535.
pub fn memory_draw(hunter: &str, index: i32, prop: &str, round: i64) -> f64 {
    let key = format!("{hunter}|{index}|{prop}|{round}");
    f64::from(godot_string_hash(&key) & 0xFFFF) / 65535.0
}

/// `_kin_count`: the pieces of `kind` whose feet are within [`KIN_WITHIN`] of `at`.
pub fn kin_count(pieces: &[Piece], kind: i32, at: Vec3) -> i32 {
    let mut count = 0;
    for p in pieces {
        if p.kind == kind && f64::from(p.feet.distance_to(at)) < KIN_WITHIN {
            count += 1;
        }
    }
    count
}

fn up(h: f64) -> Vec3 {
    Vec3::new(0.0, h as f32, 0.0)
}

/// The sensing tables: the map's pieces and each hunter's mind.
#[derive(Default)]
pub struct Seek {
    pub pieces: Vec<Piece>,
    /// `_kin_near`: kin within reach of each piece, minus itself (may be shorter than the pieces).
    pub kin_near: Vec<i32>,
    pub minds: HashMap<String, Mind>,
    doubt_pieces: Vec<DoubtPiece>,
    kept: Vec<i32>,
}

impl Seek {
    /// Holds the map's pieces (all whole) and the kin table.
    pub fn set_pieces(&mut self, pieces: Vec<Piece>, kin_near: Vec<i32>) {
        self.doubt_pieces = pieces
            .iter()
            .map(|p| DoubtPiece {
                middle: p.middle,
                broken: p.broken,
            })
            .collect();
        self.pieces = pieces;
        self.kin_near = kin_near;
    }

    /// Piece `index` broken or whole; false for an index outside the table.
    pub fn set_broken(&mut self, index: usize, broken: bool) -> bool {
        match (self.pieces.get_mut(index), self.doubt_pieces.get_mut(index)) {
            (Some(p), Some(d)) => {
                p.broken = broken;
                d.broken = broken;
                true
            }
            _ => false,
        }
    }

    /// The mind of hunter `id`, made empty on first ask.
    pub fn mind(&mut self, id: &str) -> &mut Mind {
        self.minds.entry(id.to_string()).or_default()
    }

    /// Every hunter's senses this tick, in order, into `out` (one per hunter).
    pub fn sense(
        &mut self,
        rules: &Rules,
        props: &[Prop],
        taunts: &[Taunt],
        hunters: &[Hunter],
        los: &mut dyn FnMut(Vec3, Vec3) -> bool,
        out: &mut Vec<Sensed>,
    ) {
        out.clear();
        let mut minds = std::mem::take(&mut self.minds);
        for h in hunters {
            let mind = minds.entry(h.id.clone()).or_default();
            let mut s = Sensed {
                prey: self.prey(rules, props, h, los),
                ..Sensed::default()
            };
            if s.prey < 0 {
                if let Some(key) = &h.clearing {
                    mind.cleared.insert(key.clone(), rules.now);
                }
                if h.sense {
                    s.sensed = true;
                    if h.observe {
                        observe(&mut mind.search, rules, h, los, &mut s.crossed);
                    }
                    self.doubt(rules, props, taunts, h, mind, los, &mut s.doubt);
                }
            }
            out.push(s);
        }
        self.minds = minds;
    }

    /// `_prey`: the nearest prop known for one and in sight, or -1.
    fn prey(
        &self,
        rules: &Rules,
        props: &[Prop],
        h: &Hunter,
        los: &mut dyn FnMut(Vec3, Vec3) -> bool,
    ) -> i32 {
        let mut best = -1;
        let mut best_d = f64::INFINITY;
        for (i, p) in props.iter().enumerate() {
            if !p.alive || p.watching {
                continue;
            }
            let middle = p.at + up(minf(p.hull, 1.4) * 0.5);
            let d = f64::from(h.eye.distance_to(middle));
            let flat = (p.velocity.x * p.velocity.x + p.velocity.z * p.velocity.z).sqrt();
            let moving = flat > 1.0;
            let known = p.marked
                || !p.disguised
                || moving
                || (i as i32 == h.found && rules.now - h.found_at < rules.memory + 4.0);
            if !known {
                continue;
            }
            let sight = if !p.marked {
                minf(rules.sight, 25.0)
            } else {
                rules.sight
            };
            if d > sight || d >= best_d {
                continue;
            }
            if !p.marked
                && d > 2.0
                && rad_to_deg(f64::from(h.forward.angle_to(middle - h.eye))) > rules.fov * 0.5
            {
                continue;
            }
            if !los(h.eye, middle) {
                continue;
            }
            best = i as i32;
            best_d = d;
        }
        best
    }

    /// `_doubt`'s rows: what is near and in view, not cleared, in sight, and how much doubt each adds.
    #[allow(clippy::too_many_arguments)]
    fn doubt(
        &mut self,
        rules: &Rules,
        props: &[Prop],
        taunts: &[Taunt],
        h: &Hunter,
        mind: &Mind,
        los: &mut dyn FnMut(Vec3, Vec3) -> bool,
        out: &mut Vec<DoubtAdd>,
    ) {
        let eye = h.eye;
        let mut kept = std::mem::take(&mut self.kept);
        kept.clear();
        dot_sim_doubt::scan(
            &self.doubt_pieces,
            eye,
            h.forward,
            rules.fov,
            DOUBT_REACH,
            &mut kept,
        );
        for &i in &kept {
            let p = &self.pieces[i as usize];
            let key = format!("p{i}");
            if mind.cleared.contains_key(&key) {
                continue;
            }
            // The scan already held the row loop's reach and field of view tests.
            debug_assert!(in_view(
                &self.doubt_pieces[i as usize],
                eye,
                h.forward,
                rules.fov,
                DOUBT_REACH
            ));
            let d = f64::from(eye.distance_to(p.middle));
            if !los(eye, p.middle) {
                continue;
            }
            let kin = match self.kin_near.get(i as usize) {
                Some(&k) => k,
                None => kin_count(&self.pieces, p.kind, p.feet),
            };
            let odd = self.oddness(rules, taunts, h, i, "", kin, p.feet, false);
            out.push(add(rules, i, d, odd, h.careful));
        }
        self.kept = kept;

        for (n, p) in props.iter().enumerate() {
            if !p.alive || p.watching || !p.disguised {
                continue;
            }
            let at = p.at;
            if (f64::from(at.x) - f64::from(eye.x)).abs() > DOUBT_REACH
                || (f64::from(at.z) - f64::from(eye.z)).abs() > DOUBT_REACH
            {
                continue;
            }
            let middle = at + up(p.hull * 0.5);
            let d = f64::from(eye.distance_to(middle));
            if d > DOUBT_REACH || d < 0.5 {
                continue;
            }
            if rad_to_deg(f64::from(h.forward.angle_to(middle - eye))) > rules.fov * 0.5 {
                continue;
            }
            let key = format!("u{}", p.id);
            if let Some(&t) = mind.cleared.get(&key)
                && rules.now - t < 20.0
            {
                continue;
            }
            if !los(eye, middle) {
                continue;
            }
            let kin = kin_count(&self.pieces, p.kind, at);
            let odd = self.oddness(rules, taunts, h, -1, &p.id, kin, at, p.tilted);
            out.push(add(rules, -1 - n as i32, d, odd, h.careful));
        }
    }

    /// `_oddness` of a row with `kin` of its kind round it.
    #[allow(clippy::too_many_arguments)]
    fn oddness(
        &self,
        rules: &Rules,
        taunts: &[Taunt],
        h: &Hunter,
        index: i32,
        prop: &str,
        kin: i32,
        feet: Vec3,
        tilted: bool,
    ) -> f64 {
        let mut odd = rules.base;
        if kin == 0 {
            odd += 0.35;
        }
        let memory = memory_draw(&h.id, index, prop, rules.round);
        if memory < rules.map_knowledge / (1.0 + f64::from(kin) * 0.6) {
            if index >= 0 {
                odd *= 0.15;
            } else {
                odd += 1.2;
            }
        }
        if tilted {
            odd += 0.5;
        }
        for heard in taunts {
            if rules.now - heard.t < 10.0 && f64::from(heard.at.distance_to(feet)) < TAUNT_NEAR {
                odd += 1.4;
                break;
            }
        }
        odd
    }
}

fn add(rules: &Rules, key: i32, d: f64, odd: f64, careful: bool) -> DoubtAdd {
    let closeness = clampf(1.0 - d / DOUBT_REACH, 0.0, 1.0);
    DoubtAdd {
        key,
        delta: rules.step * odd * (0.5 + closeness) * (0.6 + rules.aggression * 0.8),
        capped: careful && odd < 1.0,
    }
}

/// `DotBotSearch.observe(eye, forward, fov, sight, los, now, HEAD)`: the places crossed off, each
/// marked seen now in the mirror as the script marks it.
fn observe(
    search: &mut Search,
    rules: &Rules,
    h: &Hunter,
    los: &mut dyn FnMut(Vec3, Vec3) -> bool,
    out: &mut Vec<i32>,
) {
    let reach = minf(search.look_reach, rules.sight);
    let eye = h.eye;
    for (i, at) in search.at.iter().enumerate() {
        if rules.now - search.seen_at[i] < search.recheck * 0.25 {
            continue;
        }
        if (f64::from(at.x) - f64::from(eye.x)).abs() > reach
            || (f64::from(at.z) - f64::from(eye.z)).abs() > reach
        {
            continue;
        }
        // DotBotHiding.sees(eye, forward, fov, reach, at + (0, head, 0), los).
        let target = *at + up(HEAD);
        let to = target - eye;
        let d = to.length();
        if f64::from(d) > reach {
            continue;
        }
        if d > 0.01
            && rad_to_deg(f64::from(h.forward.angle_to(to.div_scalar(f64::from(d)))))
                > rules.fov * 0.5
        {
            continue;
        }
        if !los(eye, target) {
            continue;
        }
        search.seen_at[i] = rules.now;
        out.push(i as i32);
    }
}
