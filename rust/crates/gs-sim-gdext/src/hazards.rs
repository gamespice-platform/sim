//! `GSSimHazards`: mg-wipeout's `WoCourse.knock` and `path_clear` over a table of the course's
//! hazards.
//!
//! The course writes each hazard once after it is built (`add`: its root frame, the spec's numbers,
//! the cull sphere), then asks `knock` or a whole `path_clear` in one call each instead of the
//! script's per-piece dictionary reads, transforms and segment tests. The same answer as the
//! script to the bit (`gs_sim_hazards`; the game's `headless_native_knock` and its
//! `knock_verify` hold the two together). Bots ask it; the players' own knock stays in script.

use gs_sim_hazards::{Hazard, Hazards, Kind};
use gs_sim_math::{Vec3, Xform};
use godot::prelude::*;

/// A course's hazards and the knock asks over them.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct GSSimHazards {
    table: Hazards,
}

#[godot_api]
impl IRefCounted for GSSimHazards {
    fn init(_base: Base<RefCounted>) -> Self {
        Self {
            table: Hazards::new(64),
        }
    }
}

#[godot_api]
impl GSSimHazards {
    /// Forgets every hazard.
    #[func]
    fn clear(&mut self) {
        self.table.hazards.clear();
    }

    /// The stage's `tick_rate`, turning ticks into seconds.
    #[func]
    fn set_tick_rate(&mut self, tick_rate: i64) {
        self.table.tick_rate = tick_rate;
    }

    /// The stage's `cull_hazards`.
    #[func]
    fn set_cull(&mut self, cull: bool) {
        self.table.cull = cull;
    }

    /// How many hazards are held.
    #[func]
    fn count(&self) -> i64 {
        self.table.hazards.len() as i64
    }

    /// Adds the next hazard in the course's order. [param kind] is `spinner`, `pendulum`, `pusher`
    /// or `fall`; [param frame] the root's transform (a pendulum's `Transform3D(_yaw_basis(yaw),
    /// pivot)`); [param aux] a pusher's `Transform3D(_yaw_basis(yaw), at)` or a fall's
    /// `root.transform.affine_inverse()`, ignored otherwise; [param centre] and [param reach] the
    /// script's `_hazard_centre` and `_hazard_reach`. [param params] are the spec's numbers:
    /// spinner `[arm_height, arm_length, hub_radius, thickness, arms, phase, speed]`, pendulum
    /// `[length, radius, swing, period, phase]`, pusher and fall `[size.x, size.y, size.z,
    /// reach | push, period, phase, duty]`. An unknown kind or a wrong count is an error and adds
    /// nothing.
    #[func]
    fn add(
        &mut self,
        kind: GString,
        frame: Transform3D,
        aux: Transform3D,
        params: PackedFloat64Array,
        centre: Vector3,
        reach: f64,
    ) -> bool {
        let p = params.as_slice();
        let want = match kind.to_string().as_str() {
            "spinner" => 7,
            "pendulum" => 5,
            "pusher" | "fall" => 7,
            other => {
                godot_error!("GSSimHazards.add: unknown kind {other:?}");
                return false;
            }
        };
        if p.len() != want {
            godot_error!(
                "GSSimHazards.add: {kind} takes {want} numbers, got {}",
                p.len()
            );
            return false;
        }
        let size = || Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        let kind = match kind.to_string().as_str() {
            "spinner" => Kind::Spinner {
                arm_height: p[0],
                arm_length: p[1],
                hub_radius: p[2],
                thickness: p[3],
                arms: p[4] as i64,
                phase: p[5],
                speed: p[6],
            },
            "pendulum" => Kind::Pendulum {
                length: p[0],
                radius: p[1],
                swing: p[2],
                period: p[3],
                phase: p[4],
            },
            "pusher" => Kind::Pusher {
                yaw: xf(&aux),
                size: size(),
                reach: p[3],
                period: p[4],
                phase: p[5],
                duty: p[6],
            },
            _ => Kind::Fall {
                inverse: xf(&aux),
                size: size(),
                push: p[3],
                period: p[4],
                phase: p[5],
                duty: p[6],
            },
        };
        self.table.hazards.push(Hazard {
            frame: xf(&frame),
            centre: v3(centre),
            reach,
            kind,
        });
        true
    }

    /// `WoCourse.knock`: the velocity the first hazard touching the capsule at [param feet] throws
    /// it with at [param tick], [param limits] being `(minimum speed, maximum speed, lift)`.
    #[func]
    fn knock(&self, feet: Vector3, crouched: bool, tick: i64, limits: Vector3) -> Vector3 {
        let v = self.table.knock(v3(feet), crouched, tick, v3(limits));
        Vector3::new(v.x, v.y, v.z)
    }

    /// `WoCourse.path_clear`: whether a run from [param from] to [param to] starting at
    /// [param tick] meets no hazard.
    #[func]
    fn path_clear(
        &self,
        from: Vector3,
        to: Vector3,
        tick: i64,
        speed: f64,
        margin_seconds: f64,
    ) -> bool {
        self.table
            .path_clear(v3(from), v3(to), tick, speed, margin_seconds)
    }
}

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn xf(t: &Transform3D) -> Xform {
    Xform {
        rows: [
            v3(t.basis.rows[0]),
            v3(t.basis.rows[1]),
            v3(t.basis.rows[2]),
        ],
        origin: v3(t.origin),
    }
}
