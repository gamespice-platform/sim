//! `GSSimFigureBatch`: one call per frame for a step's native bone work across every figure drawn.
//!
//! gs-player-char's `GSHumanoidFigure.animate_all` runs each figure's script up to a native call,
//! queues that call's arguments here, and makes the whole queue one call: [method restore] for the
//! `GSSimLegs.restore_with` calls, [method upper] for the `GSSimFigurePose.upper` calls and
//! [method lower] for the `GSSimLegs.lower` calls. Each runs the same functions the single calls run,
//! one figure after another on this thread (a `Gd` cannot cross threads and the web build has none),
//! so the pose is the same; what it saves is the engine's dispatch of one call per figure.
//! Drawing only.

use godot::classes::Skeleton3D;
use godot::prelude::*;

use crate::figure::{GSSimFigurePose, GSSimLegs, GSSimTurnLimit};

/// Arguments per `restore_with` call in [method GSSimFigureBatch.restore]: legs, skeleton, turn, stride.
const RESTORE_STRIDE: usize = 4;
/// Arguments per `upper` call: the pose object, then `upper`'s ten.
const UPPER_STRIDE: usize = 11;
/// Arguments per `lower` call: the legs object, then `lower`'s nine.
const LOWER_STRIDE: usize = 10;

fn obj<T: GodotClass>(v: Option<&Variant>) -> Option<Gd<T>> {
    v.and_then(|v| v.try_to::<Gd<T>>().ok())
}

fn plane(v: Option<&Variant>) -> Plane {
    v.and_then(|v| v.try_to::<Plane>().ok()).unwrap_or(Plane {
        normal: Vector3::ZERO,
        d: 0.0,
    })
}

fn val<T: FromGodot + Default>(v: Option<&Variant>) -> T {
    v.and_then(|v| v.try_to::<T>().ok()).unwrap_or_default()
}

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimFigureBatch {}

#[godot_api]
impl GSSimFigureBatch {
    /// `GSSimLegs.restore_with` for each figure in [param calls], four entries a figure (legs,
    /// skeleton, turn limit, stride limit), in order. A figure with null legs is skipped.
    #[func]
    fn restore(&self, calls: VarArray) {
        let c: Vec<Variant> = calls.iter_shared().collect();
        for f in c.chunks_exact(RESTORE_STRIDE) {
            let Some(mut legs) = obj::<GSSimLegs>(f.first()) else {
                continue;
            };
            legs.bind_mut().restore_with(
                obj::<Skeleton3D>(f.get(1)),
                obj::<GSSimTurnLimit>(f.get(2)),
                obj::<GSSimTurnLimit>(f.get(3)),
            );
        }
    }

    /// `GSSimFigurePose.upper` for each figure in [param calls], eleven entries a figure (the pose
    /// object, then `upper`'s arguments), in order. Returns each call's grip (`Vector3.INF` for a
    /// figure with no pose object).
    #[func]
    fn upper(&self, calls: VarArray) -> PackedVector3Array {
        let c: Vec<Variant> = calls.iter_shared().collect();
        let none = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut grips = PackedVector3Array::new();
        for f in c.chunks_exact(UPPER_STRIDE) {
            let grip = match obj::<GSSimFigurePose>(f.first()) {
                Some(mut pose) => pose.bind_mut().upper(
                    obj::<Skeleton3D>(f.get(1)),
                    obj::<GSSimLegs>(f.get(2)),
                    val::<Transform3D>(f.get(3)),
                    val::<PackedFloat64Array>(f.get(4)),
                    val::<PackedFloat64Array>(f.get(5)),
                    val::<PackedInt32Array>(f.get(6)),
                    val::<PackedByteArray>(f.get(7)),
                    val::<PackedFloat64Array>(f.get(8)),
                    val::<PackedInt32Array>(f.get(9)),
                    val::<PackedInt32Array>(f.get(10)),
                ),
                None => none,
            };
            grips.push(grip);
        }
        grips
    }

    /// `GSSimLegs.lower` for each figure in [param calls], ten entries a figure (the legs object, then
    /// `lower`'s arguments), in order. Returns each call's `holding` as 1 or 0 (0 for no legs object).
    #[func]
    fn lower(&self, calls: VarArray) -> PackedByteArray {
        let c: Vec<Variant> = calls.iter_shared().collect();
        let mut held = PackedByteArray::new();
        for f in c.chunks_exact(LOWER_STRIDE) {
            let holding = match obj::<GSSimLegs>(f.first()) {
                Some(mut legs) => legs.bind_mut().lower(
                    obj::<Skeleton3D>(f.get(1)),
                    plane(f.get(2)),
                    val::<Vector3>(f.get(3)),
                    obj::<GSSimTurnLimit>(f.get(4)),
                    obj::<GSSimTurnLimit>(f.get(5)),
                    val::<Transform3D>(f.get(6)),
                    val::<Transform3D>(f.get(7)),
                    val::<PackedFloat32Array>(f.get(8)),
                    val::<PackedFloat64Array>(f.get(9)),
                ),
                None => false,
            };
            held.push(u8::from(holding));
        }
        held
    }
}
