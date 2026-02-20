mod transform;

pub use transform::*;

use bevy::ecs::system::SystemState;
use bevy::gizmos::config::{DefaultGizmoConfigGroup, GizmoConfigGroup, GizmoConfigStore};
use bevy::prelude::*;
use bevy_infinite_grid::{InfiniteGridBundle, InfiniteGridPlugin, InfiniteGridSettings};

use bevy_editor_game::CustomEntityRegistry;

use avian3d::prelude::{Collider, SimpleCollider};

use crate::editor::{EditorMode, EditorState};
use crate::scene::{DecalMarker, DirectionalLightMarker, SceneLightMarker, SplineMarker};
use crate::selection::Selected;
use crate::ui::Settings;

/// Custom gizmo config group for x-ray transform gizmos (always visible through objects)
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct XRayGizmoConfig;

/// Dimmed x-ray gizmo config for inactive axes (half thickness)
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct XRayGizmoDimmed;

/// Thick x-ray gizmo config for selection circles on meshless entities
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SelectionCircleGizmo;

pub struct EditorGizmosPlugin;

impl Plugin for EditorGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<XRayGizmoConfig>()
            .init_gizmo_group::<XRayGizmoDimmed>()
            .init_gizmo_group::<SelectionCircleGizmo>()
            .add_plugins(TransformGizmoPlugin)
            .add_plugins(InfiniteGridPlugin)
            .add_systems(PreStartup, (configure_gizmos, spawn_grid))
            .add_systems(Update, (update_gizmo_settings, draw_directional_light_gizmos, draw_point_light_gizmos, draw_decal_gizmos, draw_custom_entity_gizmos, draw_meshless_selection_gizmos));
    }
}

/// Configure gizmo appearance from settings
fn configure_gizmos(mut config_store: ResMut<GizmoConfigStore>, settings: Res<Settings>) {
    // Configure default gizmos
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = settings.gizmos.line_width;

    // Configure x-ray gizmos (for active transform handles)
    let (xray_config, _) = config_store.config_mut::<XRayGizmoConfig>();
    xray_config.line.width = settings.gizmos.line_width;
    xray_config.depth_bias = -1.0; // Render on top of everything

    // Configure dimmed x-ray gizmos (for inactive axes - half thickness)
    let (dimmed_config, _) = config_store.config_mut::<XRayGizmoDimmed>();
    dimmed_config.line.width = settings.gizmos.line_width * 0.5;
    dimmed_config.depth_bias = -1.0;

    // Configure selection circle gizmos (thick, x-ray)
    let (sel_config, _) = config_store.config_mut::<SelectionCircleGizmo>();
    sel_config.line.width = settings.gizmos.line_width * 2.0;
    sel_config.depth_bias = -1.0;
}

/// Update gizmo settings when they change
fn update_gizmo_settings(settings: Res<Settings>, mut config_store: ResMut<GizmoConfigStore>) {
    if !settings.is_changed() {
        return;
    }
    // Update default gizmos
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = settings.gizmos.line_width;

    // Update x-ray gizmos
    let (xray_config, _) = config_store.config_mut::<XRayGizmoConfig>();
    xray_config.line.width = settings.gizmos.line_width;
    xray_config.depth_bias = -1.0;

    // Update dimmed x-ray gizmos
    let (dimmed_config, _) = config_store.config_mut::<XRayGizmoDimmed>();
    dimmed_config.line.width = settings.gizmos.line_width * 0.5;
    dimmed_config.depth_bias = -1.0;

    // Update selection circle gizmos
    let (sel_config, _) = config_store.config_mut::<SelectionCircleGizmo>();
    sel_config.line.width = settings.gizmos.line_width * 2.0;
    sel_config.depth_bias = -1.0;
}

/// Spawn the infinite grid
fn spawn_grid(mut commands: Commands) {
    commands.spawn(InfiniteGridBundle {
        settings: InfiniteGridSettings {
            x_axis_color: Color::srgb(0.8, 0.2, 0.2),
            z_axis_color: Color::srgb(0.2, 0.2, 0.8),
            minor_line_color: Color::srgba(0.3, 0.3, 0.3, 0.5),
            major_line_color: Color::srgba(0.5, 0.5, 0.5, 0.7),
            fadeout_distance: 200.0,
            dot_fadeout_strength: 0.1,
            scale: 1.0,
        },
        ..default()
    });
}

/// Draw gizmos for directional lights showing their direction
fn draw_directional_light_gizmos(
    mut gizmos: Gizmos,
    lights: Query<&GlobalTransform, With<DirectionalLightMarker>>,
    editor_state: Res<EditorState>,
) {
    if !editor_state.gizmos_visible {
        return;
    }

    for transform in lights.iter() {
        let position = transform.translation();
        // Directional lights point along their negative Z axis (forward direction)
        let direction = transform.forward();

        let arrow_length = 2.0;
        let arrow_head_length = 0.4;
        let arrow_head_width = 0.2;

        let end = position + direction * arrow_length;

        // Main line
        let sun_color = Color::srgb(1.0, 0.85, 0.3);
        gizmos.line(position, end, sun_color);

        // Arrow head
        let right = transform.right();
        let up = transform.up();

        let head_base = end - direction * arrow_head_length;
        gizmos.line(end, head_base + right * arrow_head_width, sun_color);
        gizmos.line(end, head_base - right * arrow_head_width, sun_color);
        gizmos.line(end, head_base + up * arrow_head_width, sun_color);
        gizmos.line(end, head_base - up * arrow_head_width, sun_color);

        // Circle at light position to make it easier to see
        gizmos.circle(Isometry3d::new(position, Quat::from_rotation_arc(Vec3::Z, *direction)), 0.3, sun_color);
    }
}

/// Draw gizmos for point lights showing a light bulb-like widget.
/// When selected, also draws a wireframe sphere showing the light's range.
fn draw_point_light_gizmos(
    mut gizmos: Gizmos,
    lights: Query<(&GlobalTransform, &SceneLightMarker, Has<Selected>)>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
) {
    if !editor_state.gizmos_visible {
        return;
    }
    let hide_selection = *mode.get() == EditorMode::Particle;

    for (transform, light_marker, is_selected) in lights.iter() {
        let position = transform.translation();
        let light_color = light_marker.color;

        // Draw a small sphere outline (3 circles for x, y, z planes)
        let radius = 0.3;
        gizmos.circle(Isometry3d::new(position, Quat::IDENTITY), radius, light_color);
        gizmos.circle(Isometry3d::new(position, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), radius, light_color);
        gizmos.circle(Isometry3d::new(position, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)), radius, light_color);

        // Draw rays emanating from the light (8 rays in a starburst pattern)
        let ray_length = 0.5;
        let ray_start = radius * 1.1;

        for i in 0..8 {
            let angle = (i as f32) * std::f32::consts::FRAC_PI_4;
            let dir = Vec3::new(angle.cos(), 0.0, angle.sin());
            gizmos.line(
                position + dir * ray_start,
                position + dir * (ray_start + ray_length),
                light_color,
            );
        }

        gizmos.line(
            position + Vec3::Y * ray_start,
            position + Vec3::Y * (ray_start + ray_length),
            light_color,
        );
        gizmos.line(
            position - Vec3::Y * ray_start,
            position - Vec3::Y * (ray_start + ray_length),
            light_color,
        );

        // When selected, draw range sphere (but not in Particle mode)
        if is_selected && !hide_selection {
            let range = light_marker.range;
            let range_color = light_color.with_alpha(0.3);
            gizmos.circle(Isometry3d::new(position, Quat::IDENTITY), range, range_color);
            gizmos.circle(Isometry3d::new(position, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), range, range_color);
            gizmos.circle(Isometry3d::new(position, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)), range, range_color);
        }
    }
}

/// Draw wireframe cube for selected decals showing the projection volume.
fn draw_decal_gizmos(
    mut gizmos: Gizmos,
    decals: Query<&GlobalTransform, (With<DecalMarker>, With<Selected>)>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
) {
    if !editor_state.gizmos_visible || *mode.get() == EditorMode::Particle {
        return;
    }

    let color = Color::srgba(0.3, 0.9, 0.5, 0.6);

    for global_transform in &decals {
        let t = global_transform.compute_transform();
        gizmos.cube(t, color);
    }
}

/// Draw a selection circle around selected entities that have no mesh (and
/// therefore receive no outline from `sync_selection_outlines`).  The circle
/// is sized to encompass the entity's collider AABB, or uses a small default
/// radius when no collider is present.
fn draw_meshless_selection_gizmos(
    mut gizmos: Gizmos<SelectionCircleGizmo>,
    selected: Query<(&GlobalTransform, Option<&Collider>), (With<Selected>, Without<Mesh3d>, Without<SplineMarker>)>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
) {
    if !editor_state.gizmos_visible || *mode.get() == EditorMode::Particle {
        return;
    }

    let color = Color::srgb(1.0, 0.8, 0.0);

    for (transform, collider) in &selected {
        let position = transform.translation();

        // Compute radius from collider AABB, or use a small default
        let radius = if let Some(collider) = collider {
            let aabb = collider.aabb(Vec3::ZERO, Quat::IDENTITY);
            let half = aabb.size() * 0.5;
            // Use the largest horizontal extent as the circle radius
            half.x.max(half.z) * 1.2
        } else {
            0.5
        };

        // Draw a horizontal circle at the entity's position
        gizmos.circle(
            Isometry3d::new(position, Quat::IDENTITY),
            radius,
            color,
        );
    }
}

/// Draw gizmos for custom entity types registered via `CustomEntityRegistry`.
fn draw_custom_entity_gizmos(world: &mut World) {
    if !world.resource::<EditorState>().gizmos_visible {
        return;
    }

    // Collect (has_component, draw_gizmo) pairs from the registry
    let Some(registry) = world.get_resource::<CustomEntityRegistry>() else {
        return;
    };
    let gizmo_entries: Vec<_> = registry
        .entries
        .iter()
        .filter_map(|e| e.entity_type.draw_gizmo.map(|draw| (e.has_component, draw)))
        .collect();

    if gizmo_entries.is_empty() {
        return;
    }

    // Collect matching (entity, transform) pairs
    let mut to_draw: Vec<(bevy_editor_game::GizmoDrawFn, GlobalTransform)> = Vec::new();
    {
        let mut query = world.query::<(Entity, &GlobalTransform)>();
        for (entity, global_transform) in query.iter(world) {
            for &(has_comp, draw_fn) in &gizmo_entries {
                if has_comp(world, entity) {
                    to_draw.push((draw_fn, *global_transform));
                }
            }
        }
    }

    // Extract Gizmos and draw
    let mut state = SystemState::<Gizmos>::new(world);
    let mut gizmos = state.get_mut(world);
    for (draw_fn, transform) in &to_draw {
        draw_fn(&mut gizmos, transform);
    }
    state.apply(world);
}
