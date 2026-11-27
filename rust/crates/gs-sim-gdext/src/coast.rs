//! `GSSimBotCoast`: the ticks bots sit out their think, for every coasting bot of a tick in one call: the view each
//! carries on with (FC's `FcBots._coast`) and the recoil its aim is turned against (`next_recoil_offset` times the
//! bot's share), bit for bit the script's.

use godot::prelude::*;
use gs_sim_bots::coast::coast_view;
use gs_sim_weapon::shot::{PREVIEW_LEN, recoil_preview};

/// One tick's coasting views and recoil previews.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimBotCoast {}

#[godot_api]
impl GSSimBotCoast {
    /// [param views] holds `from.x, from.y, to.x, to.y` per coasting bot and [param steps] `tick - last_think_tick,
    /// every` per bot; [param recoil] holds one 18-value row per firing bot (see gs-sim-weapon's `recoil_preview`).
    /// Returns `yaw, pitch` per bot, then `pitch, yaw` offsets per recoil row; empty, with an error logged, when the
    /// arrays do not line up.
    #[func]
    fn step(
        &self,
        views: PackedFloat32Array,
        steps: PackedInt64Array,
        recoil: PackedFloat64Array,
        tick: i64,
        tick_rate: f64,
    ) -> PackedFloat64Array {
        let (v, s, r) = (views.as_slice(), steps.as_slice(), recoil.as_slice());
        let n = s.len() / 2;
        if v.len() != n * 4 || s.len() != n * 2 || !r.len().is_multiple_of(PREVIEW_LEN) {
            godot_error!(
                "GSSimBotCoast.step: {} views, {} steps and {} recoil values do not line up",
                v.len(),
                s.len(),
                r.len()
            );
            return PackedFloat64Array::new();
        }
        let mut out = Vec::with_capacity(n * 2 + r.len() / PREVIEW_LEN * 2);
        for i in 0..n {
            let (yaw, pitch) = coast_view(
                (v[i * 4], v[i * 4 + 1]),
                (v[i * 4 + 2], v[i * 4 + 3]),
                s[i * 2],
                s[i * 2 + 1],
            );
            out.push(yaw);
            out.push(pitch);
        }
        for row in r.as_chunks::<PREVIEW_LEN>().0 {
            let (x, y) = recoil_preview(row, tick, tick_rate).unwrap_or((0.0, 0.0));
            out.push(x as f64);
            out.push(y as f64);
        }
        PackedFloat64Array::from(out.as_slice())
    }
}
