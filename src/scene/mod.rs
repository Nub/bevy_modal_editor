pub mod blockout;
mod collider_constructor;
pub mod generators;
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
use bevy::light::{ClusteredDecal, FogVolume};
use bevy::pbr::decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt};
use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use bevy_editor_game::{
    AssetRef, BaseMaterialProps, CustomEntityRegistry, MaterialDefinition,
    MaterialExtensionData, MaterialLibrary, MaterialRef, SceneComponentRegistry,
    ValidationRegistry,
};

use crate::materials::{load_base_textures, MaterialTypeRegistry, resolve_material_ref};
use bevy_outliner::prelude::{HasSilhouetteMesh, SilhouetteMesh};
use bevy_procedural::{ProceduralEntity, ProceduralPlacer, ProceduralTemplate};
use bevy_spline_3d::prelude::SplineFollower;
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
    let builder = DynamicSceneBuilder::from_world(world)
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
        .allow_component::<MaterialRef>()
        // Backwards compat: still extract old types if present on legacy entities
        .allow_component::<PrimitiveMaterial>()
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
        .allow_component::<SplineFollower>()
        // Fog
        .allow_component::<FogVolumeMarker>()
        // Decals
        .allow_component::<DecalMarker>()
        // VFX
        .allow_component::<bevy_vfx::VfxSystem>()
        // Effects
        .allow_component::<crate::effects::EffectMarker>()
        // Edited meshes
        .allow_component::<crate::modeling::marker::EditMeshMarker>()
        // Blockout shapes
        .allow_component::<StairsMarker>()
        .allow_component::<RampMarker>()
        .allow_component::<ArchMarker>()
        .allow_component::<LShapeMarker>()
        // Prefabs
        .allow_component::<crate::prefabs::PrefabInstance>()
        .allow_component::<crate::prefabs::PrefabRoot>()
        // External sources
        .allow_component::<GltfSource>()
        .allow_component::<SceneSource>()
        .allow_component::<RecursiveColliderConstructor>()
        // Procedural placement
        .allow_component::<ProceduralPlacer>()
        .allow_component::<ProceduralEntity>()
        .allow_component::<ProceduralTemplate>();

    // Asset references
    let mut builder = builder.allow_component::<AssetRef>();

    // Apply game-registered components
    if let Some(registry) = world.get_resource::<SceneComponentRegistry>() {
        builder = registry.apply(builder);
    }

    builder.extract_entities(entities).build()
}

/// Regenerate runtime components (meshes, materials, colliders, lights, fog volumes)
/// for entities loaded from a scene snapshot or file.
pub fn regenerate_runtime_components(world: &mut World) {
    // Migrate legacy entities: convert PrimitiveMaterial + MaterialType → MaterialRef
    migrate_legacy_materials(world);

    // Handle primitives with MaterialRef
    let mut primitives_to_update: Vec<(Entity, PrimitiveShape, MaterialRef)> = Vec::new();
    {
        let mut query = world.query_filtered::<(
            Entity,
            &PrimitiveMarker,
            Option<&MaterialRef>,
        ), Without<Mesh3d>>();
        for (entity, marker, mat_ref) in query.iter(world) {
            let mat_ref = mat_ref.cloned().unwrap_or_else(|| {
                // Fallback for entities with no MaterialRef at all
                MaterialRef::Inline(MaterialDefinition::standard(marker.shape.default_color()))
            });
            primitives_to_update.push((entity, marker.shape, mat_ref));
        }
    }

    // Read the library and registry (cloned to avoid borrow issues)
    let library = world
        .get_resource::<MaterialLibrary>()
        .cloned()
        .unwrap_or_default();
    let registry_types: Vec<_> = world
        .get_resource::<MaterialTypeRegistry>()
        .map(|r| {
            r.types
                .iter()
                .map(|e| (e.type_name.to_string(), e.apply as fn(&mut World, Entity, &BaseMaterialProps, Option<&str>)))
                .collect()
        })
        .unwrap_or_default();

    for (entity, shape, mat_ref) in primitives_to_update {
        let mesh_handle = world.resource_mut::<Assets<Mesh>>().add(shape.create_mesh());
        let collider = shape.create_collider();

        // Insert mesh and collider
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((Mesh3d(mesh_handle), collider));
        }

        // Resolve and apply material
        if let Some(def) = resolve_material_ref(&mat_ref, &library) {
            apply_material_def_from_fns(world, entity, def, &registry_types);
        }
    }

    // Handle edited meshes
    crate::modeling::marker::regenerate_edit_meshes(world);

    // Handle blockout shapes with MaterialRef
    regenerate_blockout_materials(world, &library, &registry_types);

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
                    radius: marker.radius,
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

    // Handle decals — collect entities missing either decal runtime component
    let mut decals_to_update: Vec<(Entity, DecalMarker)> = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &DecalMarker), (
            Without<ClusteredDecal>,
            Without<ForwardDecal>,
        )>();
        for (entity, marker) in query.iter(world) {
            decals_to_update.push((entity, marker.clone()));
        }
    }

    let asset_server_for_decals = world.resource::<AssetServer>().clone();
    for (entity, marker) in decals_to_update {
        insert_decal_components(world, entity, &marker, &asset_server_for_decals);
    }

    // Regenerate custom entity types
    if let Some(registry) = world.get_resource::<CustomEntityRegistry>() {
        let regen_entries: Vec<(fn(&World, Entity) -> bool, bevy_editor_game::RegenerateFn)> =
            registry
                .entries
                .iter()
                .filter_map(|e| {
                    e.entity_type
                        .regenerate
                        .map(|r| (e.has_component, r))
                })
                .collect();
        for (has_comp, regen_fn) in regen_entries {
            let entities: Vec<Entity> = {
                let mut q = world.query::<Entity>();
                q.iter(world).filter(|&e| has_comp(world, e)).collect()
            };
            for entity in entities {
                regen_fn(world, entity);
            }
        }
    }

    // Regenerate AssetRef entities (load Scene assets)
    let asset_refs_to_load: Vec<(Entity, AssetRef)> = {
        let mut query =
            world.query_filtered::<(Entity, &AssetRef), Without<SceneRoot>>();
        query
            .iter(world)
            .filter(|(_, ar)| ar.asset_type == bevy_editor_game::AssetType::Scene)
            .map(|(e, ar)| (e, ar.clone()))
            .collect()
    };
    for (entity, asset_ref) in asset_refs_to_load {
        let handle: Handle<Scene> =
            world.resource::<AssetServer>().load(&asset_ref.path);
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert(SceneRoot(handle));
        }
    }
}

/// Insert the correct runtime decal components for the given marker.
fn insert_decal_components(
    world: &mut World,
    entity: Entity,
    marker: &DecalMarker,
    asset_server: &AssetServer,
) {
    match marker.decal_type {
        DecalType::Clustered => {
            if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                entity_mut.insert((
                    ClusteredDecal {
                        base_color_texture: marker
                            .base_color_path
                            .as_ref()
                            .map(|p| asset_server.load(p.clone())),
                        normal_map_texture: marker
                            .normal_map_path
                            .as_ref()
                            .map(|p| asset_server.load(p.clone())),
                        emissive_texture: marker
                            .emissive_path
                            .as_ref()
                            .map(|p| asset_server.load(p.clone())),
                        ..default()
                    },
                    Visibility::default(),
                    Collider::cuboid(0.5, 0.5, 0.5),
                ));
                // Ensure no stale Forward components
                entity_mut.remove::<ForwardDecal>();
                entity_mut.remove::<MeshMaterial3d<ForwardDecalMaterial<StandardMaterial>>>();
            }
        }
        DecalType::Forward => {
            let base_mat = StandardMaterial {
                base_color_texture: marker
                    .base_color_path
                    .as_ref()
                    .map(|p| asset_server.load(p.clone())),
                normal_map_texture: marker
                    .normal_map_path
                    .as_ref()
                    .map(|p| asset_server.load(p.clone())),
                emissive_texture: marker
                    .emissive_path
                    .as_ref()
                    .map(|p| asset_server.load(p.clone())),
                ..default()
            };
            let handle = world
                .resource_mut::<Assets<ForwardDecalMaterial<StandardMaterial>>>()
                .add(ForwardDecalMaterial {
                    base: base_mat,
                    extension: ForwardDecalMaterialExt {
                        depth_fade_factor: marker.depth_fade_factor,
                    },
                });
            if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                entity_mut.insert((
                    ForwardDecal,
                    MeshMaterial3d(handle),
                    Visibility::default(),
                    Collider::cuboid(0.5, 0.5, 0.5),
                ));
                // Ensure no stale Clustered components
                entity_mut.remove::<ClusteredDecal>();
            }
        }
    }
}

/// Type-erased apply function signature for material entries
type ApplyFn = fn(&mut World, Entity, &BaseMaterialProps, Option<&str>);

/// Apply a material definition to an entity using pre-collected function pointers.
/// This avoids borrowing the MaterialTypeRegistry from the World during iteration.
fn apply_material_def_from_fns(
    world: &mut World,
    entity: Entity,
    def: &MaterialDefinition,
    registry_fns: &[(String, ApplyFn)],
) {
    match &def.extension {
        None => {
            // Standard material
            let mut mat = def.base.to_standard_material();
            let asset_server = world.resource::<AssetServer>().clone();
            load_base_textures(&mut mat, &def.base, &asset_server);
            let handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(mat);
            if let Ok(mut e) = world.get_entity_mut(entity) {
                e.insert(MeshMaterial3d(handle));
            }
        }
        Some(ext) => {
            // Extension apply fns already call load_base_textures via apply_material in materials/mod.rs
            if let Some((_, apply_fn)) = registry_fns.iter().find(|(name, _)| name == &ext.type_name)
            {
                apply_fn(world, entity, &def.base, Some(&ext.data));
            } else {
                warn!(
                    "Unknown material extension type '{}', falling back to standard",
                    ext.type_name
                );
                let mut mat = def.base.to_standard_material();
                let asset_server = world.resource::<AssetServer>().clone();
                load_base_textures(&mut mat, &def.base, &asset_server);
                let handle = world
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(mat);
                if let Ok(mut e) = world.get_entity_mut(entity) {
                    e.insert(MeshMaterial3d(handle));
                }
            }
        }
    }
}

/// Migrate legacy PrimitiveMaterial + MaterialType to MaterialRef.
/// Entities that have old components but no MaterialRef get one created.
fn migrate_legacy_materials(world: &mut World) {
    let mut to_migrate: Vec<(Entity, Color, MaterialType)> = Vec::new();
    {
        let mut query = world.query_filtered::<(
            Entity,
            Option<&PrimitiveMaterial>,
            Option<&MaterialType>,
            Option<&PrimitiveMarker>,
        ), Without<MaterialRef>>();
        for (entity, prim_mat, mat_type, prim_marker) in query.iter(world) {
            // Only migrate entities that have at least one old component
            if prim_mat.is_some() || mat_type.is_some() {
                let color = prim_mat
                    .map(|m| m.base_color)
                    .or_else(|| prim_marker.map(|m| m.shape.default_color()))
                    .unwrap_or(Color::srgb(0.5, 0.5, 0.5));
                let mat_type = mat_type.copied().unwrap_or_default();
                to_migrate.push((entity, color, mat_type));
            }
        }
    }

    let grid_default_data =
        ron::to_string(&crate::materials::grid::GridMaterialProps::default()).unwrap_or_default();

    for (entity, color, mat_type) in to_migrate {
        let mat_ref = match mat_type {
            MaterialType::Standard => MaterialRef::Inline(MaterialDefinition::standard(color)),
            MaterialType::Grid => MaterialRef::Inline(MaterialDefinition::with_extension(
                BaseMaterialProps {
                    base_color: color,
                    ..default()
                },
                "grid",
                grid_default_data.clone(),
            )),
        };
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(mat_ref);
        }
    }
}

/// Regenerate materials for blockout shapes that have MaterialRef but no rendered material.
fn regenerate_blockout_materials(
    world: &mut World,
    library: &MaterialLibrary,
    registry_fns: &[(String, ApplyFn)],
) {
    // Find blockout entities without a mesh material that have MaterialRef
    let mut blockouts: Vec<(Entity, MaterialRef)> = Vec::new();
    {
        // Blockout shapes have marker components but may lack MaterialRef
        // Check stairs, ramps, arches, l-shapes that have Mesh3d but no material yet
        let mut query = world.query_filtered::<(Entity, &MaterialRef), (
            Without<MeshMaterial3d<StandardMaterial>>,
            With<Mesh3d>,
        )>();
        for (entity, mat_ref) in query.iter(world) {
            blockouts.push((entity, mat_ref.clone()));
        }
    }

    for (entity, mat_ref) in blockouts {
        if let Some(def) = resolve_material_ref(&mat_ref, library) {
            apply_material_def_from_fns(world, entity, def, registry_fns);
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
        app.init_resource::<SceneComponentRegistry>()
            .init_resource::<CustomEntityRegistry>()
            .init_resource::<ValidationRegistry>()
            .add_plugins(PrimitivesPlugin)
            .add_plugins(SerializationPlugin)
            .add_plugins(GltfSourcePlugin)
            .add_plugins(SceneSourcePlugin)
            .add_plugins(ColliderConstructorPlugin)
            .add_plugins(BlockoutPlugin)
            .add_plugins(generators::SceneGeneratorPlugin)
            .add_message::<SpawnDemoSceneEvent>()
            .add_systems(Update, (handle_spawn_demo_scene, sync_decal_markers))
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
            // Edited mesh types
            .register_type::<crate::modeling::marker::EditMeshMarker>()
            // Fog volume types
            .register_type::<FogVolumeMarker>()
            // Decal types
            .register_type::<DecalMarker>()
            .register_type::<DecalType>()
            // Effect types
            .register_type::<crate::effects::EffectMarker>()
            .register_type::<crate::effects::EffectStep>()
            .register_type::<crate::effects::EffectTrigger>()
            .register_type::<crate::effects::EffectAction>()
            .register_type::<crate::effects::RigidBodyKind>()
            .register_type::<crate::effects::SpawnLocation>()
            // Material types (new system)
            .register_type::<MaterialRef>()
            .register_type::<MaterialDefinition>()
            .register_type::<BaseMaterialProps>()
            .register_type::<MaterialExtensionData>()
            .register_type::<bevy_editor_game::AlphaModeValue>()
            .register_type::<bevy_editor_game::ParallaxMappingMethodValue>()
            // Legacy material types (backwards compat)
            .register_type::<MaterialType>()
            .register_type::<PrimitiveMaterial>()
            // Asset reference types
            .register_type::<AssetRef>()
            .register_type::<bevy_editor_game::AssetType>();
    }
}

/// When `DecalMarker` changes (e.g. from inspector edits), rebuild decal runtime components.
fn sync_decal_markers(world: &mut World) {
    let changed_decals: Vec<(Entity, DecalMarker)> = {
        let mut query = world.query_filtered::<(Entity, &DecalMarker), Changed<DecalMarker>>();
        query
            .iter(world)
            .map(|(e, m)| (e, m.clone()))
            .collect()
    };

    if changed_decals.is_empty() {
        return;
    }

    let asset_server = world.resource::<AssetServer>().clone();
    for (entity, marker) in changed_decals {
        insert_decal_components(world, entity, &marker, &asset_server);
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
        let ground_color = Color::srgb(0.4, 0.4, 0.5);
        let ground_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let ground_material = materials.add(StandardMaterial {
            base_color: ground_color,
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Name::new("Ground"),
            PrimitiveMarker { shape: PrimitiveShape::Cube },
            MaterialRef::Inline(MaterialDefinition::standard(ground_color)),
            Mesh3d(ground_mesh),
            MeshMaterial3d(ground_material),
            Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
                .with_scale(Vec3::new(20.0, 1.0, 20.0)),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));

        // Central pillar
        let pillar_color = Color::srgb(0.6, 0.5, 0.4);
        let pillar_mesh = meshes.add(Cylinder::new(0.5, 1.0));
        let pillar_material = materials.add(StandardMaterial {
            base_color: pillar_color,
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Name::new("Central Pillar"),
            PrimitiveMarker { shape: PrimitiveShape::Cylinder },
            MaterialRef::Inline(MaterialDefinition::standard(pillar_color)),
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
                MaterialRef::Inline(MaterialDefinition::standard(pillar_color)),
                Mesh3d(pillar_mesh.clone()),
                MeshMaterial3d(pillar_material.clone()),
                Transform::from_translation(Vec3::new(x, 1.5, z))
                    .with_scale(Vec3::new(1.0, 3.0, 1.0)),
                RigidBody::Static,
                Collider::cylinder(0.5, 0.5),
            ));
        }

        // Dynamic spheres
        let sphere_color = Color::srgb(0.8, 0.3, 0.3);
        let sphere_mesh = meshes.add(Sphere::new(0.5));
        let sphere_material = materials.add(StandardMaterial {
            base_color: sphere_color,
            ..default()
        });
        for (i, (x, y, z)) in [(2.0, 5.0, 2.0), (-2.0, 7.0, -2.0), (0.0, 9.0, 3.0)].iter().enumerate() {
            commands.spawn((
                SceneEntity,
                Name::new(format!("Bouncy Sphere {}", i + 1)),
                PrimitiveMarker { shape: PrimitiveShape::Sphere },
                MaterialRef::Inline(MaterialDefinition::standard(sphere_color)),
                Mesh3d(sphere_mesh.clone()),
                MeshMaterial3d(sphere_material.clone()),
                Transform::from_translation(Vec3::new(*x, *y, *z)),
                RigidBody::Dynamic,
                Collider::sphere(0.5),
            ));
        }

        // Dynamic cube
        let cube_color = Color::srgb(0.3, 0.6, 0.8);
        let cube_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let cube_material = materials.add(StandardMaterial {
            base_color: cube_color,
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Name::new("Falling Cube"),
            PrimitiveMarker { shape: PrimitiveShape::Cube },
            MaterialRef::Inline(MaterialDefinition::standard(cube_color)),
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
                radius: 0.0,
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
                    radius: 0.0,
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
                    radius: 0.0,
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
