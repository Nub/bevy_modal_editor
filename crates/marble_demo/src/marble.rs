use avian3d::prelude::*;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy_editor_game::{GameCamera, GameEntity, GameStartedEvent, GameState};

use crate::SpawnPoint;

/// Marker component for the marble entity
#[derive(Component)]
pub struct Marble;

/// Configuration for marble physics
#[derive(Resource)]
pub struct MarbleConfig {
    /// Force applied when rolling
    pub move_force: f32,
    /// Impulse applied when jumping
    pub jump_impulse: f32,
    /// Maximum velocity magnitude
    pub max_speed: f32,
}

impl Default for MarbleConfig {
    fn default() -> Self {
        Self {
            move_force: 1.0,
            jump_impulse: 1.0,
            max_speed: 10.0,
        }
    }
}

pub struct MarblePlugin;

impl Plugin for MarblePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarbleConfig>()
            .add_systems(Update, spawn_marble_on_game_start)
            .add_systems(Update, marble_input.run_if(in_state(GameState::Playing)));
    }
}

/// Spawn the marble at the SpawnPoint position when the game starts
fn spawn_marble_on_game_start(
    mut events: MessageReader<GameStartedEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    spawn_points: Query<&Transform, With<SpawnPoint>>,
) {
    for _ in events.read() {
        // Find spawn point position
        let spawn_pos = spawn_points
            .iter()
            .next()
            .map(|t| t.translation)
            .unwrap_or(Vec3::new(0.0, 2.0, 0.0));

        info!("Spawning marble at {:?}", spawn_pos);

        let normal_map = asset_server.load_with_settings(
            "textures/marble_normal.png",
            |s: &mut bevy::image::ImageLoaderSettings| {
                s.sampler = bevy::image::ImageSampler::Descriptor(
                    bevy::image::ImageSamplerDescriptor {
                        address_mode_u: bevy::image::ImageAddressMode::Repeat,
                        address_mode_v: bevy::image::ImageAddressMode::Repeat,
                        ..default()
                    },
                );
            },
        );

        commands.spawn((
            Marble,
            GameEntity,
            Name::new("Marble"),
            Mesh3d(meshes.add(
                Sphere::new(0.5)
                    .mesh()
                    .build()
                    .with_generated_tangents()
                    .expect("sphere should support tangent generation"),
            )),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 1.0, 0.88),
                specular_transmission: 0.9,
                diffuse_transmission: 1.0,
                thickness: 1.8,
                ior: 1.5,
                perceptual_roughness: 0.12,
                normal_map_texture: Some(normal_map),
                uv_transform: Affine2::from_scale(Vec2::splat(0.2)),
                ..default()
            })),
            Transform::from_translation(spawn_pos),
            RigidBody::Dynamic,
            Collider::sphere(0.5),
            Restitution::new(0.3),
            Friction::new(0.8),
            LinearDamping(0.5),
            AngularDamping(0.3),
        ));
    }
}

/// Handle marble movement input (WASD + Space) using Avian3D's Forces query data
fn marble_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    config: Res<MarbleConfig>,
    mut marbles: Query<Forces, With<Marble>>,
    camera_query: Query<&GlobalTransform, With<GameCamera>>,
) {
    let Ok(mut forces) = marbles.single_mut() else {
        return;
    };

    // Get camera forward/right for camera-relative movement
    let (cam_forward, cam_right) = if let Ok(cam_transform) = camera_query.single() {
        let forward = cam_transform.forward();
        let forward_flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
        let right = cam_transform.right();
        let right_flat = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();
        (forward_flat, right_flat)
    } else {
        (Vec3::NEG_Z, Vec3::X)
    };

    // Calculate movement direction
    let mut direction = Vec3::ZERO;
    if keyboard.pressed(KeyCode::KeyW) {
        direction += cam_forward;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        direction -= cam_forward;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        direction += cam_right;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        direction -= cam_right;
    }

    // Apply force (only if under max speed)
    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
        let current_speed = forces.linear_velocity().length();
        if current_speed < config.max_speed {
            forces.apply_force(direction * config.move_force);
        }
    }

    // Jump
    if keyboard.just_pressed(KeyCode::Space) {
        // Simple ground check: only jump if vertical velocity is near zero
        if forces.linear_velocity().y.abs() < 0.5 {
            forces.apply_linear_impulse(Vec3::Y * config.jump_impulse);
        }
    }
}
