//! `DotSimMotorBatch`: a run of players' whole motor ticks in one call, so a server's players'
//! loop crosses into native code once per run instead of once per player.
//!
//! Each row is `DotSimFpsBody.motor_tick` to the bit, in roster order: a body in hulls mode 3
//! gathers from the hull world as the single call does, and once it has ticked the world holds
//! its new feet, as the game's `moved` after that player's tick would. The old feet are kept so
//! the caller can take rows back when a player ends its tick somewhere else.

use crate::hulls::DotSimHullWorld;
use crate::motor::{DotSimFpsBody, TICK_IN_LEN, TICK_OUT_LEN};
use dot_sim_motor::Vec3 as V3;
use godot::prelude::*;

/// Per row in [`DotSimMotorBatch::run`]'s `meta`: hulls mode (0 or 3), collision mask, pass-out
/// mask; [`DotSimMotorBatch::run_rows`] adds the row's timers flags (`tick_into`'s `timers`).
const META_LEN: usize = 3;
const ROWS_META_LEN: usize = 4;

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimMotorBatch {
    /// Row, world, slot and the feet the world held before the row ran.
    moved: Vec<(usize, Gd<DotSimHullWorld>, i64, V3)>,
    out: Vec<f64>,
    rows: i64,
    runs: i64,
}

#[godot_api]
impl DotSimMotorBatch {
    /// Starts a players' loop: forgets the feet kept for [`Self::rollback`].
    #[func]
    fn begin(&mut self) {
        self.moved.clear();
    }

    /// Ticks rows `from` to `to` (exclusive) in order: `bodies` the `DotSimFpsBody` per row,
    /// `tuns` its tunables, `meta` three values per row, `input` `motor_tick`'s state per row.
    /// Answers `motor_tick`'s values per row ticked; shorter than asked when a row has no native
    /// answer (the rows before it stand, that row and the ones after are the caller's).
    #[func]
    fn run(
        &mut self,
        bodies: VarArray,
        tuns: VarArray,
        meta: PackedInt64Array,
        input: PackedFloat64Array,
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        self.ticks(
            &bodies,
            &tuns,
            meta.as_slice(),
            META_LEN,
            input.as_slice(),
            from,
            to,
        )
    }

    /// [`Self::run`] with four values per row in `meta`, the fourth `tick_into`'s `timers`: a row
    /// may leave its clocks and cooldowns to this call; dot-player-controller 31 and later call it.
    #[func]
    fn run_rows(
        &mut self,
        bodies: VarArray,
        tuns: VarArray,
        meta: PackedInt64Array,
        input: PackedFloat64Array,
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        self.ticks(
            &bodies,
            &tuns,
            meta.as_slice(),
            ROWS_META_LEN,
            input.as_slice(),
            from,
            to,
        )
    }

    /// Takes rows `from` and later back: the hull world holds again the feet it held before
    /// each of them ran. The caller ticks them again or by itself.
    #[func]
    fn rollback(&mut self, from: i64) {
        while let Some((row, _, _, _)) = self.moved.last() {
            if (*row as i64) < from {
                break;
            }
            if let Some((_, mut world, slot, old)) = self.moved.pop() {
                world.bind_mut().set_feet(slot, old);
            }
        }
    }

    /// Rows ticked and `run` calls since the last reset.
    #[func]
    fn counters(&self) -> PackedInt64Array {
        PackedInt64Array::from(&[self.rows, self.runs][..])
    }

    #[func]
    fn reset_counters(&mut self) {
        self.rows = 0;
        self.runs = 0;
    }
}

impl DotSimMotorBatch {
    #[allow(clippy::too_many_arguments)]
    fn ticks(
        &mut self,
        bodies: &VarArray,
        tuns: &VarArray,
        mp: &[i64],
        stride: usize,
        ip: &[f64],
        from: i64,
        to: i64,
    ) -> PackedFloat64Array {
        let hulls = VarArray::new();
        self.out.clear();
        self.runs += 1;
        let from = usize::try_from(from).unwrap_or(0);
        let to = usize::try_from(to)
            .unwrap_or(0)
            .min(bodies.len())
            .min(tuns.len())
            .min(mp.len() / stride)
            .min(ip.len() / TICK_IN_LEN);
        for row in from..to {
            let (Some(bv), Some(tv)) = (bodies.get(row), tuns.get(row)) else {
                break;
            };
            let (Ok(mut body), Ok(tun)) = (
                bv.try_to::<Gd<DotSimFpsBody>>(),
                tv.try_to::<PackedFloat64Array>(),
            ) else {
                break;
            };
            let m = &mp[row * stride..(row + 1) * stride];
            let timers = if stride > META_LEN { m[META_LEN] } else { 0 };
            let sp = &ip[row * TICK_IN_LEN..(row + 1) * TICK_IN_LEN];
            let at = self.out.len();
            let mut b = body.bind_mut();
            if !b.tick_into(
                sp,
                tun.as_slice(),
                m[0],
                &hulls,
                m[1],
                m[2],
                timers,
                &mut self.out,
            ) {
                break;
            }
            debug_assert_eq!(self.out.len(), at + TICK_OUT_LEN);
            self.rows += 1;
            if m[0] != 3 {
                continue;
            }
            let Some((mut world, slot)) = b.hull_world() else {
                continue;
            };
            let mut w = world.bind_mut();
            if let Some(old) = w.feet(slot) {
                let o = &self.out[at..];
                w.set_feet(slot, V3::new(o[0] as f32, o[1] as f32, o[2] as f32));
                drop(w);
                self.moved.push((row, world, slot, old));
            }
        }
        PackedFloat64Array::from(&self.out[..])
    }
}
