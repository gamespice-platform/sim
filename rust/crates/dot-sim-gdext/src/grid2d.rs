//! `DotSimGrid2D`: dot-2d's `Dot2DGrid` kept in step by the game, and Hungario's bot forage.
//!
//! The game mirrors each `place`, `remove` and `clear` of its script grid here (with a worth per
//! id: Hungario's mass of a pellet), or adopts a grid the script already built in one call
//! (`adopt`, the cells' lists in order). Then `forage` is `HungryBot.command_for`'s neighbour loop
//! in one call instead of a rect query, a `position_of` and a `mass_of` per id: the best snack
//! and the pieces in sight, to the bit (`dot_sim_grid2d`; game-hungario's
//! `headless_native_forage` and its `forage_verify` hold the two together).

use dot_sim_grid2d::{ForageRules, Grid, Rect};
use godot::prelude::*;

/// A script grid's ids, cells and positions, with a worth per id, and the bot forage over them.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimGrid2D {
    grid: Grid,
    rules: ForageRules,
    query: Vec<i64>,
}

#[godot_api]
impl DotSimGrid2D {
    /// The script grid's `cell_size` (at least 1). Set it while empty.
    #[func]
    fn set_cell_size(&mut self, cell_size: f64) {
        self.grid.set_cell(cell_size);
    }

    #[func]
    fn cell_size(&self) -> f64 {
        self.grid.cell()
    }

    /// `Dot2DGrid.place` with the id's worth.
    #[func]
    fn place(&mut self, id: i64, position: Vector2, worth: f64) {
        self.grid.place(id, position.x, position.y, worth);
    }

    /// `Dot2DGrid.remove`.
    #[func]
    fn remove(&mut self, id: i64) {
        self.grid.remove(id);
    }

    /// `Dot2DGrid.clear`.
    #[func]
    fn clear(&mut self) {
        self.grid.clear();
    }

    #[func]
    fn size(&self) -> i64 {
        self.grid.size() as i64
    }

    #[func]
    fn cell_count(&self) -> i64 {
        self.grid.cell_count() as i64
    }

    #[func]
    fn has(&self, id: i64) -> bool {
        self.grid.has(id)
    }

    #[func]
    fn position_of(&self, id: i64) -> Vector2 {
        let (x, y) = self.grid.position_of(id);
        Vector2::new(x, y)
    }

    #[func]
    fn worth_of(&self, id: i64) -> f64 {
        self.grid.worth_of(id)
    }

    /// Replaces everything with a script grid's cells: per cell its key and how many ids follow,
    /// then the ids in each list's order with their positions and worths. A count that runs past
    /// the ids is an error and leaves the grid empty.
    #[func]
    fn adopt(
        &mut self,
        keys: PackedInt64Array,
        counts: PackedInt64Array,
        ids: PackedInt64Array,
        positions: PackedVector2Array,
        worths: PackedFloat64Array,
    ) -> bool {
        self.grid.clear();
        let (keys, counts) = (keys.as_slice(), counts.as_slice());
        let (ids, positions, worths) = (ids.as_slice(), positions.as_slice(), worths.as_slice());
        if keys.len() != counts.len() || ids.len() != positions.len() || ids.len() != worths.len() {
            godot_error!(
                "DotSimGrid2D.adopt: {} keys, {} counts, {} ids, {} positions, {} worths",
                keys.len(),
                counts.len(),
                ids.len(),
                positions.len(),
                worths.len()
            );
            return false;
        }
        let mut at = 0usize;
        let mut cell = Vec::new();
        for (&key, &count) in keys.iter().zip(counts) {
            let Some(end) = usize::try_from(count)
                .ok()
                .and_then(|n| at.checked_add(n))
                .filter(|&end| end <= ids.len())
            else {
                godot_error!("DotSimGrid2D.adopt: cell count {count} runs past the ids");
                self.grid.clear();
                return false;
            };
            cell.clear();
            cell.extend((at..end).map(|i| (ids[i], positions[i].x, positions[i].y, worths[i])));
            self.grid.adopt_cell(key, &cell);
            at = end;
        }
        true
    }

    /// `Dot2DGrid.query_rect`: the same ids in the same order.
    #[func]
    fn query_rect(&mut self, rect: Rect2, exclude: i64) -> PackedInt64Array {
        let r = Rect {
            x: rect.position.x,
            y: rect.position.y,
            w: rect.size.x,
            h: rect.size.y,
        };
        self.grid.query_rect(&r, exclude, &mut self.query);
        PackedInt64Array::from(&self.query[..])
    }

    /// The id ranges and scoring `forage` uses: pieces below [param piece_limit], pickups in
    /// [`item_base`, `item_end`) worth [param pickup_worth], score `worth / (distance + bias)`,
    /// and nothing within [param near]. Hungario's are the defaults.
    #[func]
    fn set_forage_rules(
        &mut self,
        piece_limit: i64,
        item_base: i64,
        item_end: i64,
        pickup_worth: f64,
        bias: f64,
        near: f64,
    ) {
        self.rules = ForageRules {
            piece_limit,
            item_base,
            item_end,
            pickup_worth,
            bias,
            near,
        };
    }

    /// `HungryBot.command_for`'s neighbour loop: `[snack_score, snack_x, snack_y, pieces, then
    /// per piece id, x, y, distance]`, the pieces in the script's query order. The snack score is
    /// `-inf` (at `0, 0`) when nothing edible is in sight.
    #[func]
    fn forage(
        &mut self,
        origin: Vector2,
        sight: f64,
        exclude: i64,
        pickups_ok: bool,
    ) -> PackedFloat64Array {
        let f = self.grid.forage(
            &self.rules,
            (origin.x, origin.y),
            sight,
            exclude,
            pickups_ok,
        );
        let mut out = Vec::with_capacity(4 + f.rivals.len() * 4);
        out.extend_from_slice(&[
            f.snack_score,
            f64::from(f.snack_x),
            f64::from(f.snack_y),
            f.rivals.len() as f64,
        ]);
        for r in &f.rivals {
            out.extend_from_slice(&[r.id as f64, f64::from(r.x), f64::from(r.y), r.distance]);
        }
        PackedFloat64Array::from(&out[..])
    }
}
