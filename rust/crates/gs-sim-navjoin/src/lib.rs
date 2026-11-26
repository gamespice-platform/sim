//! A navigation mesh's edge topology, as GameSpice Bot's `GSBotNavPrep` (and `GSBotNavGeometry.islands`) work it out
//! in script: a vertex is keyed by its position rounded to the centimetre, an edge by its two vertex keys either way
//! round, and polygons sharing an edge are one island.
//!
//! [`topology`] answers, for polygon `i` with corners `polys[offsets[i]..offsets[i + 1]]`:
//! - `roots[i]`: the lowest polygon index of its island (the script's union-find root, which always keeps the lower);
//! - `uses[offsets[i] + k]`: how many polygon sides lie on the edge from corner `k` to corner `k + 1`
//!   (1 is an open border, the rim the link search walks).

use std::collections::HashMap;

/// The centimetre key of a vertex: Godot's `roundi(v * 100.0)` per axis, the float widened to a double first.
pub fn vertex_key(v: [f32; 3]) -> [i32; 3] {
    [round_key(v[0]), round_key(v[1]), round_key(v[2])]
}

fn round_key(x: f32) -> i32 {
    // Godot's roundi is round half away from zero on a double, as f64::round.
    (f64::from(x) * 100.0).round() as i32
}

fn edge(a: [i32; 3], b: [i32; 3]) -> ([i32; 3], [i32; 3]) {
    if a <= b { (a, b) } else { (b, a) }
}

fn find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        let up = parent[parent[i as usize] as usize];
        parent[i as usize] = up;
        i = up;
    }
    i
}

/// Why [`topology`] refused its input.
#[derive(Debug, PartialEq, Eq)]
pub enum Bad {
    /// `offsets` is empty, does not start at 0, goes down, or does not end at `polys.len()`.
    Offsets,
    /// A corner names a vertex outside `vertices`.
    Corner,
}

/// Island roots per polygon and polygon sides per edge slot; see the crate docs.
pub fn topology(
    vertices: &[[f32; 3]],
    polys: &[i32],
    offsets: &[i32],
) -> Result<(Vec<i32>, Vec<i32>), Bad> {
    if offsets.is_empty()
        || offsets[0] != 0
        || offsets[offsets.len() - 1] as usize != polys.len()
        || offsets.windows(2).any(|w| w[1] < w[0])
    {
        return Err(Bad::Offsets);
    }
    if polys.iter().any(|&c| c < 0 || c as usize >= vertices.len()) {
        return Err(Bad::Corner);
    }
    let count = offsets.len() - 1;
    let keys: Vec<[i32; 3]> = vertices.iter().map(|&v| vertex_key(v)).collect();
    let mut parent: Vec<u32> = (0..count as u32).collect();
    // edge -> (first polygon on it, sides on it)
    let mut edges: HashMap<([i32; 3], [i32; 3]), (u32, i32)> = HashMap::with_capacity(polys.len());
    for i in 0..count {
        let corners = &polys[offsets[i] as usize..offsets[i + 1] as usize];
        let n = corners.len();
        for k in 0..n {
            let e = edge(
                keys[corners[k] as usize],
                keys[corners[(k + 1) % n] as usize],
            );
            match edges.get_mut(&e) {
                Some(entry) => {
                    entry.1 += 1;
                    let owner = entry.0;
                    let (ra, rb) = (find(&mut parent, i as u32), find(&mut parent, owner));
                    if ra != rb {
                        parent[ra.max(rb) as usize] = ra.min(rb);
                    }
                }
                None => {
                    edges.insert(e, (i as u32, 1));
                }
            }
        }
    }
    let mut uses = vec![0i32; polys.len()];
    for i in 0..count {
        let start = offsets[i] as usize;
        let corners = &polys[start..offsets[i + 1] as usize];
        let n = corners.len();
        for k in 0..n {
            let e = edge(
                keys[corners[k] as usize],
                keys[corners[(k + 1) % n] as usize],
            );
            uses[start + k] = edges[&e].1;
        }
    }
    let roots = (0..count as u32)
        .map(|i| find(&mut parent, i) as i32)
        .collect();
    Ok((roots, uses))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_half_away_from_zero() {
        // exact halves round away from zero; 0.005 is not one (as f32 it scales to 0.4999.., key 0, as in script)
        assert_eq!(vertex_key([0.125, -0.125, 1.234]), [13, -13, 123]);
        assert_eq!(vertex_key([0.005, 0.0, 0.0]), [0, 0, 0]);
        assert_eq!(vertex_key([-2.5, 2.5, 0.0]), [-250, 250, 0]);
    }

    #[test]
    fn two_quads_sharing_an_edge_and_a_loose_triangle() {
        // quad 0: (0,0)-(1,0)-(1,1)-(0,1); quad 1 shares (1,0)-(1,1); triangle 2 is apart
        let v = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [2.0, 0.0, 0.0],
            [2.0, 0.0, 1.0],
            [5.0, 0.0, 5.0],
            [6.0, 0.0, 5.0],
            [5.0, 0.0, 6.0],
            // a duplicate of vertex 2 a millimetre off: the same key
            [1.001, 0.0, 1.0],
        ];
        let polys = [6, 7, 8, 0, 1, 2, 3, 1, 4, 5, 9];
        let offsets = [0, 3, 7, 11];
        let (roots, uses) = topology(&v, &polys, &offsets).unwrap();
        assert_eq!(roots, vec![0, 1, 1]);
        assert_eq!(uses, vec![1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 2]);
    }

    #[test]
    fn union_keeps_the_lowest_root_across_chains() {
        // a strip of four triangles joined 3-2, 2-1, 1-0 out of order
        let v: Vec<[f32; 3]> = (0..6).map(|i| [i as f32, 0.0, (i % 2) as f32]).collect();
        let polys = [3, 4, 5, 0, 1, 2, 2, 3, 4, 1, 2, 3];
        let offsets = [0, 3, 6, 9, 12];
        let (roots, _) = topology(&v, &polys, &offsets).unwrap();
        assert_eq!(roots, vec![0, 0, 0, 0]);
    }

    #[test]
    fn refuses_bad_input() {
        let v = [[0.0; 3]; 3];
        assert_eq!(topology(&v, &[0, 1, 2], &[0, 2]), Err(Bad::Offsets));
        assert_eq!(topology(&v, &[0, 1, 3], &[0, 3]), Err(Bad::Corner));
        assert_eq!(topology(&v, &[], &[]), Err(Bad::Offsets));
        assert_eq!(topology(&v, &[], &[0]), Ok((vec![], vec![])));
    }
}
