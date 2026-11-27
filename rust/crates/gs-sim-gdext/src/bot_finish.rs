//! `GSSimBotFinish`: gs-bot's GSBotIntentTable.finish (API 52) over a whole table in one call:
//! the stick of every row limited to length 1 and the command buttons through the game's maps.

use godot::prelude::*;
use gs_sim_brain::finish::row;
use gs_sim_brain::num::Vec2;

/// Stateless: [`Self::write`] is static.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBotFinish {}

#[godot_api]
impl GSSimBotFinish {
    /// `[cmd_move, move_buttons, fire_buttons]` for `moves` and `buttons` (one row each; rows past
    /// the shorter column are zero), the script twin's columns to the bit.
    #[func]
    fn write(
        moves: PackedVector2Array,
        buttons: PackedInt32Array,
        move_map: PackedInt32Array,
        fire_map: PackedInt32Array,
    ) -> VarArray {
        let (moves, buttons) = (moves.as_slice(), buttons.as_slice());
        let (move_map, fire_map) = (move_map.as_slice(), fire_map.as_slice());
        let n = moves.len();
        let mut out_moves = Vec::with_capacity(n);
        let mut out_mb = Vec::with_capacity(n);
        let mut out_fb = Vec::with_capacity(n);
        for (i, v) in moves.iter().enumerate() {
            let b = buttons.get(i).copied().unwrap_or(0);
            let (m, mb, fb) = row(Vec2::new(v.x, v.y), b, move_map, fire_map);
            out_moves.push(Vector2::new(m.x, m.y));
            out_mb.push(mb);
            out_fb.push(fb);
        }
        varray![
            &PackedVector2Array::from(out_moves.as_slice()).to_variant(),
            &PackedInt32Array::from(out_mb.as_slice()).to_variant(),
            &PackedInt32Array::from(out_fb.as_slice()).to_variant()
        ]
    }
}
