//! A mirror as bytes, so a map converter can bake it at convert time and the game read it back
//! instead of walking the scene. Little-endian: magic `GSCM`, version `u32`, piece count `u32`,
//! then per piece `solid u8`, `layer u32`, `shape u32`, and the verts, planes and edges each as a `u32` count
//! then `f64`s (3, 4 and 3 per item).

use crate::mirror::Mirror;
use crate::piece::{Piece, V3};

const MAGIC: &[u8; 4] = b"GSCM";
const VERSION: u32 = 2;

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
        out
    }

    /// A mirror read back from [`Mirror::to_bytes`]; `None` for a wrong magic, version or a
    /// truncated blob.
    pub fn from_bytes(bytes: &[u8]) -> Option<Mirror> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != MAGIC || r.u32()? != VERSION {
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
        (r.at == bytes.len()).then(|| Mirror::new(pieces))
    }
}
