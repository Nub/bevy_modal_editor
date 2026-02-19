//! Soft selection: distance-based falloff weighting for vertex transforms.
//!
//! When soft selection is active, transforming selected vertices also moves
//! nearby unselected vertices with a falloff weight based on distance.

use bevy::prelude::*;
use std::collections::HashSet;

use super::edit_mesh::EditMesh;

/// Falloff curve type for soft selection.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FalloffCurve {
    /// Linear falloff: weight = 1 - (d / radius).
    Linear,
    /// Smooth (cosine) falloff: weight = (cos(PI * d / radius) + 1) / 2.
    #[default]
    Smooth,
    /// Sharp falloff: weight = (1 - d / radius)^2.
    Sharp,
    /// Root falloff: weight = sqrt(1 - d / radius).
    Root,
}

impl FalloffCurve {
    pub fn display_name(&self) -> &'static str {
        match self {
            FalloffCurve::Linear => "Linear",
            FalloffCurve::Smooth => "Smooth",
            FalloffCurve::Sharp => "Sharp",
            FalloffCurve::Root => "Root",
        }
    }

    /// Compute falloff weight for a normalized distance (0..1).
    pub fn weight(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FalloffCurve::Linear => 1.0 - t,
            FalloffCurve::Smooth => (std::f32::consts::PI * t).cos() * 0.5 + 0.5,
            FalloffCurve::Sharp => (1.0 - t) * (1.0 - t),
            FalloffCurve::Root => (1.0 - t).sqrt(),
        }
    }
}

/// Compute soft selection weights for all vertices.
///
/// Returns a Vec of weights (0..1) indexed by vertex index.
/// Selected vertices get weight 1.0. Nearby vertices get falloff weights.
/// Vertices beyond `radius` get weight 0.0.
pub fn compute_soft_weights(
    mesh: &EditMesh,
    selected: &HashSet<u32>,
    radius: f32,
    curve: FalloffCurve,
) -> Vec<f32> {
    let mut weights = vec![0.0f32; mesh.positions.len()];

    if radius <= 0.0 || selected.is_empty() {
        for &vi in selected {
            if (vi as usize) < weights.len() {
                weights[vi as usize] = 1.0;
            }
        }
        return weights;
    }

    // Compute average position of selected vertices (for distance reference)
    // Use per-vertex distance to nearest selected vertex for more natural falloff
    for vi in 0..mesh.positions.len() {
        let vi32 = vi as u32;
        if selected.contains(&vi32) {
            weights[vi] = 1.0;
            continue;
        }

        let pos = mesh.positions[vi];
        let mut min_dist = f32::MAX;
        for &si in selected {
            if (si as usize) < mesh.positions.len() {
                let d = pos.distance(mesh.positions[si as usize]);
                min_dist = min_dist.min(d);
            }
        }

        if min_dist < radius {
            weights[vi] = curve.weight(min_dist / radius);
        }
    }

    weights
}

/// Apply a displacement to vertices using soft selection weights.
///
/// Each vertex is moved by `delta * weight[vertex]`.
pub fn apply_soft_displacement(
    mesh: &EditMesh,
    weights: &[f32],
    delta: Vec3,
) -> EditMesh {
    let mut result = mesh.clone();

    for (vi, &w) in weights.iter().enumerate() {
        if w > 0.0 {
            result.positions[vi] += delta * w;
        }
    }

    result.recompute_normals();
    result
}
