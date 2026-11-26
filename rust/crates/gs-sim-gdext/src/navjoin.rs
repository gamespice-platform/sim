//! `GSSimNavJoin`: a navigation mesh's edge topology for GameSpice Bot's link join (`GSBotNavPrep`), the script's
//! vertex-key, edge-count and island union-find passes in one call. The script stays the fallback (no library, the
//! web build, or `GSBotLinks.native` off) and GameSpice Bot's selftest holds the two to the same answer.

use godot::prelude::*;

#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimNavJoin {}

#[godot_api]
impl GSSimNavJoin {
    /// For the polygons `polys[offsets[i]..offsets[i + 1]]` over [param vertices]: `{"roots": PackedInt32Array}` (per
    /// polygon, the lowest polygon index of its island) and `{"uses": PackedInt32Array}` (per corner slot, the polygon
    /// sides on the edge from that corner to the next). Empty on malformed input (offsets that do not cover `polys`,
    /// a corner outside [param vertices]), which the caller answers in script.
    #[func]
    fn topology(
        &self,
        vertices: PackedVector3Array,
        polys: PackedInt32Array,
        offsets: PackedInt32Array,
    ) -> VarDictionary {
        let points: Vec<[f32; 3]> = vertices
            .as_slice()
            .iter()
            .map(|v| [v.x, v.y, v.z])
            .collect();
        let mut out = VarDictionary::new();
        if let Ok((roots, uses)) =
            gs_sim_navjoin::topology(&points, polys.as_slice(), offsets.as_slice())
        {
            out.set(
                "roots",
                &PackedInt32Array::from(roots.as_slice()).to_variant(),
            );
            out.set(
                "uses",
                &PackedInt32Array::from(uses.as_slice()).to_variant(),
            );
        }
        out
    }
}
