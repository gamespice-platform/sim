//! The rest of a common's think that reads only the tick's tables: what its chase does once [`plan_chase`]
//! has found the quarry (`ZzCommonBrain._chase`'s choice), whether it is pressed against a barricade
//! (`ZzHorde.barricade_in_way`), and how far the nearest survivor is (`ZzHorde.nearest_survivor_distance`).
//! Each in the script's order and precision, so the script answers stay the reference.

use crate::{Chaser, FORCED, Plan, Rules, Survivor, flat_distance, plan_chase};
use dot_sim_math::{Vec3, clampf, maxf, minf};
use std::collections::HashMap;

/// [`Decision::act`]: no quarry (the plan's code is below zero).
pub const ACT_NONE: i32 = 0;
/// [`Decision::act`]: in reach and level with it: halt, face it, swing unless already swinging.
pub const ACT_HIT: i32 = 1;
/// [`Decision::act`]: run at it (`chase_to`).
pub const ACT_CHASE: i32 = 2;
/// [`Decision::act`]: mid-swing out of reach: drift toward it at a third of the speed.
pub const ACT_SLOW: i32 = 3;

/// How far above or below its quarry a common can still land a blow, in metres.
pub const HIT_RISE: f64 = 1.6;
/// Metres past its ring a common goes in without asking for an attack slot.
pub const SLOT_FROM: f64 = 3.0;

/// One common's chase for the think: the [`Plan`], what it does, and whether it must ask for an attack slot first
/// (a denied ask holds the ring instead of `act`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decision {
    /// The quarry, its speed, gap and reach.
    pub plan: Plan,
    /// [`ACT_NONE`], [`ACT_HIT`], [`ACT_CHASE`] or [`ACT_SLOW`], as if it attacks.
    pub act: i32,
    /// Close enough to the quarry and not forced: `claim_attack_slot()` decides whether it attacks.
    pub ask: bool,
}

/// `ZzCommonBrain._chase`'s choice for `z`, holding `ring`, mid-swing or not.
pub fn decide_chase(
    survivors: &[Survivor],
    rules: &Rules,
    z: &Chaser,
    ring: f64,
    swinging: bool,
) -> Decision {
    let plan = plan_chase(survivors, rules, z);

    if plan.code < 0 {
        return Decision {
            plan,
            act: ACT_NONE,
            ask: false,
        };
    }

    // `forced != &"" or gap > ring + 3.0 or claim_attack_slot()`: forced outlives the plan only when it is chased.
    let ask = plan.code != FORCED && plan.gap <= ring + SLOT_FROM;
    let level = (f64::from(z.here.y) - f64::from(plan.at.y)).abs() < HIT_RISE;
    let act = if plan.gap <= plan.reach && level {
        ACT_HIT
    } else if !swinging {
        ACT_CHASE
    } else {
        ACT_SLOW
    };

    Decision { plan, act, ask }
}

/// `ZzHorde.nearest_survivor_distance`: the flat metres from `at` to the nearest living survivor, `INF` with none.
pub fn nearest(survivors: &[Survivor], at: Vec3) -> f64 {
    survivors
        .iter()
        .filter(|s| s.alive)
        .fold(f64::INFINITY, |best, s| minf(best, flat_distance(s.at, at)))
}

/// One intact barricade: its box as the engine's `AABB` holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Barricade {
    /// `AABB.position`.
    pub position: Vec3,
    /// `AABB.size`.
    pub size: Vec3,
}

impl Barricade {
    /// `AABB.end`, in `f32`.
    #[inline]
    pub fn end(&self) -> Vec3 {
        self.position + self.size
    }

    /// `AABB.get_center()`, in `f32`.
    #[inline]
    pub fn centre(&self) -> Vec3 {
        Vec3::new(
            self.position.x + self.size.x * 0.5,
            self.position.y + self.size.y * 0.5,
            self.position.z + self.size.z * 0.5,
        )
    }
}

/// `ZzHorde`'s barricades and the grid of cells beside each (`_barricade_cells`).
#[derive(Clone, Debug, Default)]
pub struct Barricades {
    boxes: Vec<Barricade>,
    reach: f64,
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

/// The script's `floori`.
#[inline]
fn floori(v: f64) -> i64 {
    v.floor() as i64
}

impl Barricades {
    /// `ZzHorde.set_barricades`: `boxes` in the order the horde's dictionary holds them, each filed under every
    /// `cell`-metre cell its box grown by `reach + 0.5` touches, pressed against within `reach`. `None` for a cell
    /// size that is not above zero.
    pub fn new(boxes: Vec<Barricade>, cell: f64, reach: f64) -> Option<Self> {
        if cell.is_nan() || cell <= 0.0 {
            return None;
        }

        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        // `AABB.grow`: by an `f32`, the position less it and the size two of it more.
        let by = (reach + 0.5) as f32;

        for (i, b) in boxes.iter().enumerate() {
            let pos = Vec3::new(b.position.x - by, b.position.y - by, b.position.z - by);
            let size = Vec3::new(
                b.size.x + 2.0 * by,
                b.size.y + 2.0 * by,
                b.size.z + 2.0 * by,
            );
            let end = pos + size;

            for cx in floori(f64::from(pos.x) / cell)..=floori(f64::from(end.x) / cell) {
                for cz in floori(f64::from(pos.z) / cell)..=floori(f64::from(end.z) / cell) {
                    cells.entry((cx, cz)).or_default().push(i);
                }
            }
        }

        Some(Self {
            boxes,
            reach,
            cell,
            cells,
        })
    }

    /// How many barricades the table holds.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// No barricades.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// `ZzHorde.barricade_in_way`: the row of the barricade a zombie at `at` wishing `wish` is pressed against.
    pub fn in_way(&self, at: Vec3, wish: Vec3) -> Option<usize> {
        if self.cells.is_empty() {
            return None;
        }

        let (x, y, z) = (f64::from(at.x), f64::from(at.y), f64::from(at.z));
        let rows = self
            .cells
            .get(&(floori(x / self.cell), floori(z / self.cell)))?;

        for &i in rows {
            let b = &self.boxes[i];
            let end = b.end();
            let (px, py, pz) = (
                f64::from(b.position.x),
                f64::from(b.position.y),
                f64::from(b.position.z),
            );
            let (ex, ey, ez) = (f64::from(end.x), f64::from(end.y), f64::from(end.z));
            let dx = maxf(maxf(px - x, x - ex), 0.0);
            let dz = maxf(maxf(pz - z, z - ez), 0.0);

            if dx * dx + dz * dz > self.reach * self.reach || y > ey || y + 1.8 < py {
                continue;
            }

            // Toward the nearest point of the boards, each difference rounded into the `f32` vector.
            let mut toward = Vec3::new(
                (clampf(x, px, ex) - x) as f32,
                0.0,
                (clampf(z, pz, ez) - z) as f32,
            );

            if f64::from(toward.length_squared()) < 0.0001 {
                toward = b.centre() - at;
                toward.y = 0.0;
            }

            if wish.dot(toward) > 0.0 {
                return Some(i);
            }
        }

        None
    }
}
