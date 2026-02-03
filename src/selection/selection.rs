use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;
use bevy_outliner::prelude::*;
use bevy_spline_3d::prelude::Spline;

use crate::editor::{EditorCamera, EditorMode, EditorState};
use crate::scene::{SceneEntity, SplineMarker};
use crate::ui::Settings;

/// Marker component for selected entities
#[derive(Component, Default)]
pub struct Selected;

/// Resource to track multi-selection state
#[derive(Resource, Default)]
pub struct SelectionState {
    pub multi_select: bool,
}

pub struct SelectionSystemPlugin;

impl Plugin for SelectionSystemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectionState>()
            .add_systems(Update, (update_multi_select_state, handle_click_selection, sync_selection_outlines));
    }
}

/// Track shift key for multi-selection
fn update_multi_select_state(keyboard: Res<ButtonInput<KeyCode>>, mut state: ResMut<SelectionState>) {
    state.multi_select = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
}

/// Handle click-to-select using raycasting
#[allow(clippy::too_many_arguments)]
fn handle_click_selection(
    mouse_button: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    spatial_query: SpatialQuery,
    scene_entities: Query<Entity, With<SceneEntity>>,
    splines: Query<(Entity, &Spline, &GlobalTransform), With<SplineMarker>>,
    parent_query: Query<&ChildOf>,
    selected: Query<Entity, With<Selected>>,
    selection_state: Res<SelectionState>,
    mode: Res<State<EditorMode>>,
    mut commands: Commands,
    mut contexts: EguiContexts,
) {
    // Only select on left click
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't process selection if clicking on UI
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let Ok(window) = window_query.single() else {
        return;
    };

    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    // Create ray from camera through cursor
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    // First, check for spline proximity picking (splines don't have colliders)
    // This allows clicking near the spline curve to select it
    // Use screen-space distance for consistent picking regardless of camera distance
    let screen_pick_distance = 15.0; // Pixels from the curve
    let mut closest_spline: Option<(Entity, f32)> = None;

    for (entity, spline, spline_transform) in &splines {
        if let Some(screen_dist) = spline_screen_distance(
            spline,
            spline_transform,
            cursor_position,
            camera,
            camera_transform,
            32,
        ) {
            if screen_dist < screen_pick_distance {
                if closest_spline.is_none() || screen_dist < closest_spline.unwrap().1 {
                    closest_spline = Some((entity, screen_dist));
                }
            }
        }
    }

    // Cast ray against physics colliders
    let filter = SpatialQueryFilter::default();
    let physics_hit = spatial_query.cast_ray(
        ray.origin,
        ray.direction,
        100.0,
        true,
        &filter,
    );

    // Determine what to select - prefer physics hits over splines unless clicking very close to spline
    let entity_to_select = if let Some(hit) = physics_hit {
        let physics_entity = find_selectable_parent(hit.entity, &scene_entities, &parent_query);

        // If we have both a spline and a physics hit, prefer spline only if very close to curve
        if let (Some(phys_entity), Some((spline_entity, screen_dist))) = (physics_entity, closest_spline) {
            // Prefer spline if cursor is within 10 pixels of the curve
            if screen_dist < 10.0 {
                Some(spline_entity)
            } else {
                Some(phys_entity)
            }
        } else {
            physics_entity
        }
    } else {
        closest_spline.map(|(e, _)| e)
    };

    if let Some(entity_to_select) = entity_to_select {
        if !selection_state.multi_select {
            // Clear previous selection
            for entity in selected.iter() {
                commands.entity(entity).remove::<Selected>();
            }
        }

        // Toggle selection if multi-select and already selected
        if selection_state.multi_select && selected.get(entity_to_select).is_ok() {
            commands.entity(entity_to_select).remove::<Selected>();
        } else {
            commands.entity(entity_to_select).insert(Selected);
        }
    } else if !selection_state.multi_select && *mode.get() != EditorMode::Edit {
        // Clicked on nothing - clear selection (but not in Edit mode where we're editing control points)
        for entity in selected.iter() {
            commands.entity(entity).remove::<Selected>();
        }
    }
}

/// Calculate the minimum screen-space distance from cursor to a spline curve.
/// Returns None if the spline is invalid or no points are visible.
fn spline_screen_distance(
    spline: &Spline,
    spline_transform: &GlobalTransform,
    cursor_pos: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    samples: usize,
) -> Option<f32> {
    if !spline.is_valid() {
        return None;
    }

    // Sample points along the spline
    let points = spline.sample(samples);
    if points.is_empty() {
        return None;
    }

    let mut min_distance = f32::MAX;
    let mut found_visible = false;

    // Project each segment to screen space and check distance
    for i in 0..points.len().saturating_sub(1) {
        let p0_world = spline_transform.transform_point(points[i]);
        let p1_world = spline_transform.transform_point(points[i + 1]);

        // Project to screen space
        let Ok(p0_screen) = camera.world_to_viewport(camera_transform, p0_world) else {
            continue;
        };
        let Ok(p1_screen) = camera.world_to_viewport(camera_transform, p1_world) else {
            continue;
        };

        found_visible = true;

        // Calculate distance from cursor to this line segment in screen space
        let dist = point_to_segment_distance_2d(cursor_pos, p0_screen, p1_screen);
        min_distance = min_distance.min(dist);
    }

    if found_visible {
        Some(min_distance)
    } else {
        None
    }
}

/// Calculate the distance from a point to a 2D line segment.
fn point_to_segment_distance_2d(point: Vec2, seg_start: Vec2, seg_end: Vec2) -> f32 {
    let seg = seg_end - seg_start;
    let seg_len_sq = seg.length_squared();

    if seg_len_sq < 1e-6 {
        // Degenerate segment
        return point.distance(seg_start);
    }

    // Project point onto line, clamped to segment
    let t = ((point - seg_start).dot(seg) / seg_len_sq).clamp(0.0, 1.0);
    let closest = seg_start + seg * t;

    point.distance(closest)
}

/// Walk up the parent hierarchy to find an entity with SceneEntity component
fn find_selectable_parent(
    entity: Entity,
    scene_entities: &Query<Entity, With<SceneEntity>>,
    parent_query: &Query<&ChildOf>,
) -> Option<Entity> {
    // Check if the current entity is a scene entity
    if scene_entities.get(entity).is_ok() {
        return Some(entity);
    }

    // Walk up the parent chain
    if let Ok(child_of) = parent_query.get(entity) {
        return find_selectable_parent(child_of.parent(), scene_entities, parent_query);
    }

    // No selectable parent found
    None
}

/// Selection outline color - orange to match the previous AABB selection color
const SELECTION_OUTLINE_COLOR: Color = Color::srgb(1.0, 0.8, 0.0);

/// Sync MeshOutline components with Selected state
/// Adds outlines to selected mesh entities, removes them when deselected
fn sync_selection_outlines(
    mut commands: Commands,
    editor_state: Res<EditorState>,
    settings: Res<Settings>,
    // Entities that are selected and have a mesh but no outline yet
    needs_outline: Query<Entity, (With<Selected>, With<Mesh3d>, Without<MeshOutline>)>,
    // Entities that have an outline but are no longer selected
    has_outline_not_selected: Query<Entity, (With<MeshOutline>, Without<Selected>)>,
    // All entities with outlines (for preview mode cleanup and width updates)
    mut all_with_outline: Query<(Entity, &mut MeshOutline)>,
    // Also check children of selected entities (for GLTF models with nested meshes)
    selected_entities: Query<Entity, With<Selected>>,
    children_query: Query<&Children>,
    child_meshes: Query<Entity, (With<Mesh3d>, Without<MeshOutline>, Without<Selected>)>,
) {
    let outline_width = settings.gizmos.outline_width;

    // Don't show outlines when gizmos are hidden (preview mode)
    if !editor_state.gizmos_visible {
        // Remove ALL outlines when in preview mode (including selected entities)
        for (entity, _) in all_with_outline.iter() {
            commands.entity(entity).remove::<MeshOutline>();
        }
        return;
    }

    // Update outline width on existing outlines if it changed
    for (_, mut outline) in all_with_outline.iter_mut() {
        if outline.width != outline_width {
            outline.width = outline_width;
        }
    }

    // Add outlines to selected entities with meshes
    for entity in needs_outline.iter() {
        commands.entity(entity).insert(MeshOutline::new(SELECTION_OUTLINE_COLOR, outline_width));
    }

    // Add outlines to children of selected entities (for GLTF models)
    for selected_entity in selected_entities.iter() {
        add_outline_to_descendants(
            &mut commands,
            selected_entity,
            &children_query,
            &child_meshes,
            outline_width,
        );
    }

    // Remove outlines from entities that are no longer selected
    for entity in has_outline_not_selected.iter() {
        commands.entity(entity).remove::<MeshOutline>();
    }
}

/// Recursively add outlines to all mesh descendants of an entity
fn add_outline_to_descendants(
    commands: &mut Commands,
    entity: Entity,
    children_query: &Query<&Children>,
    child_meshes: &Query<Entity, (With<Mesh3d>, Without<MeshOutline>, Without<Selected>)>,
    outline_width: f32,
) {
    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            // If this child has a mesh and no outline, add one
            if child_meshes.get(child).is_ok() {
                commands.entity(child).insert(MeshOutline::new(SELECTION_OUTLINE_COLOR, outline_width));
            }
            // Recurse into grandchildren
            add_outline_to_descendants(commands, child, children_query, child_meshes, outline_width);
        }
    }
}
