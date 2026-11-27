//! `GSSimTraceMirror`: a map's static collision copied out of the scene into
//! [`gs_sim_trace::mirror::Mirror`], so server traces (bot sight and nav, drops, hulls) skip the
//! physics server. [method load_scene] walks the map's collision bodies once at map load; a baked
//! blob ([method to_bytes], [method load_bytes]) is the hook for doing that at convert time.
//! Moving bodies (`AnimatableBody3D`, or any body in the `trace_dynamic` group) are kept as
//! body-space pieces and re-posed by [method sync_dynamic]. Each engine shape is also kept as a
//! Godot shape ([`GShape`]), so a box sweep is Godot's `cast_motion` to the bit; rays match
//! Godot Physics within float noise. The game's parity suite checks both.

use godot::classes::{
    AnimatableBody3D, BoxShape3D, CollisionObject3D, ConcavePolygonShape3D, ConvexPolygonShape3D,
    PhysicsServer3D, Shape3D, StaticBody3D,
};
use godot::prelude::*;
use gs_sim_motor::mirror_world::MirrorWorld;
use gs_sim_motor::trace::SharedWorld;
use gs_sim_trace::gjk::{F3, GShape, Geom, Xf};
use gs_sim_trace::mirror::{Hit, Mirror};
use gs_sim_trace::piece::{Piece, V3, Xform};
use std::sync::Arc;

/// Bodies in this group are moving brushes even when they are plain `StaticBody3D`s.
const DYNAMIC_GROUP: &str = "trace_dynamic";

fn v3(v: Vector3) -> V3 {
    [f64::from(v.x), f64::from(v.y), f64::from(v.z)]
}

fn gv(v: V3) -> Vector3 {
    Vector3::new(v[0] as f32, v[1] as f32, v[2] as f32)
}

fn xform(t: Transform3D) -> Xform {
    let r = t.basis.rows;
    Xform {
        cols: [
            [f64::from(r[0].x), f64::from(r[1].x), f64::from(r[2].x)],
            [f64::from(r[0].y), f64::from(r[1].y), f64::from(r[2].y)],
            [f64::from(r[0].z), f64::from(r[1].z), f64::from(r[2].z)],
        ],
        origin: v3(t.origin),
    }
}

fn f3(v: Vector3) -> F3 {
    [v.x, v.y, v.z]
}

/// A Godot transform as the f32 [`Xf`] the server composes with.
fn gxf(t: Transform3D) -> Xf {
    let r = t.basis.rows;
    Xf {
        rows: [f3(r[0]), f3(r[1]), f3(r[2])],
        origin: f3(t.origin),
    }
}

/// A moving body: its pieces in body space, its shapes with their owner transforms (piece
/// ranges relative to the body's own pieces), and where to read its pose.
struct Mover {
    body: InstanceId,
    pieces: Vec<Piece>,
    shapes: Vec<(GShape, Xf)>,
}

/// The shape the physics server holds for `shape`, in shape space, as the server's GJK sees it:
/// a box's half extents, a hull's built vertices, a trimesh's faces.
fn shape_geom(shape: &Gd<Shape3D>) -> Option<Geom> {
    if let Ok(b) = shape.clone().try_cast::<BoxShape3D>() {
        return Some(Geom::Box(f3(b.get_size() * 0.5)));
    }
    if let Ok(c) = shape.clone().try_cast::<ConvexPolygonShape3D>() {
        let data = PhysicsServer3D::singleton().shape_get_data(shape.get_rid());
        let pts = data
            .try_to::<PackedVector3Array>()
            .unwrap_or_else(|_| c.get_points());
        return Geom::hull(pts.as_slice().iter().map(|p| f3(*p)).collect());
    }
    let c = shape.clone().try_cast::<ConcavePolygonShape3D>().ok()?;
    Geom::faces(c.get_faces().as_slice().iter().map(|p| f3(*p)).collect())
}

/// The pieces one shape gives under `x`; `None` for a shape type the mirror does not carry.
fn shape_pieces(shape: Gd<Shape3D>, x: &Xform, layer: u32) -> Option<Vec<Piece>> {
    let shape = match shape.try_cast::<BoxShape3D>() {
        Ok(b) => {
            return Some(
                Piece::cuboid(x, v3(b.get_size() * 0.5), layer)
                    .into_iter()
                    .collect(),
            );
        }
        Err(s) => s,
    };
    let shape = match shape.try_cast::<ConvexPolygonShape3D>() {
        Ok(c) => {
            let pts: Vec<V3> = c
                .get_points()
                .as_slice()
                .iter()
                .map(|p| x.point(v3(*p)))
                .collect();
            return Some(Piece::convex(&pts, layer).into_iter().collect());
        }
        Err(s) => s,
    };
    let c = shape.try_cast::<ConcavePolygonShape3D>().ok()?;
    Some(faces_pieces(
        c.get_faces().as_slice(),
        x,
        layer,
        c.is_backface_collision_enabled(),
    ))
}

/// Trimesh `faces` (three points a triangle) under `x`; `both` stages each face both ways round,
/// for a trimesh with `backface_collision`.
fn faces_pieces(faces: &[Vector3], x: &Xform, layer: u32, both: bool) -> Vec<Piece> {
    let mut out = Vec::new();
    for t in faces.as_chunks::<3>().0 {
        let p = |i: usize| x.point(v3(t[i]));
        out.extend(Piece::triangle(p(0), p(1), p(2), layer));
        if both {
            out.extend(Piece::triangle(p(0), p(2), p(1), layer));
        }
    }
    out
}

/// Builds and queries a collision mirror. Load it on the main thread; once built its queries are
/// safe from any thread (the static part is read-only, the dynamic list behind a lock).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimTraceMirror {
    mirror: Arc<Mirror>,
    staged: Vec<Piece>,
    /// The Godot shapes of the staged pieces, for [`Mirror::with_shapes`].
    staged_shapes: Vec<GShape>,
    movers: Vec<Mover>,
    /// Shapes the mirror does not carry (spheres, capsules, height maps), seen at load.
    skipped: i64,
    /// The id the next staged shape's pieces get.
    next_shape: u32,
    /// The motor's capsule world over `mirror`, built on first ask and shared by every body.
    motor_world: Option<(Arc<Mirror>, SharedWorld)>,
    /// A walk for [method load_baked]: static bodies come from the blob, only movers are read.
    movers_only: bool,
}

impl GSSimTraceMirror {
    /// The built mirror, for a native caller (a mind's sight) that asks it directly.
    pub fn mirror(&self) -> Arc<Mirror> {
        self.mirror.clone()
    }

    /// The motor's trace world over the built mirror ([`MirrorWorld`]), shared by every body
    /// given it; rebuilt only when the mirror was rebuilt or reloaded.
    pub fn motor_world(&mut self) -> SharedWorld {
        if let Some((m, w)) = &self.motor_world
            && Arc::ptr_eq(m, &self.mirror)
        {
            return w.clone();
        }
        let w: SharedWorld = Arc::new(MirrorWorld::new(self.mirror.clone()));
        self.motor_world = Some((self.mirror.clone(), w.clone()));
        w
    }

    /// `pieces` (one engine shape) given the next shape id.
    fn tag(&mut self, mut pieces: Vec<Piece>) -> Vec<Piece> {
        self.next_shape = self.next_shape.wrapping_add(1);
        for p in &mut pieces {
            p.shape = self.next_shape;
        }
        pieces
    }

    fn walk(&mut self, node: Gd<Node>) {
        if let Ok(body) = node.clone().try_cast::<CollisionObject3D>() {
            self.add_body(body);
        }
        for child in node.get_children().iter_shared() {
            self.walk(child);
        }
    }

    fn add_body(&mut self, body: Gd<CollisionObject3D>) {
        let moving =
            body.clone().try_cast::<AnimatableBody3D>().is_ok() || body.is_in_group(DYNAMIC_GROUP);
        if !moving && body.clone().try_cast::<StaticBody3D>().is_err() {
            return;
        }
        if self.movers_only && !moving {
            return;
        }
        let layer = body.get_collision_layer();
        let global = body.get_global_transform();
        let pose = if moving {
            Transform3D::IDENTITY
        } else {
            global
        };
        let mut pieces = Vec::new();
        let mut shapes = Vec::new();
        for owner in body.get_shape_owners().as_slice() {
            let owner = *owner as u32;
            if body.is_shape_owner_disabled(owner) {
                continue;
            }
            let local = body.shape_owner_get_transform(owner);
            let x = xform(pose * local);
            // The server composes body * shape in f32 at query time; so does `Xf::mul`.
            let owner_xf = gxf(local);
            for i in 0..body.shape_owner_get_shape_count(owner) {
                let Some(shape) = body.shape_owner_get_shape(owner, i) else {
                    continue;
                };
                let geom = shape_geom(&shape);
                let Some(p) = shape_pieces(shape, &x, layer) else {
                    self.skipped += 1;
                    continue;
                };
                if moving {
                    let p = self.tag(p);
                    let range = [pieces.len() as u32, (pieces.len() + p.len()) as u32];
                    pieces.extend(p);
                    if let Some(g) = geom {
                        let s = GShape::new(g, owner_xf, layer, self.next_shape, range);
                        shapes.push((s, owner_xf));
                    }
                } else {
                    self.stage(p, geom, gxf(global).mul(&owner_xf), layer);
                }
            }
        }
        if moving {
            self.movers.push(Mover {
                body: body.instance_id(),
                pieces,
                shapes,
            });
        }
    }

    /// Stages one shape's `pieces` and, when the server shape is one the mirror can cast against,
    /// its [`GShape`] at `x` naming their range.
    fn stage(&mut self, pieces: Vec<Piece>, geom: Option<Geom>, x: Xf, layer: u32) {
        let pieces = self.tag(pieces);
        let first = self.staged.len() as u32;
        self.staged.extend(pieces);
        if let Some(g) = geom {
            let range = [first, self.staged.len() as u32];
            let s = GShape::new(g, x, layer, self.next_shape, range);
            self.staged_shapes.push(s);
        }
    }

    fn hit_dict(from: V3, to: V3, hit: Option<Hit>) -> VarDictionary {
        let mut out = VarDictionary::new();
        if let Some(h) = hit {
            out.set(
                "position",
                &gv(Mirror::at(from, to, h.fraction)).to_variant(),
            );
            out.set("normal", &gv(h.normal).to_variant());
            out.set("fraction", &h.fraction.to_variant());
        }
        out
    }
}

#[godot_api]
impl GSSimTraceMirror {
    /// Replaces the mirror with the collision under [param root]: every enabled shape of every
    /// `StaticBody3D` (boxes, convex hulls, trimeshes) with its body's collision layer, plus the
    /// moving bodies for [method sync_dynamic]. Returns the static piece count.
    #[func]
    fn load_scene(&mut self, root: Gd<Node>) -> i64 {
        self.staged.clear();
        self.staged_shapes.clear();
        self.movers.clear();
        self.skipped = 0;
        self.walk(root);
        self.build();
        self.sync_dynamic();
        self.mirror.len() as i64
    }

    /// As [method load_scene], with the static part from a baked blob ([method to_bytes]) instead
    /// of the scene: only [param root]'s moving bodies are read. False (nothing changed) when
    /// [param bytes] is not a blob; the caller then loads the scene.
    #[func]
    fn load_baked(&mut self, root: Gd<Node>, bytes: PackedByteArray) -> bool {
        let Some(m) = Mirror::from_bytes(bytes.as_slice()) else {
            return false;
        };
        self.staged.clear();
        self.staged_shapes.clear();
        self.movers.clear();
        self.skipped = 0;
        self.movers_only = true;
        self.walk(root);
        self.movers_only = false;
        self.mirror = Arc::new(m);
        self.sync_dynamic();
        true
    }

    /// Stages a box of [param size] under [param transform], for [method build].
    #[func]
    fn add_box(&mut self, transform: Transform3D, size: Vector3, layer: i64) {
        let p = Piece::cuboid(&xform(transform), v3(size * 0.5), layer as u32);
        let geom = Some(Geom::Box(f3(size * 0.5)));
        self.stage(p.into_iter().collect(), geom, gxf(transform), layer as u32);
    }

    /// Stages the convex hull of [param points] under [param transform], for [method build].
    #[func]
    fn add_convex(&mut self, transform: Transform3D, points: PackedVector3Array, layer: i64) {
        let x = xform(transform);
        let pts: Vec<V3> = points.as_slice().iter().map(|p| x.point(v3(*p))).collect();
        let p = Piece::convex(&pts, layer as u32).into_iter().collect();
        let geom = Geom::hull(points.as_slice().iter().map(|p| f3(*p)).collect());
        self.stage(p, geom, gxf(transform), layer as u32);
    }

    /// Stages trimesh [param faces] (three points a triangle) under [param transform].
    #[func]
    fn add_faces(&mut self, transform: Transform3D, faces: PackedVector3Array, layer: i64) {
        let p = faces_pieces(faces.as_slice(), &xform(transform), layer as u32, false);
        let geom = Geom::faces(faces.as_slice().iter().map(|p| f3(*p)).collect());
        self.stage(p, geom, gxf(transform), layer as u32);
    }

    /// Builds the mirror over everything staged (the staging list empties).
    #[func]
    fn build(&mut self) {
        let pieces = std::mem::take(&mut self.staged);
        let shapes = std::mem::take(&mut self.staged_shapes);
        self.mirror = Arc::new(Mirror::with_shapes(pieces, shapes));
    }

    /// Re-poses the moving bodies from their current transforms; a freed body drops out. Call it
    /// on the main thread once a tick, before the tick's queries.
    #[func]
    fn sync_dynamic(&mut self) {
        let mut posed = Vec::new();
        let mut shapes = Vec::new();
        self.movers.retain(|m| {
            let Ok(body) = Gd::<CollisionObject3D>::try_from_instance_id(m.body) else {
                return false;
            };
            let global = body.get_global_transform();
            let x = xform(global);
            let base = posed.len() as u32;
            posed.extend(m.pieces.iter().map(|p| p.transformed(&x)));
            for (s, owner_xf) in &m.shapes {
                let mut s = s.clone();
                s.pieces = s.pieces.map(|k| base + k);
                s.pose(gxf(global).mul(owner_xf));
                shapes.push(s);
            }
            true
        });
        self.mirror.set_dynamic(posed);
        self.mirror.set_dynamic_shapes(shapes);
    }

    /// The static pieces and shapes as a bake blob (`GSCM` v3), for a map converter to store.
    #[func]
    fn to_bytes(&self) -> PackedByteArray {
        PackedByteArray::from(self.mirror.to_bytes().as_slice())
    }

    /// Replaces the static mirror with a baked blob; false (mirror unchanged) when it is not one.
    #[func]
    fn load_bytes(&mut self, bytes: PackedByteArray) -> bool {
        match Mirror::from_bytes(bytes.as_slice()) {
            Some(m) => {
                self.mirror = Arc::new(m);
                true
            }
            None => false,
        }
    }

    /// The ray from [param from] to [param to] on [param mask]: `position`, `normal` and
    /// `fraction` of the nearest hit, or an empty dictionary (like `intersect_ray`).
    #[func]
    fn ray(&self, from: Vector3, to: Vector3, mask: i64) -> VarDictionary {
        let (f, t) = (v3(from), v3(to));
        Self::hit_dict(f, t, self.mirror.ray(f, t, mask as u32))
    }

    /// Whether nothing on [param mask] lies between [param from] and [param to].
    #[func]
    fn clear(&self, from: Vector3, to: Vector3, mask: i64) -> bool {
        self.mirror.ray(v3(from), v3(to), mask as u32).is_none()
    }

    /// Many rays at once, parallel arrays in and out: `fraction` (1 for a miss), `position`
    /// (the end point for a miss) and `normal` (zero for a miss).
    #[func]
    fn rays(&self, froms: PackedVector3Array, tos: PackedVector3Array, mask: i64) -> VarDictionary {
        let n = froms.len().min(tos.len());
        let (fs, ts) = (froms.as_slice(), tos.as_slice());
        let mut fraction = vec![1.0f64; n];
        let mut position: Vec<Vector3> = ts[..n].to_vec();
        let mut normal = vec![Vector3::ZERO; n];
        for k in 0..n {
            let (f, t) = (v3(fs[k]), v3(ts[k]));
            if let Some(h) = self.mirror.ray(f, t, mask as u32) {
                fraction[k] = h.fraction;
                position[k] = gv(Mirror::at(f, t, h.fraction));
                normal[k] = gv(h.normal);
            }
        }
        let mut out = VarDictionary::new();
        out.set(
            "fraction",
            &PackedFloat64Array::from(fraction.as_slice()).to_variant(),
        );
        out.set(
            "position",
            &PackedVector3Array::from(position.as_slice()).to_variant(),
        );
        out.set(
            "normal",
            &PackedVector3Array::from(normal.as_slice()).to_variant(),
        );
        out
    }

    /// The axis-aligned box of [param half] extents at [param from] moved by [param motion]:
    /// `fraction` (1 for no contact), `normal`, `hit` and `start_solid`.
    #[func]
    fn cast_box(&self, from: Vector3, motion: Vector3, half: Vector3, mask: i64) -> VarDictionary {
        let hit = self
            .mirror
            .sweep(v3(from), v3(motion), v3(half), mask as u32);
        let mut out = VarDictionary::new();
        out.set("hit", &hit.is_some().to_variant());
        out.set("fraction", &hit.map_or(1.0, |h| h.fraction).to_variant());
        out.set(
            "normal",
            &gv(hit.map_or([0.0; 3], |h| h.normal)).to_variant(),
        );
        out.set(
            "start_solid",
            &hit.is_some_and(|h| h.start_solid).to_variant(),
        );
        out
    }

    /// Many box sweeps of one size: each one's contact fraction (1 for none).
    #[func]
    fn cast_boxes(
        &self,
        froms: PackedVector3Array,
        motions: PackedVector3Array,
        half: Vector3,
        mask: i64,
    ) -> PackedFloat64Array {
        let n = froms.len().min(motions.len());
        let (fs, ms) = (froms.as_slice(), motions.as_slice());
        let h = v3(half);
        let out: Vec<f64> = (0..n)
            .map(|k| {
                self.mirror
                    .sweep(v3(fs[k]), v3(ms[k]), h, mask as u32)
                    .map_or(1.0, |hit| hit.fraction)
            })
            .collect();
        PackedFloat64Array::from(out.as_slice())
    }

    /// The layers on [param mask] of every solid holding [param point], 0 for open space.
    #[func]
    fn point_contents(&self, point: Vector3, mask: i64) -> i64 {
        i64::from(self.mirror.point_contents(v3(point), mask as u32))
    }

    /// Static and dynamic piece counts and the skipped shape count.
    #[func]
    fn stats(&self) -> VarDictionary {
        let mut out = VarDictionary::new();
        out.set("pieces", &(self.mirror.len() as i64).to_variant());
        out.set("dynamic", &(self.mirror.dynamic_len() as i64).to_variant());
        out.set("movers", &(self.movers.len() as i64).to_variant());
        out.set("skipped", &self.skipped.to_variant());
        out
    }
}
