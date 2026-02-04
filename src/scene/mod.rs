pub mod blockout;
mod collider_constructor;
mod gltf_source;
mod primitives;
mod scene_source;
mod serialization;

pub use blockout::*;
pub use collider_constructor::*;
pub use gltf_source::*;
pub use primitives::*;
pub use scene_source::*;
pub use serialization::*;

use avian3d::prelude::*;
use bevy::ecs::entity::EntityHashMap;
use bevy::light::FogVolume;
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use bevy_grid_shader::GridMaterial;
use bevy_outliner::prelude::{HasSilhouetteMesh, SilhouetteMesh};
use bevy_spline_3d::distribution::{
    DistributedInstance, DistributionOrientation, DistributionSource, DistributionSpacing,
    SplineDistribution,
};
use bevy_spline_3d::prelude::{Spline, SplineType};
use serde::de::DeserializeSeed;

/// Marker component for entities that are part of the editable scene
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct SceneEntity;

/// Marker component for entities that are procedurally generated from scene objects
/// (e.g. road meshes, intersection meshes, distributed instances) but not tracked as
/// `SceneEntity`. Enables a future baking process.
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct SceneProceduralObject;

/// Build a DynamicScene from the world with editor-relevant components.
/// This is the single source of truth for which components are included in snapshots and saves.
pub fn build_editor_scene(world: &World, entities: impl Iterator<Item = Entity>) -> DynamicScene {
    DynamicSceneBuilder::from_world(world)
        .deny_all()
        // Core
        .allow_component::<SceneEntity>()
        .allow_component::<Name>()
        .allow_component::<Transform>()
        .allow_component::<RigidBody>()
        .allow_component::<ChildOf>()
        .allow_component::<Children>()
        // Primitives
        .allow_component::<PrimitiveMarker>()
        .allow_component::<MaterialType>()
        // Groups
        .allow_component::<GroupMarker>()
        .allow_component::<Locked>()
        // Lights
        .allow_component::<SceneLightMarker>()
        .allow_component::<DirectionalLightMarker>()
        // Splines
        .allow_component::<SplineMarker>()
        .allow_component::<Spline>()
        .allow_component::<SplineDistribution>()
        .allow_component::<DistributionSource>()
        .allow_component::<DistributedInstance>()
        // Fog
        .allow_component::<FogVolumeMarker>()
        // Blockout shapes
        .allow_component::<StairsMarker>()
        .allow_component::<RampMarker>()
        .allow_component::<ArchMarker>()
        .allow_component::<LShapeMarker>()
        // Game markers
        .allow_component::<SpawnPoint>()
        // External sources
        .allow_component::<GltfSource>()
        .allow_component::<SceneSource>()
        .allow_component::<RecursiveColliderConstructor>()
        .extract_entities(entities)
        .build()
}

/// Regenerate runtime components (meshes, materials, colliders, lights, fog volumes)
/// for entities loaded from a scene snapshot or file.
pub fn regenerate_runtime_components(world: &mut World) {
    // Handle primitives - collect entity, shape, and material type
    let mut primitives_to_update: Vec<(Entity, PrimitiveShape, MaterialType)> = Vec::new();
    {
        let mut query = world
            .query_filtered::<(Entity, &PrimitiveMarker, Option<&MaterialType>), Without<Mesh3d>>();
        for (entity, marker, mat_type) in query.iter(world) {
            primitives_to_update.push((
                entity,
                marker.shape,
                mat_type.copied().unwrap_or_default(),
            ));
        }
    }

    for (entity, shape, mat_type) in primitives_to_update {
        let mesh_handle = world.resource_mut::<Assets<Mesh>>().add(shape.create_mesh());
        let collider = shape.create_collider();

        match mat_type {
            MaterialType::Standard => {
                let material_handle = world
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(shape.create_material());
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    entity_mut.insert((
                        Mesh3d(mesh_handle),
                        collider,
                        MeshMaterial3d(material_handle),
                    ));
                }
            }
            MaterialType::Grid => {
                let grid_mat = ExtendedMaterial {
                    base: shape.create_material(),
                    extension: GridMaterial::default(),
                };
                let material_handle = world.resource_mut::<Assets<GridMat>>().add(grid_mat);
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    entity_mut.insert((
                        Mesh3d(mesh_handle),
                        collider,
                        MeshMaterial3d(material_handle),
                    ));
                }
            }
        }
    }

    // Handle point lights
    let mut lights_to_update: Vec<(Entity, SceneLightMarker)> = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &SceneLightMarker), Without<PointLight>>();
        for (entity, marker) in query.iter(world) {
            lights_to_update.push((entity, marker.clone()));
        }
    }

    for (entity, marker) in lights_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                PointLight {
                    color: marker.color,
                    intensity: marker.intensity,
                    range: marker.range,
                    shadows_enabled: marker.shadows_enabled,
                    ..default()
                },
                Visibility::default(),
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }

    // Handle directional lights
    let mut dir_lights_to_update: Vec<(Entity, DirectionalLightMarker)> = Vec::new();
    {
        let mut query = world
            .query_filtered::<(Entity, &DirectionalLightMarker), Without<DirectionalLight>>();
        for (entity, marker) in query.iter(world) {
            dir_lights_to_update.push((entity, marker.clone()));
        }
    }

    for (entity, marker) in dir_lights_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                DirectionalLight {
                    color: marker.color,
                    illuminance: marker.illuminance,
                    shadows_enabled: marker.shadows_enabled,
                    ..default()
                },
                Visibility::default(),
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }

    // Handle spline visibility (splines use proximity-based picking, not colliders)
    let mut splines_to_update: Vec<Entity> = Vec::new();
    {
        let mut query = world.query_filtered::<Entity, (With<SplineMarker>, Without<Visibility>)>();
        for entity in query.iter(world) {
            splines_to_update.push(entity);
        }
    }

    for entity in splines_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert(Visibility::default());
        }
    }

    // Handle fog volumes
    let mut fog_to_update: Vec<(Entity, FogVolumeMarker)> = Vec::new();
    {
        let mut query =
            world.query_filtered::<(Entity, &FogVolumeMarker), Without<FogVolume>>();
        for (entity, marker) in query.iter(world) {
            fog_to_update.push((entity, marker.clone()));
        }
    }

    for (entity, marker) in fog_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                FogVolume {
                    fog_color: marker.fog_color,
                    density_factor: marker.density_factor,
                    absorption: marker.absorption,
                    scattering: marker.scattering,
                    scattering_asymmetry: marker.scattering_asymmetry,
                    light_tint: marker.light_tint,
                    light_intensity: marker.light_intensity,
                    ..default()
                },
                Visibility::default(),
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }

    // Handle spawn points (need visibility and collider for selection)
    let mut spawn_points_to_update: Vec<Entity> = Vec::new();
    {
        let mut query =
            world.query_filtered::<Entity, (With<SpawnPoint>, Without<Visibility>)>();
        for entity in query.iter(world) {
            spawn_points_to_update.push(entity);
        }
    }

    for entity in spawn_points_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                Visibility::default(),
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }

}

/// Restore the scene from serialized RON data.
///
/// This is the shared implementation used by both undo/redo and play/reset.
/// It handles: silhouette cleanup, despawning SceneEntity entities,
/// RON deserialization, writing to world, and regenerating runtime components.
pub fn restore_scene_from_data(world: &mut World, data: &str) {
    // Clean up silhouette entities BEFORE despawning scene entities.
    // The outliner creates separate SilhouetteMesh entities (on render layer 31) that are
    // NOT children of the source entity. If we despawn the source first, the outliner's
    // RemovedComponents<MeshOutline> cleanup fails because the source entity is already gone,
    // leaving orphaned silhouettes that continue rendering.
    let silhouettes_to_remove: Vec<Entity> = {
        let mut query = world.query_filtered::<&HasSilhouetteMesh, With<SceneEntity>>();
        query.iter(world).map(|h| h.silhouette).collect()
    };
    // Also collect silhouettes from children of scene entities (e.g. GLTF mesh children)
    let child_silhouettes: Vec<Entity> = {
        let mut query = world.query_filtered::<&HasSilhouetteMesh, Without<SceneEntity>>();
        query.iter(world).map(|h| h.silhouette).collect()
    };
    for entity in silhouettes_to_remove.into_iter().chain(child_silhouettes) {
        world.despawn(entity);
    }

    // Also despawn any orphaned SilhouetteMesh entities that may have lost their source
    let orphaned_silhouettes: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SilhouetteMesh>>();
        query.iter(world).collect()
    };
    for entity in orphaned_silhouettes {
        world.despawn(entity);
    }

    // Clear existing scene entities
    let entities_to_remove: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };

    info!("Removing {} existing scene entities", entities_to_remove.len());
    for entity in entities_to_remove {
        world.despawn(entity);
    }

    // Deserialize the snapshot
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();

    let scene_deserializer = SceneDeserializer {
        type_registry: &type_registry,
    };

    let Ok(mut ron_deserializer) = ron::de::Deserializer::from_str(data) else {
        warn!("Failed to parse scene data");
        return;
    };

    let Ok(scene) = scene_deserializer.deserialize(&mut ron_deserializer) else {
        warn!("Failed to deserialize scene data");
        return;
    };

    drop(type_registry);

    // Write scene to world
    let mut entity_map = EntityHashMap::default();
    if let Err(e) = scene.write_to_world(world, &mut entity_map) {
        warn!("Failed to restore scene: {:?}", e);
        return;
    }

    info!("Wrote {} entities to world from scene data", entity_map.len());

    // Regenerate meshes, materials, and colliders
    regenerate_runtime_components(world);

    info!("Scene restoration complete");
}

/// Event to spawn the demo scene
#[derive(Message)]
pub struct SpawnDemoSceneEvent;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PrimitivesPlugin)
            .add_plugins(SerializationPlugin)
            .add_plugins(GltfSourcePlugin)
            .add_plugins(SceneSourcePlugin)
            .add_plugins(ColliderConstructorPlugin)
            .add_plugins(BlockoutPlugin)
            .add_message::<SpawnDemoSceneEvent>()
            .add_systems(Update, handle_spawn_demo_scene)
            // Register types for scene serialization
            .register_type::<SceneEntity>()
            .register_type::<SceneProceduralObject>()
            .register_type::<PrimitiveMarker>()
            .register_type::<PrimitiveShape>()
            .register_type::<GroupMarker>()
            .register_type::<Locked>()
            .register_type::<SceneLightMarker>()
            .register_type::<DirectionalLightMarker>()
            .register_type::<RecursiveColliderConstructor>()
            .register_type::<ColliderType>()
            .register_type::<SceneSource>()
            // Spline types
            .register_type::<SplineMarker>()
            .register_type::<Spline>()
            .register_type::<SplineType>()
            // Spline distribution types
            .register_type::<SplineDistribution>()
            .register_type::<DistributionOrientation>()
            .register_type::<DistributionSpacing>()
            .register_type::<DistributionSource>()
            .register_type::<DistributedInstance>()
            // Fog volume types
            .register_type::<FogVolumeMarker>()
            // Material types
            .register_type::<MaterialType>()
            // Game markers
            .register_type::<SpawnPoint>();
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
