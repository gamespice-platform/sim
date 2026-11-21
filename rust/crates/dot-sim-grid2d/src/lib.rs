//! dot-2d's `GS2DGrid` to the bit, and game-hungario's bot forage scan over it.
//!
//! The grid is the script's: a cell key packed from two 32-bit halves, an id list per cell in
//! insertion order with swap-and-pop removal, empty cells dropped, and the position each id was
//! last placed at (an engine `Vector2`, so `f32`). A query walks the cells `x` then `y` and each
//! cell's list in order, so ids come out in the script's order. Each entry also carries a
//! `worth` the game writes when it places it (Hungario: what eating it is worth, in mass).
//!
//! [`Grid::forage`] is `HungryBot.command_for`'s neighbour loop: the rect query over the bot's
//! sight, the `f32` distance to each id, and the snack pick (worth over distance plus a bias, the
//! first of equals) in the script's `f64` order; pieces are handed back in query order, because
//! their mass and owner live on the piece, not in the grid. [`judge`] is the bot's verdict on
//! them (`HungryBot._judge`), given a way to read each piece's owner and mass.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A multiply hasher for the grid's `i64` keys; the default SipHash costs more than the lookup.
#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write_i64(&mut self, v: i64) {
        self.0 = (v as u64 ^ (v as u64 >> 29)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

type Map<V> = HashMap<i64, V, BuildHasherDefault<KeyHasher>>;

/// The script's `int(floor(v / cell_size))`: the `f32` coordinate widened, divided in `f64`.
#[inline]
pub fn cell_of(v: f32, cell: f64) -> i64 {
    (f64::from(v) / cell).floor() as i64
}

/// The script's `((cx & 0xFFFFFFFF) << 32) | (cy & 0xFFFFFFFF)`.
#[inline]
pub fn key(cx: i64, cy: i64) -> i64 {
    (cx & 0xFFFF_FFFF).wrapping_shl(32) | (cy & 0xFFFF_FFFF)
}

/// An engine `Rect2` (`f32`): its position and size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// `Rect2.has_point`: inside the start edges, outside the end edges (`position + size`).
    #[inline]
    pub fn has_point(&self, x: f32, y: f32) -> bool {
        !(x < self.x || y < self.y || x >= self.x + self.w || y >= self.y + self.h)
    }
}

/// One placed id.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Entry {
    home: i64,
    x: f32,
    y: f32,
    worth: f64,
}

/// What `HungryBot.command_for` needs to tell its id ranges apart and score a snack. Hungario's
/// values are the default: pieces below `1_000_000`, item drops in `[3_000_000, 4_000_000)`, an
/// item worth `220`, a distance bias of `40`, and anything within `0.001` skipped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForageRules {
    /// Ids below this are pieces: handed back, not scored.
    pub piece_limit: i64,
    /// Ids in `[item_base, item_end)` are pickups, scored on `pickup_worth` (skipped when full).
    pub item_base: i64,
    pub item_end: i64,
    pub pickup_worth: f64,
    /// The `+ 40.0` under the distance.
    pub bias: f64,
    /// The `distance <= 0.001` skip.
    pub near: f64,
}

impl Default for ForageRules {
    fn default() -> Self {
        Self {
            piece_limit: 1_000_000,
            item_base: 3_000_000,
            item_end: 4_000_000,
            pickup_worth: 220.0,
            bias: 40.0,
            near: 0.001,
        }
    }
}

/// A piece seen by [`Grid::forage`], in query order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rival {
    pub id: i64,
    pub x: f32,
    pub y: f32,
    /// `origin.distance_to(at)` (`f32`) widened.
    pub distance: f64,
}

/// [`Grid::forage`]'s answer: the best snack (score `-inf` at `(0, 0)` when none) and the
/// pieces in sight, in query order.
#[derive(Clone, Debug, PartialEq)]
pub struct Forage {
    pub snack_score: f64,
    pub snack_x: f32,
    pub snack_y: f32,
    pub rivals: Vec<Rival>,
}

/// `GS2DGrid`'s state, with a worth per id.
pub struct Grid {
    cell: f64,
    cells: Map<Vec<i64>>,
    entries: Map<Entry>,
    scratch: Vec<i64>,
}

impl Default for Grid {
    fn default() -> Self {
        Self::new(128.0)
    }
}

impl Grid {
    /// The script's `_init`: the cell size, at least `1`.
    pub fn new(cell: f64) -> Self {
        Self {
            cell: cell.max(1.0),
            cells: Map::default(),
            entries: Map::default(),
            scratch: Vec::new(),
        }
    }

    pub fn cell(&self) -> f64 {
        self.cell
    }

    /// Sets the cell size (at least `1`); only meaningful while empty, as in the script.
    pub fn set_cell(&mut self, cell: f64) {
        self.cell = cell.max(1.0);
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn has(&self, id: i64) -> bool {
        self.entries.contains_key(&id)
    }

    /// The script's `position_of`: `(0, 0)` for an id not placed.
    pub fn position_of(&self, id: i64) -> (f32, f32) {
        self.entries.get(&id).map_or((0.0, 0.0), |e| (e.x, e.y))
    }

    pub fn worth_of(&self, id: i64) -> f64 {
        self.entries.get(&id).map_or(0.0, |e| e.worth)
    }

    /// The ids in cell `key`, in the script's order (empty for none).
    pub fn bucket(&self, key: i64) -> &[i64] {
        self.cells.get(&key).map_or(&[], Vec::as_slice)
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.entries.clear();
    }

    #[inline]
    fn key_of(&self, x: f32, y: f32) -> i64 {
        key(cell_of(x, self.cell), cell_of(y, self.cell))
    }

    /// The script's `place`: adds or moves an id (same cell: only the position and worth move).
    pub fn place(&mut self, id: i64, x: f32, y: f32, worth: f64) {
        let key = self.key_of(x, y);
        if let Some(entry) = self.entries.get_mut(&id) {
            let previous = entry.home;
            *entry = Entry {
                home: key,
                x,
                y,
                worth,
            };
            if previous == key {
                return;
            }
            self.leave(previous, id);
        } else {
            self.entries.insert(
                id,
                Entry {
                    home: key,
                    x,
                    y,
                    worth,
                },
            );
        }
        self.cells.entry(key).or_default().push(id);
    }

    /// The script's `remove`.
    pub fn remove(&mut self, id: i64) {
        if let Some(entry) = self.entries.remove(&id) {
            self.leave(entry.home, id);
        }
    }

    /// The script's `_remove_from_cell`: swap-and-pop, and an emptied cell dropped.
    fn leave(&mut self, key: i64, id: i64) {
        let Some(bucket) = self.cells.get_mut(&key) else {
            return;
        };
        if let Some(index) = bucket.iter().position(|&e| e == id) {
            bucket.swap_remove(index);
        }
        if bucket.is_empty() {
            self.cells.remove(&key);
        }
    }

    /// Appends ids to cell `key` in order, each with its position and worth, for adopting a
    /// grid the script already built: the lists come out exactly as the script holds them.
    pub fn adopt_cell(&mut self, key: i64, ids: &[(i64, f32, f32, f64)]) {
        if ids.is_empty() {
            return;
        }
        let bucket = self.cells.entry(key).or_default();
        for &(id, x, y, worth) in ids {
            bucket.push(id);
            self.entries.insert(
                id,
                Entry {
                    home: key,
                    x,
                    y,
                    worth,
                },
            );
        }
    }

    /// The script's `query_rect`: ids other than `exclude` whose position is inside `rect`,
    /// cells `x` then `y`, each list in order.
    pub fn query_rect(&self, rect: &Rect, exclude: i64, out: &mut Vec<i64>) {
        out.clear();
        let min_x = cell_of(rect.x, self.cell);
        let max_x = cell_of(rect.x + rect.w, self.cell);
        let min_y = cell_of(rect.y, self.cell);
        let max_y = cell_of(rect.y + rect.h, self.cell);
        for cx in min_x..=max_x {
            for cy in min_y..=max_y {
                let Some(bucket) = self.cells.get(&key(cx, cy)) else {
                    continue;
                };
                for &id in bucket {
                    if id == exclude {
                        continue;
                    }
                    let (x, y) = self.position_of(id);
                    if rect.has_point(x, y) {
                        out.push(id);
                    }
                }
            }
        }
    }

    /// `HungryBot.command_for`'s neighbour loop around `origin` within `sight` (the script's
    /// `f64`, rounded to `f32` by its `Vector2`): the rect query, then per id the `f32`
    /// distance, the near skip, pieces handed back, pickups (when `pickups_ok`) and food scored
    /// in the script's `f64`, the first of equals kept.
    pub fn forage(
        &mut self,
        rules: &ForageRules,
        origin: (f32, f32),
        sight: f64,
        exclude: i64,
        pickups_ok: bool,
    ) -> Forage {
        let s = sight as f32;
        let rect = Rect {
            x: origin.0 - s,
            y: origin.1 - s,
            w: s * 2.0,
            h: s * 2.0,
        };
        let mut out = Forage {
            snack_score: f64::NEG_INFINITY,
            snack_x: 0.0,
            snack_y: 0.0,
            rivals: Vec::new(),
        };
        let mut ids = std::mem::take(&mut self.scratch);
        self.query_rect(&rect, exclude, &mut ids);
        for &id in &ids {
            let Some(e) = self.entries.get(&id) else {
                continue;
            };
            let dx = origin.0 - e.x;
            let dy = origin.1 - e.y;
            let distance = f64::from((dx * dx + dy * dy).sqrt());
            if distance <= rules.near {
                continue;
            }
            if id < rules.piece_limit {
                out.rivals.push(Rival {
                    id,
                    x: e.x,
                    y: e.y,
                    distance,
                });
                continue;
            }
            let worth = if id >= rules.item_base && id < rules.item_end {
                if !pickups_ok {
                    continue;
                }
                rules.pickup_worth
            } else {
                e.worth
            };
            let score = worth / (distance + rules.bias);
            if score > out.snack_score {
                out.snack_score = score;
                out.snack_x = e.x;
                out.snack_y = e.y;
            }
        }
        self.scratch = ids;
        out
    }
}

/// What a bot makes of the pieces it sees ([`judge`]): the nearest enemy piece, the nearest that
/// can eat the head and the nearest it can eat. A `-1` id and an infinite distance are "none";
/// the threat is a place (`(0, 0)` when none), the prey and the enemy are piece ids.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Judged {
    pub threat_distance: f64,
    pub threat_x: f32,
    pub threat_y: f32,
    pub prey: i64,
    pub prey_distance: f64,
    pub enemy: i64,
    pub enemy_distance: f64,
}

impl Default for Judged {
    fn default() -> Self {
        Self {
            threat_distance: f64::INFINITY,
            threat_x: 0.0,
            threat_y: 0.0,
            prey: -1,
            prey_distance: f64::INFINITY,
            enemy: -1,
            enemy_distance: f64::INFINITY,
        }
    }
}

/// `HungryBot._judge` over [`Forage::rivals`] in their order: `piece` reads a rival's owner and
/// mass (`None`: gone, skipped). A rival of `owner`'s own is skipped; any other is an enemy, a
/// threat at `mass >= head_mass * fear_ratio`, else prey at `head_mass >= mass * eat_ratio`, each
/// the nearest (the first of equals), in the script's `f64` arithmetic.
pub fn judge(
    rivals: &[Rival],
    mut piece: impl FnMut(i64) -> Option<(i64, f64)>,
    owner: i64,
    head_mass: f64,
    fear_ratio: f64,
    eat_ratio: f64,
) -> Judged {
    let mut out = Judged::default();
    for r in rivals {
        let Some((their_owner, mass)) = piece(r.id) else {
            continue;
        };
        if their_owner == owner {
            continue;
        }
        if r.distance < out.enemy_distance {
            out.enemy_distance = r.distance;
            out.enemy = r.id;
        }
        if mass >= head_mass * fear_ratio {
            if r.distance < out.threat_distance {
                out.threat_distance = r.distance;
                out.threat_x = r.x;
                out.threat_y = r.y;
            }
        } else if head_mass >= mass * eat_ratio && r.distance < out.prey_distance {
            out.prey_distance = r.distance;
            out.prey = r.id;
        }
    }
    out
}

#[cfg(test)]
mod tests;
