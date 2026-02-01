mod transform;

pub use transform::*;

use bevy::gizmos::config::{DefaultGizmoConfigGroup, GizmoConfigStore};
use bevy::prelude::*;

use crate::editor::EditorState;

pub struct EditorGizmosPlugin;

impl Plugin for EditorGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TransformGizmoPlugin)
            .add_systems(Startup, configure_gizmos)
            .add_systems(Update, draw_grid);
    }
}

/// Configure gizmo appearance
fn configure_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 3.0;
}

/// Draw editor grid
fn draw_grid(mut gizmos: Gizmos, editor_state: Res<EditorState>) {
    if !editor_state.gizmos_visible {
        return;
    }

    let grid_size = 10;
    let grid_spacing = 1.0;
    let color = Color::srgba(0.5, 0.5, 0.5, 0.3);

    for i in -grid_size..=grid_size {
        let pos = i as f32 * grid_spacing;

        // Lines along X axis
        gizmos.line(
            Vec3::new(-grid_size as f32 * grid_spacing, 0.0, pos),
            Vec3::new(grid_size as f32 * grid_spacing, 0.0, pos),
            color,
        );

        // Lines along Z axis
        gizmos.line(
            Vec3::new(pos, 0.0, -grid_size as f32 * grid_spacing),
            Vec3::new(pos, 0.0, grid_size as f32 * grid_spacing),
            color,
        );
    }

    // Axis indicators at origin
    gizmos.line(Vec3::ZERO, Vec3::X * 2.0, Color::srgb(1.0, 0.0, 0.0));
    gizmos.line(Vec3::ZERO, Vec3::Y * 2.0, Color::srgb(0.0, 1.0, 0.0));
    gizmos.line(Vec3::ZERO, Vec3::Z * 2.0, Color::srgb(0.0, 0.0, 1.0));
}
