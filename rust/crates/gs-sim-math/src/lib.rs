//! Vector arithmetic rounded the way the engine rounds it.
//!
//! Engine vectors are `f32` while script floats are `f64`; every operation here is spelled in
//! the engine's order and precision so a native kernel and the script it replaces agree to the bit.

/// Three `f32` components, as the engine's `Vector3`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    /// A vector from its components.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Dot product, summed x, y, z as the engine does.
    #[inline]
    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// Cross product.
    #[inline]
    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// Length in `f32`, as `Vector3.length()`.
    #[inline]
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Squared length in `f32`, as `Vector3.length_squared()`.
    #[inline]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Unit vector, as `Vector3.normalized()`: zero for a zero vector, else each component divided by the length.
    #[inline]
    pub fn normalized(self) -> Self {
        let lsq = self.length_squared();
        if lsq == 0.0 {
            return Self::default();
        }
        let l = lsq.sqrt();
        Self::new(self.x / l, self.y / l, self.z / l)
    }

    /// `Vector3.is_normalized()`: the squared length equal to one within `UNIT_EPSILON` (0.001), in `f32`.
    #[inline]
    pub fn is_normalized(self) -> bool {
        let lsq = self.length_squared();
        lsq == 1.0 || (lsq - 1.0).abs() < 0.001
    }

    /// `Vector3 * float`: the script's `f64` scalar rounded to `f32` first, then each component multiplied.
    #[inline]
    pub fn scale(self, s: f64) -> Self {
        let s = s as f32;
        Self::new(self.x * s, self.y * s, self.z * s)
    }

    /// `Vector3 / float`, rounded the same way as [`Vec3::scale`]; a division per component, not a reciprocal.
    #[inline]
    pub fn div_scalar(self, s: f64) -> Self {
        let s = s as f32;
        Self::new(self.x / s, self.y / s, self.z / s)
    }

    /// `Vector3.distance_to()`: the length of `to - self`.
    #[inline]
    pub fn distance_to(self, to: Self) -> f32 {
        (to - self).length()
    }

    /// Unsigned angle in radians, as `Vector3.angle_to()`: atan2 of the cross length and the dot.
    #[inline]
    pub fn angle_to(self, o: Self) -> f32 {
        self.cross(o).length().atan2(self.dot(o))
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;

    #[inline]
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    #[inline]
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Neg for Vec3 {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

/// An engine `Transform3D`: the basis as three rows, then the origin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xform {
    pub rows: [Vec3; 3],
    pub origin: Vec3,
}

impl Xform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        rows: [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ],
        origin: Vec3::new(0.0, 0.0, 0.0),
    };

    /// `Transform3D.xform`: each row dotted with `v`, plus the origin's component.
    #[inline]
    pub fn xform(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.rows[0].dot(v) + self.origin.x,
            self.rows[1].dot(v) + self.origin.y,
            self.rows[2].dot(v) + self.origin.z,
        )
    }

    /// `Transform3D * Transform3D` as the engine computes it: the origin is `self.xform(o.origin)`,
    /// and each basis entry is `o`'s column dotted with `self`'s row (`Basis::tdotx` and friends),
    /// summed in row order.
    #[inline]
    pub fn mul(&self, o: &Self) -> Self {
        let [o0, o1, o2] = o.rows;
        let row = |r: Vec3| {
            Vec3::new(
                o0.x * r.x + o1.x * r.y + o2.x * r.z,
                o0.y * r.x + o1.y * r.y + o2.y * r.z,
                o0.z * r.x + o1.z * r.y + o2.z * r.z,
            )
        };
        Self {
            rows: [row(self.rows[0]), row(self.rows[1]), row(self.rows[2])],
            origin: self.xform(o.origin),
        }
    }

    /// `basis * v` (`Basis::xform`): each row dotted with `v`, no origin.
    #[inline]
    pub fn basis_xform(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.rows[0].dot(v),
            self.rows[1].dot(v),
            self.rows[2].dot(v),
        )
    }

    /// A rotation of `angle` radians about the unit `axis` (`Basis(axis, angle)`, the engine's
    /// `set_axis_angle` in `f32`), at the origin.
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let sq = Vec3::new(axis.x * axis.x, axis.y * axis.y, axis.z * axis.z);
        let cosine = angle.cos();
        let r00 = sq.x + cosine * (1.0 - sq.x);
        let r11 = sq.y + cosine * (1.0 - sq.y);
        let r22 = sq.z + cosine * (1.0 - sq.z);
        let sine = angle.sin();
        let t = 1.0 - cosine;
        let (xyzt, zyxs) = (axis.x * axis.y * t, axis.z * sine);
        let (r01, r10) = (xyzt - zyxs, xyzt + zyxs);
        let (xyzt, zyxs) = (axis.x * axis.z * t, axis.y * sine);
        let (r02, r20) = (xyzt + zyxs, xyzt - zyxs);
        let (xyzt, zyxs) = (axis.y * axis.z * t, axis.x * sine);
        let (r12, r21) = (xyzt - zyxs, xyzt + zyxs);
        Self {
            rows: [
                Vec3::new(r00, r01, r02),
                Vec3::new(r10, r11, r12),
                Vec3::new(r20, r21, r22),
            ],
            origin: Vec3::default(),
        }
    }

    /// `affine_inverse()`: the engine's `Basis::invert` (cofactors over the determinant, in its
    /// order), then the origin as `basis.xform(-origin)`. A singular basis stays as it was, as the
    /// engine's (which logs an error and returns before writing it).
    pub fn affine_inverse(&self) -> Self {
        let m = |r: usize, c: usize| {
            let row = self.rows[r];
            match c {
                0 => row.x,
                1 => row.y,
                _ => row.z,
            }
        };
        let cofac = |r1: usize, c1: usize, r2: usize, c2: usize| {
            m(r1, c1) * m(r2, c2) - m(r1, c2) * m(r2, c1)
        };
        let co = [cofac(1, 1, 2, 2), cofac(1, 2, 2, 0), cofac(1, 0, 2, 1)];
        let det = m(0, 0) * co[0] + m(0, 1) * co[1] + m(0, 2) * co[2];
        let basis = if det == 0.0 {
            self.rows
        } else {
            let s = 1.0f32 / det;
            [
                Vec3::new(co[0] * s, cofac(0, 2, 2, 1) * s, cofac(0, 1, 1, 2) * s),
                Vec3::new(co[1] * s, cofac(0, 0, 2, 2) * s, cofac(0, 2, 1, 0) * s),
                Vec3::new(co[2] * s, cofac(0, 1, 2, 0) * s, cofac(0, 0, 1, 1) * s),
            ]
        };
        let inverse = Self {
            rows: basis,
            origin: Vec3::default(),
        };
        Self {
            rows: basis,
            origin: inverse.basis_xform(-self.origin),
        }
    }
}

/// The script's `maxf`: `a < b ? b : a`, so a NaN in `a` survives as it does there.
#[inline]
pub fn maxf(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// The script's `minf`: `a < b ? a : b`.
#[inline]
pub fn minf(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

/// The script's `clampf`: `v < lo ? lo : (v > hi ? hi : v)`.
#[inline]
pub fn clampf(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Radians to degrees in `f64`, as the script's `rad_to_deg`.
#[inline]
pub fn rad_to_deg(r: f64) -> f64 {
    r * (180.0 / std::f64::consts::PI)
}

/// A `Quaternion` as the engine stores it: x, y, z, w.
pub type Quat = [f32; 4];

impl Vec3 {
    /// `Vector3.normalize()` in place, as the engine: zero for a non-finite or zero vector, else
    /// each component divided by the length (the square root of the `f32` squared length).
    #[inline]
    fn engine_normalized(self) -> Self {
        if !(self.x.is_finite() && self.y.is_finite() && self.z.is_finite()) {
            return Self::default();
        }
        self.normalized()
    }

    /// `Vector3 * real_t` with an `f32` scalar.
    #[inline]
    fn times(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

/// `Quaternion * Quaternion`, the engine's component order.
pub fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [x, y, z, w] = a;
    [
        w * b[0] + x * b[3] + y * b[2] - z * b[1],
        w * b[1] + y * b[3] + z * b[0] - x * b[2],
        w * b[2] + z * b[3] + x * b[1] - y * b[0],
        w * b[3] - x * b[0] - y * b[1] - z * b[2],
    ]
}

/// `Quaternion.normalized()`: each component times `1 / length`, as the engine's `operator/=`.
pub fn quat_normalized(q: Quat) -> Quat {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let inv = 1.0 / len;
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

/// `Quaternion.slerp` of two normalized quaternions in the engine's order and rounding (the
/// platform's own `acos` and `sin`, as the engine calls them).
pub fn quat_slerp(a: Quat, b: Quat, weight: f32) -> Quat {
    let mut cosom = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let to = if cosom < 0.0 {
        cosom = -cosom;
        [-b[0], -b[1], -b[2], -b[3]]
    } else {
        b
    };
    let (scale0, scale1) = if (1.0 - cosom) > 0.00001 {
        let omega = cosom.acos();
        let sinom = omega.sin();
        (
            // The engine's `1.0 - p_weight` is a double literal: this share is worked out in
            // double (double sin, double divide) and rounded once; the other stays in float.
            (((1.0 - weight as f64) * omega as f64).sin() / sinom as f64) as f32,
            (weight * omega).sin() / sinom,
        )
    } else {
        (1.0 - weight, weight)
    };
    [
        scale0 * a[0] + scale1 * to[0],
        scale0 * a[1] + scale1 * to[1],
        scale0 * a[2] + scale1 * to[2],
        scale0 * a[3] + scale1 * to[3],
    ]
}

/// `Vector3.lerp` per component: `from + weight * (to - from)`.
#[inline]
pub fn lerp_f32(from: f32, to: f32, weight: f32) -> f32 {
    from + weight * (to - from)
}

/// A basis's column `i`, as `Basis::get_column`.
#[inline]
fn column(rows: &[Vec3; 3], i: usize) -> Vec3 {
    let c = |r: Vec3| match i {
        0 => r.x,
        1 => r.y,
        _ => r.z,
    };
    Vec3::new(c(rows[0]), c(rows[1]), c(rows[2]))
}

/// `Basis.orthonormalized()`: Gram-Schmidt over the columns in the engine's order and rounding.
pub fn orthonormalized(rows: &[Vec3; 3]) -> [Vec3; 3] {
    let x = column(rows, 0).engine_normalized();
    let y = column(rows, 1);
    let y = (y - x.times(x.dot(y))).engine_normalized();
    let z = column(rows, 2);
    let z = (z - x.times(x.dot(z)) - y.times(y.dot(z))).engine_normalized();
    [
        Vec3::new(x.x, y.x, z.x),
        Vec3::new(x.y, y.y, z.y),
        Vec3::new(x.z, y.z, z.z),
    ]
}

/// `Basis.determinant()`, expanded along the first column as the engine does.
pub fn determinant(m: &[Vec3; 3]) -> f32 {
    m[0].x * (m[1].y * m[2].z - m[2].y * m[1].z) - m[1].x * (m[0].y * m[2].z - m[2].y * m[0].z)
        + m[2].x * (m[0].y * m[1].z - m[1].y * m[0].z)
}

/// `Basis.get_quaternion()`: the trace branch, else the largest diagonal entry's, in `f32`.
pub fn quaternion(m: &[Vec3; 3]) -> Quat {
    let r = |i: usize, j: usize| {
        let row = m[i];
        match j {
            0 => row.x,
            1 => row.y,
            _ => row.z,
        }
    };
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let mut q = [0.0f32; 4];
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt();
        q[3] = s * 0.5;
        let s = 0.5 / s;
        q[0] = (r(2, 1) - r(1, 2)) * s;
        q[1] = (r(0, 2) - r(2, 0)) * s;
        q[2] = (r(1, 0) - r(0, 1)) * s;
    } else {
        let i = if r(0, 0) < r(1, 1) {
            if r(1, 1) < r(2, 2) { 2 } else { 1 }
        } else if r(0, 0) < r(2, 2) {
            2
        } else {
            0
        };
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        let s = (r(i, i) - r(j, j) - r(k, k) + 1.0).sqrt();
        q[i] = s * 0.5;
        let s = 0.5 / s;
        q[3] = (r(k, j) - r(j, k)) * s;
        q[j] = (r(j, i) + r(i, j)) * s;
        q[k] = (r(k, i) + r(i, k)) * s;
    }
    q
}

/// `Basis.get_rotation_quaternion()`: orthonormalized, negated when its determinant is negative,
/// then [`quaternion`].
pub fn rotation_quaternion(rows: &[Vec3; 3]) -> Quat {
    let mut m = orthonormalized(rows);
    if determinant(&m) < 0.0 {
        m = [-m[0], -m[1], -m[2]];
    }
    quaternion(&m)
}

#[cfg(test)]
mod tests {
    use super::{quat_normalized, quat_slerp};

    #[test]
    fn slerp_ends_and_midpoint() {
        let a = [0.0, 0.0, 0.0, 1.0];
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let b = quat_normalized([0.0, h, 0.0, h]);
        assert_eq!(quat_slerp(a, b, 0.0), a);
        let end = quat_slerp(a, b, 1.0);
        for k in 0..4 {
            assert!((end[k] - b[k]).abs() < 1e-6);
        }
        // Half way between identity and a quarter turn is an eighth turn about y.
        let mid = quat_slerp(a, b, 0.5);
        let (s, c) = (
            std::f32::consts::FRAC_PI_8.sin(),
            std::f32::consts::FRAC_PI_8.cos(),
        );
        assert!((mid[1] - s).abs() < 1e-6 && (mid[3] - c).abs() < 1e-6);
        // The short way round: the negated end is the same rotation.
        let neg = quat_slerp(a, [-b[0], -b[1], -b[2], -b[3]], 0.5);
        assert!((neg[1] - mid[1]).abs() < 1e-6 && (neg[3] - mid[3]).abs() < 1e-6);
        // Nearly equal quaternions lerp.
        assert_eq!(quat_slerp(a, a, 0.3), a);
    }

    use super::*;

    #[test]
    fn axis_angle_quarter_turn_about_up_sends_x_to_minus_z() {
        let b = Xform::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);
        let v = b.basis_xform(Vec3::new(1.0, 0.0, 0.0));
        assert!(
            v.x.abs() < 1e-6 && v.y == 0.0 && (v.z + 1.0).abs() < 1e-6,
            "{v:?}"
        );
        // The engine's rounding: the diagonal is axis_sq + cos * (1 - axis_sq), in f32.
        let a = 0.3f32;
        let r = Xform::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), a);
        assert_eq!(r.rows[0].x, a.cos());
        assert_eq!(r.rows[2].z, 1.0);
        assert_eq!(r.rows[0].y, -a.sin());
    }

    #[test]
    fn affine_inverse_undoes_the_transform() {
        let f = Xform {
            rows: Xform::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.7).rows,
            origin: Vec3::new(3.0, -1.5, 8.25),
        };
        let back = f.affine_inverse().xform(f.xform(Vec3::new(0.4, 2.0, -1.0)));
        assert!(
            (back.x - 0.4).abs() < 1e-5
                && (back.y - 2.0).abs() < 1e-5
                && (back.z + 1.0).abs() < 1e-5
        );
        assert_eq!(Xform::IDENTITY.affine_inverse(), Xform::IDENTITY);
        let flat = Xform {
            rows: [Vec3::default(); 3],
            origin: Vec3::new(1.0, 2.0, 3.0),
        };
        assert_eq!(flat.affine_inverse().rows, [Vec3::default(); 3]);
    }

    #[test]
    fn angle_of_perpendicular_axes_is_ninety_degrees() {
        let a = Vec3::new(1.0, 0.0, 0.0).angle_to(Vec3::new(0.0, 0.0, -1.0));
        assert!((rad_to_deg(a as f64) - 90.0).abs() < 1e-5);
    }

    #[test]
    fn angle_to_self_is_zero_and_opposite_is_pi() {
        let v = Vec3::new(0.3, -2.0, 5.0);
        assert_eq!(v.angle_to(v), 0.0);
        let back = Vec3::new(-0.3, 2.0, -5.0);
        assert!((v.angle_to(back) - std::f32::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn normalized_matches_engine_rounding() {
        let v = Vec3::new(0.3, -2.0, 5.0);
        let l = (0.3f32 * 0.3 + 2.0 * 2.0 + 5.0 * 5.0).sqrt();
        assert_eq!(v.normalized(), Vec3::new(0.3 / l, -2.0 / l, 5.0 / l));
        assert_eq!(Vec3::default().normalized(), Vec3::default());
        assert!(v.normalized().is_normalized());
        assert!(!Vec3::new(1.0, 0.1, 0.0).is_normalized());
    }

    #[test]
    fn scale_rounds_the_scalar_to_f32_first() {
        let v = Vec3::new(1.0, 3.0, -7.0);
        let s = 0.1f64;
        assert_eq!(v.scale(s), Vec3::new(0.1f32, 3.0 * 0.1f32, -7.0 * 0.1f32));
    }

    #[test]
    fn script_min_max_clamp_keep_their_nan_order() {
        assert!(maxf(f64::NAN, 0.0).is_nan());
        assert_eq!(maxf(0.0, f64::NAN), 0.0);
        assert_eq!(clampf(2.0, 0.0, 1.0), 1.0);
        assert_eq!(clampf(-2.0, 0.0, 1.0), 0.0);
        assert_eq!(minf(0.25, 0.5), 0.25);
    }

    #[test]
    fn xform_mul_follows_the_engine_order() {
        let a = Xform {
            rows: [
                Vec3::new(0.1, 0.7, -0.3),
                Vec3::new(-0.6, 0.2, 0.9),
                Vec3::new(0.35, -0.45, 0.15),
            ],
            origin: Vec3::new(1.5, -2.25, 0.125),
        };
        let b = Xform {
            rows: [
                Vec3::new(0.3, -0.1, 0.8),
                Vec3::new(0.05, 0.95, -0.2),
                Vec3::new(-0.7, 0.4, 0.33),
            ],
            origin: Vec3::new(-0.4, 0.6, 3.0),
        };
        let m = a.mul(&b);
        // Basis::tdotz(a.rows[1]): b's third column dotted with a's second row, in row order.
        assert_eq!(m.rows[1].z, 0.8f32 * -0.6 + -0.2f32 * 0.2 + 0.33f32 * 0.9);
        assert_eq!(m.origin.x, 0.1f32 * -0.4 + 0.7 * 0.6 + -0.3 * 3.0 + 1.5);
        assert_eq!(Xform::IDENTITY.mul(&b), b);
        assert_eq!(a.mul(&Xform::IDENTITY), a);
    }

    #[test]
    fn length_is_computed_in_f32() {
        let v = Vec3::new(3.0, 4.0, 12.0);
        assert_eq!(v.length(), 13.0);
        assert_eq!(
            Vec3::new(0.1, 0.2, 0.3).length(),
            (0.1f32 * 0.1 + 0.2 * 0.2 + 0.3 * 0.3).sqrt()
        );
    }

    #[test]
    fn rotation_quaternion_of_a_quarter_turn_and_a_mirror() {
        // A quarter turn about y: x goes to -z.
        let rows = [
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
        ];
        let q = rotation_quaternion(&rows);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((q[1] - h).abs() < 1e-6 && (q[3] - h).abs() < 1e-6, "{q:?}");
        // A mirrored, scaled identity still reads as no rotation.
        let m = [
            Vec3::new(-2.0, 0.0, 0.0),
            Vec3::new(0.0, -2.0, 0.0),
            Vec3::new(0.0, 0.0, -2.0),
        ];
        assert_eq!(rotation_quaternion(&m), [0.0, 0.0, 0.0, 1.0]);
        // A non-finite column normalizes to zero, as the engine's.
        let n = [
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        assert_eq!(orthonormalized(&n)[0].x, 0.0);
    }
}
