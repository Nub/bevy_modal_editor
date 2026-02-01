mod transform;

pub use transform::*;

use bevy::gizmos::config::{DefaultGizmoConfigGroup, GizmoConfigStore};
use bevy::prelude::*;
use bevy_infinite_grid::{InfiniteGridBundle, InfiniteGridPlugin, InfiniteGridSettings};

use crate::editor::EditorState;

pub struct EditorGizmosPlugin;

impl Plugin for EditorGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TransformGizmoPlugin)
            .add_plugins(InfiniteGridPlugin)
            .add_systems(Startup, (configure_gizmos, spawn_grid))
            .add_systems(Update, draw_origin_axes);
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
