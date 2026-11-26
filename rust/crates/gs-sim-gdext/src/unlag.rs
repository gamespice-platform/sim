//! `GSSimUnlagRing`: FC's lag-compensation history (`fc_game_unlag.gd`) held natively, recorded
//! for every player in one call a tick. See [`gs_sim_unlag`] for the rules; this class only moves
//! packed arrays across.

use godot::prelude::*;
use gs_sim_unlag::Rings;

/// Every player's position ring.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimUnlagRing {
    rings: Rings,
}

#[godot_api]
impl IRefCounted for GSSimUnlagRing {
    fn init(_base: Base<RefCounted>) -> Self {
        GSSimUnlagRing {
            rings: Rings::new(1),
        }
    }
}

#[godot_api]
impl GSSimUnlagRing {
    /// The slots a ring made from now on gets (at least 1); existing rings keep theirs.
    #[func]
    fn set_history(&mut self, slots: i64) {
        self.rings.set_len(slots.max(1) as usize);
    }

    /// Records tick [param tick] for players [param ids]: feet [param positions] and crouch
    /// fractions [param crouches], row for row. A player whose previous tick lies more than
    /// [param cut] away has its ring cleared first. False (nothing recorded) when the rows differ in length.
    #[func]
    fn record(
        &mut self,
        tick: i64,
        ids: PackedInt64Array,
        positions: PackedVector3Array,
        crouches: PackedFloat32Array,
        cut: f64,
    ) -> bool {
        let (ids, pos, crouch) = (ids.as_slice(), positions.as_slice(), crouches.as_slice());
        if pos.len() != ids.len() || crouch.len() != ids.len() || tick < 0 {
            return false;
        }
        for i in 0..ids.len() {
            let p = pos[i];
            self.rings
                .record(tick, ids[i], [p.x, p.y, p.z], crouch[i], cut);
        }
        true
    }

    /// Where player [param id] was at fractional tick [param at]; null when not recorded.
    #[func]
    fn position_at(&self, id: i64, at: f64) -> Variant {
        match self.rings.position_at(id, at) {
            Some(p) => Vector3::new(p[0], p[1], p[2]).to_variant(),
            None => Variant::nil(),
        }
    }

    /// Player [param id]'s crouch fraction at [param at]; -1 when not recorded.
    #[func]
    fn crouch_at(&self, id: i64, at: f64) -> f64 {
        self.rings.crouch_at(id, at)
    }

    /// Whether player [param id] has a ring.
    #[func]
    fn has(&self, id: i64) -> bool {
        self.rings.has(id)
    }

    /// Drops player [param id]'s ring (a player who left).
    #[func]
    fn forget(&mut self, id: i64) {
        self.rings.forget(id);
    }

    /// Drops every ring.
    #[func]
    fn clear(&mut self) {
        self.rings.clear();
    }
}
