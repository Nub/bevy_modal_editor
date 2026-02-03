//! Spline gizmo rendering and projection systems.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::distribution::SplineDistribution;
use crate::road::SplineRoad;
use crate::spline::{
    get_effective_control_points, get_effective_curve_points, CachedSplineCurve,
    ControlPointMarker, ProjectedSplineCache, SelectedControlPoint, SelectedSpline, Spline,
    SplineProjectionConfig, SplineType, project_spline_point,
};
use crate::surface::SplineMeshProjection;

use super::{EditorSettings, SplineXRayGizmos};

/// Run condition that checks if avian3d physics is available.
/// We check for the Gravity resource which is always present when PhysicsPlugins is added.
pub fn physics_available(gravity: Option<Res<Gravity>>) -> bool {
    gravity.is_some()
}

/// System to update cached spline curves when splines change.
pub fn update_spline_cache(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    changed_splines: Query<(Entity, &Spline), Changed<Spline>>,
    mut cached: Query<(Entity, &Spline, &mut CachedSplineCurve)>,
    uncached: Query<(Entity, &Spline), Without<CachedSplineCurve>>,
) {
    let resolution = settings.visuals.curve_resolution;

    // Add cache to splines that don't have one
    for (entity, spline) in &uncached {
        let points = if spline.is_valid() {
            spline.sample(resolution)
        } else {
            Vec::new()
        };
        commands.entity(entity).insert(CachedSplineCurve {
            points,
            resolution,
        });
    }

    // Update cache for changed splines
    for (entity, spline) in &changed_splines {
        if let Ok((_, _, mut cache)) = cached.get_mut(entity) {
            cache.points = if spline.is_valid() {
                spline.sample(resolution)
            } else {
                Vec::new()
            };
            cache.resolution = resolution;
        }
    }

    // Update cache if resolution changed
    if settings.is_changed() {
        for (_, spline, mut cache) in &mut cached {
            if cache.resolution != resolution {
                cache.points = if spline.is_valid() {
                    spline.sample(resolution)
                } else {
                    Vec::new()
                };
                cache.resolution = resolution;
            }
        }
    }
}

/// System to project spline visualization onto terrain when surface projection is enabled.
///
/// Uses the centralized projection code from [`crate::spline::projection`].
#[allow(clippy::too_many_arguments)]
pub fn project_spline_visualization(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    spatial_query: SpatialQuery,
    // Query all roads/distributions with projection (including changed ones)
    roads: Query<(&SplineRoad, &SplineMeshProjection)>,
    distributions: Query<(&SplineDistribution, &SplineMeshProjection)>,
    // Track when projection settings change
    changed_road_projections: Query<&SplineRoad, Changed<SplineMeshProjection>>,
    changed_dist_projections: Query<&SplineDistribution, Changed<SplineMeshProjection>>,
    // All splines with their caches
    all_splines: Query<(
        Entity,
        &Spline,
        Option<&CachedSplineCurve>,
        Option<&ProjectedSplineCache>,
    )>,
    // Splines that changed
    changed_splines: Query<
        Entity,
        Or<(Changed<CachedSplineCurve>, Changed<Spline>)>,
    >,
) {
    // Build a map of splines that have surface projection enabled via roads or distributions
    let mut projected_splines: std::collections::HashMap<Entity, &SplineMeshProjection> =
        std::collections::HashMap::new();

    for (road, projection) in &roads {
        if projection.enabled {
            projected_splines.insert(road.spline, projection);
        }
    }

    for (distribution, projection) in &distributions {
        if projection.enabled {
            projected_splines.insert(distribution.spline, projection);
        }
    }

    // Collect splines that need re-projection due to changed projection settings
    let mut splines_needing_update: std::collections::HashSet<Entity> =
        std::collections::HashSet::new();

    for road in &changed_road_projections {
        splines_needing_update.insert(road.spline);
    }

    for dist in &changed_dist_projections {
        splines_needing_update.insert(dist.spline);
    }

    // Also include splines that changed
    for entity in &changed_splines {
        splines_needing_update.insert(entity);
    }

    let visual_offset = settings.visuals.projection_visual_offset;

    // Process all splines
    for (entity, spline, cache, existing_projection) in &all_splines {
        let Some(surface_config) = projected_splines.get(&entity) else {
            // This spline doesn't have surface projection - remove projected cache if present
            if existing_projection.is_some() {
                commands.entity(entity).remove::<ProjectedSplineCache>();
            }
            continue;
        };

        let Some(cache) = cache else {
            continue;
        };

        // Check if we need to update this spline's projection
        let needs_update = existing_projection.is_none()  // No projection yet
            || splines_needing_update.contains(&entity)   // Spline or projection changed
            || needs_reprojection(spline, existing_projection); // Projection seems invalid

        if !needs_update {
            continue;
        }

        let config = SplineProjectionConfig {
            surface: surface_config,
            visual_offset,
        };

        // Project curve points
        let curve_points: Vec<Vec3> = cache
            .points
            .iter()
            .map(|&p| project_spline_point(&spatial_query, p, &config))
            .collect();

        // Project control points
        let control_points: Vec<Vec3> = spline
            .control_points
            .iter()
            .map(|&p| project_spline_point(&spatial_query, p, &config))
            .collect();

        commands.entity(entity).insert(ProjectedSplineCache {
            curve_points,
            control_points,
        });
    }
}

/// Check if a spline's projection cache appears invalid and needs re-projection.
/// This catches cases where projection failed initially (e.g., physics not ready).
fn needs_reprojection(spline: &Spline, projected: Option<&ProjectedSplineCache>) -> bool {
    let Some(projected) = projected else {
        return true;
    };

    // If control point counts don't match, definitely needs update
    if projected.control_points.len() != spline.control_points.len() {
        return true;
    }

    // If all projected points are identical to original points, projection likely failed
    // (Check first control point as a quick heuristic)
    if let (Some(&original), Some(&proj)) = (
        spline.control_points.first(),
        projected.control_points.first(),
    ) {
        // If they're exactly equal, projection probably didn't happen
        // (Real projection would have at least a small offset)
        if (original - proj).length_squared() < 0.0001 {
            return true;
        }
    }

    false
}

/// System to render spline curves using Bevy gizmos.
///
/// Uses cached sample points from [`CachedSplineCurve`] to avoid
/// resampling splines every frame. When surface projection is enabled,
/// uses projected points from [`ProjectedSplineCache`].
///
/// When x-ray is enabled, renders an additional faded pass that shows through geometry.
pub fn render_spline_curves(
    settings: Res<EditorSettings>,
    splines: Query<(
        &Spline,
        &GlobalTransform,
        Option<&SelectedSpline>,
        Option<&CachedSplineCurve>,
        Option<&ProjectedSplineCache>,
    )>,
    mut gizmos: Gizmos,
    mut xray_gizmos: Gizmos<SplineXRayGizmos>,
) {
    if !settings.show_gizmos {
        return;
    }

    for (spline, global_transform, selected, cache, projected) in &splines {
        if !spline.is_valid() {
            continue;
        }

        let color = if selected.is_some() {
            settings.colors.spline_selected
        } else {
            settings.colors.spline
        };

        // Use the centralized helper to get effective curve points
        let fallback_points;
        let points_ref = if let Some(pts) = get_effective_curve_points(cache, projected) {
            pts
        } else {
            fallback_points = spline.sample(settings.visuals.curve_resolution);
            &fallback_points
        };

        // Transform points from local to world space
        let world_points: Vec<Vec3> = points_ref
            .iter()
            .map(|&p| global_transform.transform_point(p))
            .collect();

        // X-ray pass (faded, renders through geometry)
        if settings.xray_enabled {
            let xray_color = color.with_alpha(settings.xray_opacity);
            for window in world_points.windows(2) {
                xray_gizmos.line(window[0], window[1], xray_color);
            }
            if spline.closed && world_points.len() >= 2 {
                xray_gizmos.line(world_points[world_points.len() - 1], world_points[0], xray_color);
            }
        }

        // Normal pass (with depth testing)
        for window in world_points.windows(2) {
            gizmos.line(window[0], window[1], color);
        }

        // For closed splines, connect last to first
        if spline.closed && world_points.len() >= 2 {
            gizmos.line(world_points[world_points.len() - 1], world_points[0], color);
        }

        // Render Bezier handle lines (using effective control points)
        if spline.spline_type == SplineType::CubicBezier && settings.show_handle_lines {
            let handle_points = get_effective_control_points(spline, projected);
            let world_handles: Vec<Vec3> = handle_points
                .iter()
                .map(|&p| global_transform.transform_point(p))
                .collect();
            render_bezier_handles(&world_handles, &settings, &mut gizmos, &mut xray_gizmos);
        }
    }
}

fn render_bezier_handles(
    points: &[Vec3],
    settings: &EditorSettings,
    gizmos: &mut Gizmos,
    xray_gizmos: &mut Gizmos<SplineXRayGizmos>,
) {
    if points.len() < 4 {
        return;
    }

    let num_segments = (points.len() - 1) / 3;
    for seg in 0..num_segments {
        let i = seg * 3;
        if i + 3 < points.len() {
            // X-ray pass
            if settings.xray_enabled {
                let xray_color = settings.colors.handle_line.with_alpha(settings.xray_opacity);
                xray_gizmos.line(points[i], points[i + 1], xray_color);
                xray_gizmos.line(points[i + 3], points[i + 2], xray_color);
            }
            // Normal pass - line from anchor to handle
            gizmos.line(points[i], points[i + 1], settings.colors.handle_line);
            gizmos.line(points[i + 3], points[i + 2], settings.colors.handle_line);
        }
    }
}

/// System to render control point spheres.
/// Uses the centralized helper to get effective positions.
/// When x-ray is enabled, renders an additional faded pass that shows through geometry.
pub fn render_control_points(
    settings: Res<EditorSettings>,
    splines: Query<(Entity, &Spline, &GlobalTransform, Option<&SelectedSpline>, Option<&ProjectedSplineCache>)>,
    selected_points: Query<&ControlPointMarker, With<SelectedControlPoint>>,
    mut gizmos: Gizmos,
    mut xray_gizmos: Gizmos<SplineXRayGizmos>,
) {
    if !settings.show_gizmos {
        return;
    }

    // Collect selected point indices per spline
    let mut selected_indices: std::collections::HashMap<Entity, std::collections::HashSet<usize>> =
        std::collections::HashMap::new();
    for marker in &selected_points {
        selected_indices
            .entry(marker.spline_entity)
            .or_default()
            .insert(marker.index);
    }

    let sizes = &settings.sizes;
    let colors = &settings.colors;

    for (entity, spline, global_transform, spline_selected, projected) in &splines {
        let entity_selected = selected_indices.get(&entity);
        let is_spline_selected = spline_selected.is_some();

        // Skip unselected splines if configured to only show control points for selected
        if settings.show_control_points_only_for_selected && !is_spline_selected {
            continue;
        }

        // Use the centralized helper to get effective control points
        let control_points = get_effective_control_points(spline, projected);

        // Transform control points to world space
        let world_points: Vec<Vec3> = control_points
            .iter()
            .map(|&p| global_transform.transform_point(p))
            .collect();

        // For CatmullRom splines, draw lines connecting adjacent control points
        // to show what each control point is attached to
        if spline.spline_type == SplineType::CatmullRom
            && world_points.len() >= 2
            && settings.show_handle_lines
        {
            render_catmull_rom_connections(&world_points, spline.closed, &settings, &mut gizmos, &mut xray_gizmos);
        }

        let last_index = world_points.len().saturating_sub(1);

        for (i, &point) in world_points.iter().enumerate() {
            let is_selected = entity_selected.is_some_and(|indices| indices.contains(&i));
            // Endpoints are first and last points, but only for open splines
            let is_endpoint = !spline.closed && (i == 0 || i == last_index);

            let color = if is_selected {
                colors.point_selected
            } else if is_endpoint {
                if is_spline_selected {
                    colors.endpoint_active
                } else {
                    colors.endpoint
                }
            } else if is_spline_selected {
                colors.point_active
            } else {
                colors.point
            };

            // Make points larger when spline is selected, even larger when point itself is selected
            // Endpoints are slightly larger than regular points
            let radius = if is_selected {
                sizes.point_radius * sizes.point_selected_scale
            } else if is_endpoint {
                if is_spline_selected {
                    sizes.point_radius * sizes.endpoint_selected_spline_scale
                } else {
                    sizes.point_radius * sizes.endpoint_scale
                }
            } else if is_spline_selected {
                sizes.point_radius * sizes.point_selected_spline_scale
            } else {
                sizes.point_radius
            };

            // X-ray pass (faded, renders through geometry)
            if settings.xray_enabled {
                let xray_color = color.with_alpha(settings.xray_opacity);
                xray_gizmos.sphere(Isometry3d::from_translation(point), radius, xray_color);
            }

            // Normal pass (with depth testing)
            gizmos.sphere(Isometry3d::from_translation(point), radius, color);
        }
    }
}

/// Render lines connecting adjacent control points for CatmullRom splines.
/// This helps visualize what each control point is attached to.
fn render_catmull_rom_connections(
    points: &[Vec3],
    closed: bool,
    settings: &EditorSettings,
    gizmos: &mut Gizmos,
    xray_gizmos: &mut Gizmos<SplineXRayGizmos>,
) {
    // X-ray pass
    if settings.xray_enabled {
        let xray_color = settings.colors.handle_line.with_alpha(settings.xray_opacity);
        for window in points.windows(2) {
            xray_gizmos.line(window[0], window[1], xray_color);
        }
        if closed && points.len() >= 2 {
            xray_gizmos.line(points[points.len() - 1], points[0], xray_color);
        }
    }

    // Normal pass - draw lines between adjacent control points
    for window in points.windows(2) {
        gizmos.line(window[0], window[1], settings.colors.handle_line);
    }

    // For closed splines, connect last to first
    if closed && points.len() >= 2 {
        gizmos.line(points[points.len() - 1], points[0], settings.colors.handle_line);
    }
}

/// Sync control point marker entities with spline control points.
/// Preserves selection state when markers are recreated.
pub fn sync_control_point_entities(
    mut commands: Commands,
    splines: Query<(Entity, &Spline), Changed<Spline>>,
    existing_markers: Query<(Entity, &ControlPointMarker)>,
    selected_points: Query<Entity, With<SelectedControlPoint>>,
) {
    for (spline_entity, spline) in &splines {
        // Collect which indices were selected before we despawn markers
        let mut selected_indices: Vec<usize> = Vec::new();
        for (marker_entity, marker) in &existing_markers {
            if marker.spline_entity == spline_entity && selected_points.contains(marker_entity) {
                selected_indices.push(marker.index);
            }
        }

        // Remove old markers for this spline
        for (marker_entity, marker) in &existing_markers {
            if marker.spline_entity == spline_entity {
                commands.entity(marker_entity).despawn();
            }
        }

        // Create new markers, preserving selection
        for (index, _) in spline.control_points.iter().enumerate() {
            let mut entity_commands = commands.spawn(ControlPointMarker {
                spline_entity,
                index,
            });

            // Re-apply selection if this index was previously selected
            if selected_indices.contains(&index) {
                entity_commands.insert(SelectedControlPoint);
            }
        }
    }
}

/// Clean up control point markers when spline entities are removed.
pub fn cleanup_orphaned_markers(
    mut commands: Commands,
    mut removed: RemovedComponents<Spline>,
    markers: Query<(Entity, &ControlPointMarker)>,
) {
    for removed_spline in removed.read() {
        for (marker_entity, marker) in &markers {
            if marker.spline_entity == removed_spline {
                commands.entity(marker_entity).despawn();
            }
        }
    }
}
