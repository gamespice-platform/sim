//! A mirror as bytes, so a map converter can bake it at convert time and the game read it back
//! instead of walking the scene. Little-endian: magic `GSCM`, version `u32`, piece count `u32`,
//! then per piece `solid u8`, `layer u32`, `shape u32`, and the verts, planes and edges each as a `u32` count
//! then `f64`s (3, 4 and 3 per item). Version 3 adds the Godot shapes ([`GShape`]): a `u32`
//! count, then per shape a geometry tag `u8` (0 box: three `f32` half extents; 1 hull, 2
//! trimesh: a `u32` vertex count then three `f32`s each), the transform as twelve `f32`s (basis
//! rows, origin), `layer u32`, `shape u32` and its piece range as two `u32`s. Everything else
//! (hull extremes, face hierarchy, bounds) is rebuilt from these, the same way every time, so a
//! blob read back writes the same bytes. Version 2 blobs still load (no shapes).

use crate::gjk::{F3, GShape, Geom, Xf};
use crate::mirror::Mirror;
use crate::piece::{Piece, V3};

const MAGIC: &[u8; 4] = b"GSCM";
const VERSION: u32 = 3;
const TAG_BOX: u8 = 0;
const TAG_HULL: u8 = 1;
const TAG_FACES: u8 = 2;

fn put_f32s(out: &mut Vec<u8>, vs: &[f32]) {
    for v in vs {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn put_points(out: &mut Vec<u8>, ps: &[F3]) {
    put_u32(out, ps.len() as u32);
    put_f32s(out, ps.as_flattened());
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f64s(out: &mut Vec<u8>, vs: &[f64]) {
    for v in vs {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f32s<const N: usize>(&mut self) -> Option<[f32; N]> {
        let mut out = [0.0; N];
        for c in out.iter_mut() {
            *c = self.f32()?;
        }
        Some(out)
    }

    fn points(&mut self) -> Option<Vec<F3>> {
        let n = self.u32()? as usize;
        if n > self.bytes.len() / 12 {
            return None;
        }
        (0..n).map(|_| self.f32s::<3>()).collect()
    }

    fn shape(&mut self) -> Option<GShape> {
        let geom = match self.take(1)?[0] {
            TAG_BOX => Geom::Box(self.f32s::<3>()?),
            TAG_HULL => Geom::hull(self.points()?)?,
            TAG_FACES => Geom::faces(self.points()?)?,
            _ => return None,
        };
        let xf = Xf::from_floats(self.f32s::<12>()?);
        let (layer, shape) = (self.u32()?, self.u32()?);
        let pieces = [self.u32()?, self.u32()?];
        Some(GShape::new(geom, xf, layer, shape, pieces))
    }

    fn list<const N: usize>(&mut self) -> Option<Vec<[f64; N]>> {
        let n = self.u32()? as usize;
        if n > self.bytes.len() / (8 * N) {
            return None;
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let mut item = [0.0; N];
            for c in item.iter_mut() {
                *c = self.f64()?;
            }
            out.push(item);
        }
        Some(out)
    }
}

impl Mirror {
    /// The static pieces as a bake blob (dynamic entries are scene state and stay out).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u32(&mut out, VERSION);
        put_u32(&mut out, self.len() as u32);
        for p in self.pieces() {
            out.push(u8::from(p.solid));
            put_u32(&mut out, p.layer);
            put_u32(&mut out, p.shape);
            put_u32(&mut out, p.verts.len() as u32);
            put_f64s(&mut out, p.verts.as_flattened());
            put_u32(&mut out, p.planes.len() as u32);
            put_f64s(&mut out, p.planes.as_flattened());
            put_u32(&mut out, p.edges.len() as u32);
            put_f64s(&mut out, p.edges.as_flattened());
        }
        put_u32(&mut out, self.shapes().len() as u32);
        for s in self.shapes() {
            match &s.geom {
                Geom::Box(h) => {
                    out.push(TAG_BOX);
                    put_f32s(&mut out, h);
                }
                Geom::Hull { verts, .. } => {
                    out.push(TAG_HULL);
                    put_points(&mut out, verts);
                }
                Geom::Faces(f) => {
                    out.push(TAG_FACES);
                    put_points(&mut out, &f.verts);
                }
            }
            put_f32s(&mut out, &s.xf.floats());
            put_u32(&mut out, s.layer);
            put_u32(&mut out, s.shape);
            put_u32(&mut out, s.pieces[0]);
            put_u32(&mut out, s.pieces[1]);
        }
        out
    }

    /// A mirror read back from [`Mirror::to_bytes`]; `None` for a wrong magic, version or a
    /// truncated blob.
    pub fn from_bytes(bytes: &[u8]) -> Option<Mirror> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != MAGIC {
            return None;
        }
        let version = r.u32()?;
        if version != 2 && version != VERSION {
            return None;
        }
        let n = r.u32()? as usize;
        let mut pieces = Vec::with_capacity(n.min(bytes.len()));
        for _ in 0..n {
            let solid = r.take(1)?[0] != 0;
            let layer = r.u32()?;
            let shape = r.u32()?;
            let verts: Vec<V3> = r.list::<3>()?;
            let planes = r.list::<4>()?;
            let edges: Vec<V3> = r.list::<3>()?;
            if verts.is_empty() || planes.is_empty() {
                return None;
            }
            let mut piece = Piece::from_parts(verts, planes, edges, solid, layer);
            piece.shape = shape;
            pieces.push(piece);
        }
        let mut shapes = Vec::new();
        if version >= 3 {
            let n = r.u32()? as usize;
            if n > bytes.len() {
                return None;
            }
            for _ in 0..n {
                shapes.push(r.shape()?);
            }
        }
        (r.at == bytes.len()).then(|| Mirror::with_shapes(pieces, shapes))
    }
}
