//! UV seam edge marking.
//!
//! Seams define where a mesh's surface is "cut" for UV unwrapping.
//! In Edge mode, pressing S toggles seam marking on the selected edge.
//! Seams are stored as a `HashSet<(u32, u32)>` of canonical edge pairs.

use std::collections::HashSet;

/// Canonical edge key: (min_vertex, max_vertex).
pub type SeamEdge = (u32, u32);

/// Make a canonical edge key from two vertex indices.
pub fn seam_key(a: u32, b: u32) -> SeamEdge {
    if a <= b { (a, b) } else { (b, a) }
}

/// Toggle a seam edge in the set. Returns true if the seam was added, false if removed.
pub fn toggle_seam(seams: &mut HashSet<SeamEdge>, a: u32, b: u32) -> bool {
    let key = seam_key(a, b);
    if seams.contains(&key) {
        seams.remove(&key);
        false
    } else {
        seams.insert(key);
        true
    }
}

/// Toggle seam on a half-edge (look up its vertices from the half-edge mesh).
pub fn toggle_seam_he(
    seams: &mut HashSet<SeamEdge>,
    he_mesh: &super::half_edge::HalfEdgeMesh,
    half_edge_id: u32,
) -> bool {
    let (from, to) = he_mesh.edge_vertices(half_edge_id);
    toggle_seam(seams, from, to)
}

/// Check if an edge is marked as a seam.
pub fn is_seam(seams: &HashSet<SeamEdge>, a: u32, b: u32) -> bool {
    seams.contains(&seam_key(a, b))
}

/// Collect all seam edges as vertex-pair sets, suitable for splitting
/// the mesh into UV islands during unwrapping.
pub fn seam_edge_set(seams: &HashSet<SeamEdge>) -> HashSet<SeamEdge> {
    seams.clone()
}
