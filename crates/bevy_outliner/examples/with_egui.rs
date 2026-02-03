//! Example showing bevy_outliner working with egui.
//!
//! Run with: cargo run --example with_egui --features bevy_egui

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use bevy_outliner::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, EguiPlugin::default(), OutlinePlugin))
        .init_resource::<OutlineConfig>()
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, ui_system)
        .add_systems(Update, (update_outlines, rotate_cube))
        .run();
}

#[derive(Resource)]
struct OutlineConfig {
    color: [f32; 4],
    width: f32,
    enabled: bool,
}

impl Default for OutlineConfig {
    fn default() -> Self {
        Self {
            color: [1.0, 0.5, 0.0, 1.0],
            width: 5.0,
            enabled: true,
        }
    }
}

#[derive(Component)]
struct OutlinedCube;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Cube with outline
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.2, 0.2))),
        Transform::from_xyz(0.0, 1.0, 0.0),
        MeshOutline::default(),
        OutlinedCube,
    ));

    // Ground plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(10.0, 10.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.3, 0.3))),
    ));

    // Light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Camera with outline support
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 5.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        OutlineSettings::default(),
    ));
}

fn ui_system(mut contexts: EguiContexts, mut config: ResMut<OutlineConfig>) -> Result {
    egui::Window::new("Outline Settings").show(contexts.ctx_mut()?, |ui| {
        ui.checkbox(&mut config.enabled, "Enable Outlines");
        ui.add(egui::Slider::new(&mut config.width, 1.0..=20.0).text("Width"));
        ui.color_edit_button_rgba_unmultiplied(&mut config.color);
    });
    Ok(())
}

fn update_outlines(
    config: Res<OutlineConfig>,
    mut query: Query<&mut MeshOutline, With<OutlinedCube>>,
    mut camera_query: Query<&mut OutlineSettings>,
) {
    if !config.is_changed() {
        return;
    }

    for mut outline in query.iter_mut() {
        outline.color = LinearRgba::new(
            config.color[0],
            config.color[1],
            config.color[2],
            config.color[3],
        );
        outline.width = config.width;
    }

    for mut settings in camera_query.iter_mut() {
        settings.enabled = config.enabled;
    }
}

fn rotate_cube(time: Res<Time>, mut query: Query<&mut Transform, With<OutlinedCube>>) {
    for mut transform in query.iter_mut() {
        transform.rotate_y(time.delta_secs() * 0.5);
    }
}
