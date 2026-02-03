//! Spline projection for visualizing splines on terrain surfaces.
//!
//! This module provides a unified code path for projecting spline curves
//! and control points onto surfaces, used by both the editor visualization
//! and selection/picking systems.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::surface::{project_point, SplineMeshProjection};

use super::{CachedSplineCurve, Spline};

/// Cached projected points for spline visualization.
///
/// When a spline is associated with a road or distribution that has surface projection enabled,
/// this component stores the projected curve and control points for visualization and picking.
///
/// This component is automatically managed by the editor's projection system when the
/// editor plugin is active.
#[derive(Component, Default, Clone, Debug)]
pub struct ProjectedSplineCache {
    /// Projected curve sample points (for rendering the spline line).
    pub curve_points: Vec<Vec3>,
    /// Projected control point positions (for rendering and picking the control point spheres).
    pub control_points: Vec<Vec3>,
}

/// Configuration for spline projection visualization.
///
/// This is separate from the `SplineMeshProjection` used for meshes because
/// visualization may need different offsets to prevent z-fighting with gizmos.
#[derive(Clone, Debug)]
pub struct SplineProjectionConfig<'a> {
    /// The surface projection settings.
    pub surface: &'a SplineMeshProjection,
    /// Additional visual offset above the surface (for gizmo visibility).
    pub visual_offset: f32,
}

/// Project a single point for spline visualization.
///
/// Uses the surface projection settings plus an additional visual offset
/// to ensure the projected point is visible above the terrain.
pub fn project_spline_point(
    spatial_query: &SpatialQuery,
    point: Vec3,
    config: &SplineProjectionConfig,
) -> Vec3 {
    if let Some(hit) = project_point(spatial_query, point, config.surface) {
        // The surface projection already applies normal_offset.
        // Add visual_offset in the surface normal direction for gizmo visibility.
        hit.position + hit.normal * config.visual_offset
    } else {
        point
    }
}

/// Helper to get the effective control points for a spline.
///
/// Returns projected positions if available, otherwise returns the original positions.
/// This provides a single code path for any system that needs to work with
/// spline control points (rendering, picking, etc.).
///
/// # Example
///
/// ```ignore
/// use bevy_spline_3d::prelude::*;
///
/// fn my_system(
///     splines: Query<(&Spline, Option<&ProjectedSplineCache>)>,
/// ) {
///     for (spline, projected) in &splines {
///         // Works with both projected and non-projected splines
///         let points = get_effective_control_points(spline, projected);
///         for point in points {
///             // Use the point...
///         }
///     }
/// }
/// ```
pub fn get_effective_control_points<'a>(
    spline: &'a Spline,
    projected: Option<&'a ProjectedSplineCache>,
) -> &'a [Vec3] {
    if let Some(projected) = projected {
        &projected.control_points
    } else {
        &spline.control_points
    }
}

/// Helper to get the effective curve points for a spline.
///
/// Returns projected positions if available, then cached positions, otherwise returns None.
/// This provides a single code path for rendering spline curves regardless of
/// projection state.
///
/// # Example
///
/// ```ignore
/// use bevy_spline_3d::prelude::*;
///
/// fn render_spline(
///     spline: &Spline,
///     cached: Option<&CachedSplineCurve>,
///     projected: Option<&ProjectedSplineCache>,
///     settings: &EditorSettings,
/// ) {
///     let fallback;
///     let points = if let Some(pts) = get_effective_curve_points(cached, projected) {
///         pts
///     } else {
///         fallback = spline.sample(settings.curve_resolution);
///         &fallback
///     };
///     // Render the points...
/// }
/// ```
pub fn get_effective_curve_points<'a>(
    cached: Option<&'a CachedSplineCurve>,
    projected: Option<&'a ProjectedSplineCache>,
) -> Option<&'a [Vec3]> {
    if let Some(projected) = projected {
        if !projected.curve_points.is_empty() {
            return Some(&projected.curve_points);
        }
    }
    if let Some(cached) = cached {
        if !cached.points.is_empty() {
            return Some(&cached.points);
        }
    }
    None
}
