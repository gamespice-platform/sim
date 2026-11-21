//! `GSSimGrid2D`: dot-2d's `GS2DGrid` kept in step by the game, and Hungario's bot forage.
//!
//! The game mirrors each `place`, `remove` and `clear` of its script grid here (with a worth per
//! id: Hungario's mass of a pellet), or adopts a grid the script already built in one call
//! (`adopt`, the cells' lists in order). Then `forage` is `HungryBot.command_for`'s neighbour loop
//! in one call instead of a rect query, a `position_of` and a `mass_of` per id: the best snack
//! and the pieces in sight, to the bit (`dot_sim_grid2d`; game-hungario's
//! `headless_native_forage` and its `forage_verify` hold the two together). `forage_judged` also
//! runs `HungryBot._judge` over those pieces in the same call, reading each one's `owner_id` and
//! `state.mass` from the world's pieces, so no piece crosses back to the script.

use dot_sim_grid2d::{ForageRules, Grid, Rect, judge};
use godot::prelude::*;

/// A script grid's ids, cells and positions, with a worth per id, and the bot forage over them.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimGrid2D {
    grid: Grid,
    rules: ForageRules,
    query: Vec<i64>,
    names: Option<[StringName; 3]>,
}

#[godot_api]
impl GSSimGrid2D {
    /// The script grid's `cell_size` (at least 1). Set it while empty.
    #[func]
    fn set_cell_size(&mut self, cell_size: f64) {
        self.grid.set_cell(cell_size);
    }

    #[func]
    fn cell_size(&self) -> f64 {
        self.grid.cell()
    }

    /// `GS2DGrid.place` with the id's worth.
    #[func]
    fn place(&mut self, id: i64, position: Vector2, worth: f64) {
        self.grid.place(id, position.x, position.y, worth);
    }

    /// `GS2DGrid.remove`.
    #[func]
    fn remove(&mut self, id: i64) {
        self.grid.remove(id);
    }

    /// `GS2DGrid.clear`.
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
                "GSSimGrid2D.adopt: {} keys, {} counts, {} ids, {} positions, {} worths",
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
                godot_error!("GSSimGrid2D.adopt: cell count {count} runs past the ids");
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

    /// `GS2DGrid.query_rect`: the same ids in the same order.
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

    /// `forage` and then `HungryBot._judge` over its pieces, in one call: `[snack_score, snack_x,
    /// snack_y, threat_distance, threat_x, threat_y, prey_id, prey_distance, enemy_id,
    /// enemy_distance]`. [param pieces] is the world's id -> piece (`owner_id`, `state.mass`); a
    /// piece missing there is skipped, as `_judge` skips a null one. An id of `-1` and an infinite
    /// distance are "none".
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn forage_judged(
        &mut self,
        origin: Vector2,
        sight: f64,
        exclude: i64,
        pickups_ok: bool,
        pieces: VarDictionary,
        owner: i64,
        head_mass: f64,
        fear_ratio: f64,
        eat_ratio: f64,
    ) -> PackedFloat64Array {
        let f = self.grid.forage(
            &self.rules,
            (origin.x, origin.y),
            sight,
            exclude,
            pickups_ok,
        );
        let [owner_id, state, mass] = self.names.get_or_insert_with(|| {
            [
                StringName::from("owner_id"),
                StringName::from("state"),
                StringName::from("mass"),
            ]
        });
        let number = |v: Variant| {
            v.try_to::<f64>()
                .ok()
                .or_else(|| v.try_to::<i64>().ok().map(|n| n as f64))
        };
        let j = judge(
            &f.rivals,
            |id| {
                let piece = pieces.get(id)?.try_to::<Gd<Object>>().ok()?;
                let their_owner = piece.get(&*owner_id).try_to::<i64>().ok()?;
                let state = piece.get(&*state).try_to::<Gd<Object>>().ok()?;
                Some((their_owner, number(state.get(&*mass))?))
            },
            owner,
            head_mass,
            fear_ratio,
            eat_ratio,
        );
        PackedFloat64Array::from(
            &[
                f.snack_score,
                f64::from(f.snack_x),
                f64::from(f.snack_y),
                j.threat_distance,
                f64::from(j.threat_x),
                f64::from(j.threat_y),
                j.prey as f64,
                j.prey_distance,
                j.enemy as f64,
                j.enemy_distance,
            ][..],
        )
    }
}
