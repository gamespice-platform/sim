//! `DotSimKillViewTrack`: one entity's ring of kill-cam view samples, each row built natively.
//!
//! The twin of `DotKillViewRing._Track` and the row its `record` builds (dot-replay): time apart,
//! a row is `[0, origin xyz, rotation xyzw, fov, cursor xy, aux 0..AUX]` in `f32`, the rotation
//! `basis.orthonormalized().get_rotation_quaternion()` in the engine's order and rounding
//! (`dot_sim_math::rotation_quaternion`), aux zero-padded or cut to `AUX`, the oldest row
//! overwritten when full. So every row is the script's to the bit; only the script's array
//! building and per-float copies go.

use dot_sim_math::{Vec3, orthonormalized, rotation_quaternion};
use godot::prelude::*;

/// Floats per row, as `DotKillViewRing.STRIDE`.
const STRIDE: usize = 23;
/// Game-defined floats per row, as `DotKillViewRing.AUX`.
const AUX: usize = 12;
/// Floats before the aux ones.
const HEAD: usize = STRIDE - AUX;

/// One entity's samples; [method setup] sizes it before the first [method record].
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct DotSimKillViewTrack {
    ring: Ring,
}

#[godot_api]
impl DotSimKillViewTrack {
    /// Holds the newest [param cap] samples (at least one); clears any held.
    #[func]
    fn setup(&mut self, cap: i32) {
        self.ring = Ring::new(usize::try_from(cap).unwrap_or(0).max(1));
    }

    /// Stores one sample as the newest, overwriting the oldest when full.
    #[func]
    fn record(
        &mut self,
        time: f64,
        xform: Transform3D,
        fov: f64,
        cursor: Vector2,
        aux: PackedFloat32Array,
    ) {
        let b = xform.basis.rows;
        let v = |v: Vector3| Vec3::new(v.x, v.y, v.z);
        let o = xform.origin;
        let head = row_head(
            [v(b[0]), v(b[1]), v(b[2])],
            [o.x, o.y, o.z],
            fov,
            [cursor.x, cursor.y],
        );
        self.ring.push(time, &head, aux.as_slice());
    }

    /// How many samples are held.
    #[func]
    fn samples(&self) -> i32 {
        self.ring.count as i32
    }

    /// The newest sample's time; only meaningful with samples held.
    #[func]
    fn newest(&self) -> f64 {
        self.ring.time_at(self.ring.count.saturating_sub(1))
    }

    /// Sample [param i]'s time, oldest first.
    #[func]
    fn time_at(&self, i: i32) -> f64 {
        self.ring.time_at(usize::try_from(i).unwrap_or(0))
    }

    /// Sample [param i]'s row of `STRIDE` floats, oldest first.
    #[func]
    fn row_at(&self, i: i32) -> PackedFloat32Array {
        PackedFloat32Array::from(self.ring.row_at(usize::try_from(i).unwrap_or(0)))
    }
}

/// A row's first `HEAD` floats: the time slot (0), origin, rotation, fov rounded to `f32`, cursor.
fn row_head(basis: [Vec3; 3], origin: [f32; 3], fov: f64, cursor: [f32; 2]) -> [f32; HEAD] {
    let q = rotation_quaternion(&orthonormalized(&basis));
    [
        0.0, origin[0], origin[1], origin[2], q[0], q[1], q[2], q[3], fov as f32, cursor[0],
        cursor[1],
    ]
}

/// Fixed-size ring of rows, oldest overwritten; the script `_Track`'s slots and order.
#[derive(Default)]
struct Ring {
    cap: usize,
    count: usize,
    start: usize,
    times: Vec<f64>,
    data: Vec<f32>,
}

impl Ring {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            count: 0,
            start: 0,
            times: vec![0.0; cap],
            data: vec![0.0; cap * STRIDE],
        }
    }

    fn push(&mut self, time: f64, head: &[f32; HEAD], aux: &[f32]) {
        if self.cap == 0 {
            *self = Self::new(1);
        }
        let slot = if self.count == self.cap {
            let s = self.start;
            self.start = (self.start + 1) % self.cap;
            s
        } else {
            self.count += 1;
            (self.start + self.count - 1) % self.cap
        };
        self.times[slot] = time;
        let row = &mut self.data[slot * STRIDE..(slot + 1) * STRIDE];
        row[..HEAD].copy_from_slice(head);
        for (k, out) in row[HEAD..].iter_mut().enumerate() {
            *out = aux.get(k).copied().unwrap_or(0.0);
        }
    }

    fn slot(&self, i: usize) -> usize {
        (self.start + i) % self.cap.max(1)
    }

    fn time_at(&self, i: usize) -> f64 {
        self.times.get(self.slot(i)).copied().unwrap_or(0.0)
    }

    fn row_at(&self, i: usize) -> &[f32] {
        let s = self.slot(i) * STRIDE;
        self.data.get(s..s + STRIDE).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_newest_rows_in_order_and_pads_aux() {
        let mut r = Ring::new(3);
        let head = |t: f32| {
            let mut h = [0.0; HEAD];
            h[1] = t;
            h
        };
        for t in 0..5 {
            r.push(f64::from(t), &head(t as f32), &[t as f32; 14]);
        }
        assert_eq!(r.count, 3);
        assert_eq!((r.time_at(0), r.time_at(2)), (2.0, 4.0));
        assert_eq!(r.row_at(0)[1], 2.0);
        assert_eq!(r.row_at(2)[STRIDE - 1], 4.0);
        let mut p = Ring::new(2);
        p.push(1.0, &head(1.0), &[7.0]);
        assert_eq!(&p.row_at(0)[HEAD..HEAD + 2], &[7.0, 0.0]);
        assert_eq!(p.row_at(0).len(), STRIDE);
    }

    #[test]
    fn the_head_rounds_fov_once() {
        let id = [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let h = row_head(id, [1.0, 2.0, 3.0], 0.1, [0.5, 0.25]);
        assert_eq!(
            h,
            [0.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 0.1f32, 0.5, 0.25]
        );
    }
}
