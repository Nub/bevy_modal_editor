mod collider_constructor;
mod gltf_source;
mod primitives;
mod serialization;

pub use collider_constructor::*;
pub use gltf_source::*;
pub use primitives::*;
pub use serialization::*;

use avian3d::prelude::*;
use bevy::prelude::*;

/// Marker component for entities that are part of the editable scene
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct SceneEntity;

/// Event to spawn the demo scene
#[derive(Message)]
pub struct SpawnDemoSceneEvent;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PrimitivesPlugin)
            .add_plugins(SerializationPlugin)
            .add_plugins(GltfSourcePlugin)
            .add_plugins(ColliderConstructorPlugin)
            .add_message::<SpawnDemoSceneEvent>()
            .add_systems(Update, handle_spawn_demo_scene)
            // Register types for scene serialization
            .register_type::<SceneEntity>()
            .register_type::<PrimitiveMarker>()
            .register_type::<PrimitiveShape>()
            .register_type::<GroupMarker>()
            .register_type::<Locked>()
            .register_type::<SceneLightMarker>()
            .register_type::<DirectionalLightMarker>()
            .register_type::<RecursiveColliderConstructor>()
            .register_type::<ColliderType>();
    }
}

/// Handle spawning the demo scene
fn handle_spawn_demo_scene(
    mut events: MessageReader<SpawnDemoSceneEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<SceneEntity>>,
) {
    for _ in events.read() {
        // Clear existing scene entities
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }

        // Ground plane
        let ground_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let ground_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.4, 0.5),
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Name::new("Ground"),
            PrimitiveMarker { shape: PrimitiveShape::Cube },
            Mesh3d(ground_mesh),
            MeshMaterial3d(ground_material),
            Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
                .with_scale(Vec3::new(20.0, 1.0, 20.0)),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));

        // Central pillar
        let pillar_mesh = meshes.add(Cylinder::new(0.5, 1.0));
        let pillar_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.5, 0.4),
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Name::new("Central Pillar"),
            PrimitiveMarker { shape: PrimitiveShape::Cylinder },
            Mesh3d(pillar_mesh.clone()),
            MeshMaterial3d(pillar_material.clone()),
            Transform::from_translation(Vec3::new(0.0, 2.0, 0.0))
                .with_scale(Vec3::new(1.5, 4.0, 1.5)),
            RigidBody::Static,
            Collider::cylinder(0.5, 0.5),
        ));

        // Corner pillars
        for (x, z) in [(-6.0, -6.0), (-6.0, 6.0), (6.0, -6.0), (6.0, 6.0)] {
            commands.spawn((
                SceneEntity,
                Name::new("Corner Pillar"),
                PrimitiveMarker { shape: PrimitiveShape::Cylinder },
                Mesh3d(pillar_mesh.clone()),
                MeshMaterial3d(pillar_material.clone()),
                Transform::from_translation(Vec3::new(x, 1.5, z))
                    .with_scale(Vec3::new(1.0, 3.0, 1.0)),
                RigidBody::Static,
                Collider::cylinder(0.5, 0.5),
            ));
        }

        // Dynamic spheres
        let sphere_mesh = meshes.add(Sphere::new(0.5));
        let sphere_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.3, 0.3),
            ..default()
        });
        for (i, (x, y, z)) in [(2.0, 5.0, 2.0), (-2.0, 7.0, -2.0), (0.0, 9.0, 3.0)].iter().enumerate() {
            commands.spawn((
                SceneEntity,
                Name::new(format!("Bouncy Sphere {}", i + 1)),
                PrimitiveMarker { shape: PrimitiveShape::Sphere },
                Mesh3d(sphere_mesh.clone()),
                MeshMaterial3d(sphere_material.clone()),
                Transform::from_translation(Vec3::new(*x, *y, *z)),
                RigidBody::Dynamic,
                Collider::sphere(0.5),
            ));
        }

        // Dynamic cube
        let cube_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let cube_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.6, 0.8),
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Name::new("Falling Cube"),
            PrimitiveMarker { shape: PrimitiveShape::Cube },
            Mesh3d(cube_mesh),
            MeshMaterial3d(cube_material),
            Transform::from_translation(Vec3::new(-3.0, 8.0, 1.0))
                .with_rotation(Quat::from_euler(EulerRot::XYZ, 0.3, 0.5, 0.2)),
            RigidBody::Dynamic,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));

        // Lights - Central warm light
        commands.spawn((
            SceneEntity,
            Name::new("Central Light"),
            SceneLightMarker {
                color: Color::srgb(1.0, 0.95, 0.8),
                intensity: 80000.0,
                range: 30.0,
                shadows_enabled: true,
            },
            PointLight {
                color: Color::srgb(1.0, 0.95, 0.8),
                intensity: 80000.0,
                range: 30.0,
                shadows_enabled: true,
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, 8.0, 0.0)),
        ));

        // Corner lights with colors
        let corner_lights = [
            ((-6.0, 4.0, -6.0), Color::srgb(1.0, 0.2, 0.2), "Red Light"),
            ((-6.0, 4.0, 6.0), Color::srgb(0.2, 0.4, 1.0), "Blue Light"),
            ((6.0, 4.0, -6.0), Color::srgb(0.2, 1.0, 0.3), "Green Light"),
            ((6.0, 4.0, 6.0), Color::srgb(0.9, 0.2, 1.0), "Purple Light"),
        ];

        for ((x, y, z), color, name) in corner_lights {
            commands.spawn((
                SceneEntity,
                Name::new(name),
                SceneLightMarker {
                    color,
                    intensity: 30000.0,
                    range: 15.0,
                    shadows_enabled: true,
                },
                PointLight {
                    color,
                    intensity: 30000.0,
                    range: 15.0,
                    shadows_enabled: true,
                    ..default()
                },
                Transform::from_translation(Vec3::new(x, y, z)),
            ));
        }

        // Ground accent lights
        let accent_lights = [
            ((3.0, 0.5, 0.0), Color::srgb(0.5, 0.8, 1.0)),
            ((-3.0, 0.5, 0.0), Color::srgb(1.0, 0.5, 0.8)),
            ((0.0, 0.5, 3.0), Color::srgb(0.8, 1.0, 0.5)),
            ((0.0, 0.5, -3.0), Color::srgb(1.0, 0.8, 0.5)),
        ];

        for (i, ((x, y, z), color)) in accent_lights.iter().enumerate() {
            commands.spawn((
                SceneEntity,
                Name::new(format!("Accent Light {}", i + 1)),
                SceneLightMarker {
                    color: *color,
                    intensity: 10000.0,
                    range: 8.0,
                    shadows_enabled: false,
                },
                PointLight {
                    color: *color,
                    intensity: 10000.0,
                    range: 8.0,
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_translation(Vec3::new(*x, *y, *z)),
            ));
        }

        info!("Demo scene spawned! Use Save Scene to export it.");
    }
}
