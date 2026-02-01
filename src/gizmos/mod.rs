mod transform;

pub use transform::*;

use bevy::gizmos::config::{DefaultGizmoConfigGroup, GizmoConfigStore};
use bevy::prelude::*;
use bevy_infinite_grid::{InfiniteGridBundle, InfiniteGridPlugin, InfiniteGridSettings};

use crate::editor::EditorState;
use crate::scene::{DirectionalLightMarker, SceneLightMarker};

pub struct EditorGizmosPlugin;

impl Plugin for EditorGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TransformGizmoPlugin)
            .add_plugins(InfiniteGridPlugin)
            .add_systems(Startup, (configure_gizmos, spawn_grid))
            .add_systems(Update, (draw_origin_axes, draw_directional_light_gizmos, draw_point_light_gizmos));
    }
}

/// Configure gizmo appearance
fn configure_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 3.0;
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

/// Draw origin axis indicators
fn draw_origin_axes(mut gizmos: Gizmos, editor_state: Res<EditorState>) {
    if !editor_state.gizmos_visible {
        return;
    }

    // Axis indicators at origin
    gizmos.line(Vec3::ZERO, Vec3::X * 2.0, Color::srgb(1.0, 0.0, 0.0));
    gizmos.line(Vec3::ZERO, Vec3::Y * 2.0, Color::srgb(0.0, 1.0, 0.0));
    gizmos.line(Vec3::ZERO, Vec3::Z * 2.0, Color::srgb(0.0, 0.0, 1.0));
}

/// Draw gizmos for directional lights showing their direction
fn draw_directional_light_gizmos(
    mut gizmos: Gizmos,
    lights: Query<&GlobalTransform, With<DirectionalLightMarker>>,
) {
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

/// Draw gizmos for point lights showing a light bulb-like widget
fn draw_point_light_gizmos(
    mut gizmos: Gizmos,
    lights: Query<(&GlobalTransform, &SceneLightMarker)>,
    editor_state: Res<EditorState>,
) {
    if !editor_state.gizmos_visible {
        return;
    }

    for (transform, light_marker) in lights.iter() {
        let position = transform.translation();

        // Use the light's color for the gizmo, but ensure it's visible
        let light_color = light_marker.color;

        // Draw a small sphere outline (3 circles for x, y, z planes)
        let radius = 0.3;
        gizmos.circle(Isometry3d::new(position, Quat::IDENTITY), radius, light_color);
        gizmos.circle(Isometry3d::new(position, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), radius, light_color);
        gizmos.circle(Isometry3d::new(position, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)), radius, light_color);

        // Draw rays emanating from the light (8 rays in a starburst pattern)
        let ray_length = 0.5;
        let ray_start = radius * 1.1; // Start slightly outside the sphere

        // Rays in the XZ plane
        for i in 0..8 {
            let angle = (i as f32) * std::f32::consts::FRAC_PI_4;
            let dir = Vec3::new(angle.cos(), 0.0, angle.sin());
            gizmos.line(
                position + dir * ray_start,
                position + dir * (ray_start + ray_length),
                light_color,
            );
        }

        // Rays going up and down
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
    }
}
