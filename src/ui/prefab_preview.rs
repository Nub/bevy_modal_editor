//! 3D preview viewport for prefabs in the insert palette, rendered to a
//! texture and displayed in the palette's side panel via egui.
//!
//! Mirrors `gltf_preview.rs` but loads RON scene files and regenerates
//! runtime components (meshes, materials, colliders) via the scene system.

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use bevy_egui::EguiUserTextures;
use serde::de::DeserializeSeed;
use std::fs;

use crate::scene::regenerate_runtime_components;
use crate::ui::preview_common::{
    apply_preview_rotation, fit_transform_from_extents, register_preview_egui_texture,
    spawn_preview_scene, PreviewSceneConfig, PreviewTexture, PREVIEW_ROTATION_SPEED,
};

/// Render layer for the prefab preview scene.
const PREFAB_PREVIEW_RENDER_LAYER: usize = 26;

#[derive(Component)]
struct PrefabPreviewCamera;

#[derive(Component)]
struct PrefabPreviewLight;

#[derive(Component)]
struct PrefabPreviewRoot;

/// Marker on entities loaded into the prefab preview scene.
#[derive(Component)]
struct PrefabPreviewChild;

/// State resource for the prefab preview system.
#[derive(Resource)]
pub struct PrefabPreviewState {
    /// Shared render texture + egui texture id.
    pub texture: PreviewTexture,
    /// Set by the palette to indicate which prefab scene to preview.
    pub current_path: Option<String>,
    /// Change detection — tracks last loaded path.
    last_loaded_path: Option<String>,
    /// Current rotation angle.
    rotation_angle: f32,
    /// Computed fit-to-frame scale.
    fit_scale: Vec3,
    /// Computed fit-to-frame translation.
    fit_translation: Vec3,
    /// True once AABB has been computed and framing applied.
    framed: bool,
    /// Counter for frames since scene was loaded.
    frames_since_load: u32,
}

pub struct PrefabPreviewPlugin;

impl Plugin for PrefabPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, setup_prefab_preview)
            .add_systems(
                Update,
                (
                    register_prefab_preview_texture,
                    sync_prefab_preview_scene,
                    propagate_prefab_preview_layers,
                    frame_prefab_preview_camera,
                    rotate_prefab_preview,
                ),
            );
    }
}

/// Create the prefab preview scene: camera, lights, and root entity on a dedicated render layer.
fn setup_prefab_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let config = PreviewSceneConfig::object_preview(PREFAB_PREVIEW_RENDER_LAYER, -6);
    let layer = RenderLayers::layer(PREFAB_PREVIEW_RENDER_LAYER);
    let handles =
        spawn_preview_scene(&mut commands, &mut images, &mut meshes, &mut materials, &config);

    commands.entity(handles.camera).insert(PrefabPreviewCamera);
    for &light in &handles.lights {
        commands.entity(light).insert(PrefabPreviewLight);
    }

    // Empty root entity — prefab scenes get loaded as children of this
    commands.spawn((
        PrefabPreviewRoot,
        Transform::IDENTITY,
        Visibility::Inherited,
        layer,
    ));

    commands.insert_resource(PrefabPreviewState {
        texture: PreviewTexture {
            render_texture: handles.render_texture,
            egui_texture_id: None,
        },
        current_path: None,
        last_loaded_path: None,
        rotation_angle: 0.0,
        fit_scale: Vec3::ONE,
        fit_translation: Vec3::ZERO,
        framed: false,
        frames_since_load: 0,
    });
}

/// Register the render texture with bevy_egui once.
fn register_prefab_preview_texture(
    mut state: ResMut<PrefabPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    register_preview_egui_texture(&mut state.texture, &mut user_textures);
}

/// Swap the prefab preview scene when the highlighted prefab changes.
/// Uses exclusive world access to deserialize the RON scene and regenerate
/// runtime components (meshes, materials, colliders).
fn sync_prefab_preview_scene(world: &mut World) {
    // Check if changed
    let changed = {
        let state = world.resource::<PrefabPreviewState>();
        state.current_path != state.last_loaded_path
    };
    if !changed {
        return;
    }

    // Update tracking
    let current_path = world.resource::<PrefabPreviewState>().current_path.clone();
    {
        let mut state = world.resource_mut::<PrefabPreviewState>();
        state.last_loaded_path = current_path.clone();
        state.framed = false;
        state.frames_since_load = 0;
        state.rotation_angle = 0.0;
        state.fit_scale = Vec3::ONE;
        state.fit_translation = Vec3::ZERO;
    }

    // Find the root entity and despawn existing children
    let root_entity = {
        let mut query = world.query_filtered::<Entity, With<PrefabPreviewRoot>>();
        query.iter(world).next()
    };
    let Some(root_entity) = root_entity else {
        return;
    };

    // Collect and despawn existing preview children
    let children_to_despawn: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<PrefabPreviewChild>>();
        query.iter(world).collect()
    };
    for child in children_to_despawn {
        world.despawn(child);
    }

    // Reset root transform
    if let Ok(mut entity_mut) = world.get_entity_mut(root_entity) {
        entity_mut.insert(Transform::IDENTITY);
    }

    // Load the scene if path is set
    let Some(path) = current_path else {
        return;
    };

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read prefab preview scene '{}': {}", path, e);
            return;
        }
    };

    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry_guard = type_registry.read();
    let scene_deserializer = SceneDeserializer {
        type_registry: &type_registry_guard,
    };

    let mut ron_deserializer = match ron::de::Deserializer::from_str(&content) {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to parse prefab preview scene '{}': {}", path, e);
            return;
        }
    };

    let scene: DynamicScene = match scene_deserializer.deserialize(&mut ron_deserializer) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to deserialize prefab preview scene: {:?}", e);
            return;
        }
    };

    drop(type_registry_guard);

    // Write scene to world
    let mut entity_map = bevy::ecs::entity::EntityHashMap::default();
    if let Err(e) = scene.write_to_world(world, &mut entity_map) {
        warn!("Failed to instantiate prefab preview scene: {:?}", e);
        return;
    }

    let loaded_entities: Vec<Entity> = entity_map.values().copied().collect();

    // Find root entities (those without parents that were also loaded)
    let root_entities: Vec<Entity> = loaded_entities
        .iter()
        .filter(|&&entity| {
            if let Ok(entity_ref) = world.get_entity(entity) {
                if let Some(child_of) = entity_ref.get::<ChildOf>() {
                    !loaded_entities.contains(&child_of.parent())
                } else {
                    true
                }
            } else {
                false
            }
        })
        .copied()
        .collect();

    let layer = RenderLayers::layer(PREFAB_PREVIEW_RENDER_LAYER);

    // Mark all loaded entities and assign render layer
    for &entity in &loaded_entities {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((PrefabPreviewChild, layer.clone()));
        }
    }

    // Parent root entities under the preview root
    for loaded_root in root_entities {
        if let Ok(mut parent_entity) = world.get_entity_mut(root_entity) {
            parent_entity.add_child(loaded_root);
        }
    }

    // Regenerate runtime components (meshes, materials, colliders)
    regenerate_runtime_components(world);
}

/// Walk all descendants of the prefab preview root and ensure they have the
/// correct render layer. Runtime regeneration may add new child entities
/// (e.g. mesh children) that need the layer.
fn propagate_prefab_preview_layers(
    state: Res<PrefabPreviewState>,
    root_query: Query<Entity, With<PrefabPreviewRoot>>,
    children_query: Query<&Children>,
    layer_query: Query<(), With<RenderLayers>>,
    mut commands: Commands,
) {
    if state.framed || state.current_path.is_none() {
        return;
    }

    let Ok(root_entity) = root_query.single() else {
        return;
    };

    let layer = RenderLayers::layer(PREFAB_PREVIEW_RENDER_LAYER);
    propagate_layers_recursive(root_entity, &children_query, &layer_query, &layer, &mut commands);
}

fn propagate_layers_recursive(
    entity: Entity,
    children_query: &Query<&Children>,
    layer_query: &Query<(), With<RenderLayers>>,
    layer: &RenderLayers,
    commands: &mut Commands,
) {
    if layer_query.get(entity).is_err() {
        commands.entity(entity).insert(layer.clone());
    }

    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            propagate_layers_recursive(child, children_query, layer_query, layer, commands);
        }
    }
}

/// Compute framing once the prefab scene has been regenerated and has valid AABBs.
fn frame_prefab_preview_camera(
    mut state: ResMut<PrefabPreviewState>,
    root_query: Query<Entity, With<PrefabPreviewRoot>>,
    children_query: Query<&Children>,
    aabb_query: Query<(&Aabb, &GlobalTransform)>,
    mut transform_query: Query<&mut Transform>,
) {
    if state.current_path.is_none() {
        return;
    }

    if state.framed && state.frames_since_load > 30 {
        return;
    }

    state.frames_since_load += 1;

    // Wait a few frames for regeneration + transform propagation
    if state.frames_since_load < 5 {
        return;
    }

    let Ok(root_entity) = root_query.single() else {
        return;
    };

    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut found_any = false;

    collect_aabbs(
        root_entity,
        &children_query,
        &aabb_query,
        &mut min,
        &mut max,
        &mut found_any,
    );

    if !found_any {
        if state.frames_since_load >= 120 {
            state.framed = true;
        }
        return;
    }

    let center = (min + max) * 0.5;
    let extents = max - min;
    let (scale, translation) = fit_transform_from_extents(center, extents);

    let scale_changed = (state.fit_scale - scale).length() > 0.01;
    if !state.framed || scale_changed {
        state.fit_scale = scale;
        state.fit_translation = translation;
        state.framed = true;

        if let Ok(mut transform) = transform_query.get_mut(root_entity) {
            transform.scale = state.fit_scale;
            transform.translation = state.fit_translation;
        }
    }
}

fn collect_aabbs(
    entity: Entity,
    children_query: &Query<&Children>,
    aabb_query: &Query<(&Aabb, &GlobalTransform)>,
    min: &mut Vec3,
    max: &mut Vec3,
    found_any: &mut bool,
) {
    if let Ok((aabb, global_transform)) = aabb_query.get(entity) {
        let center = Vec3::from(aabb.center);
        let half = Vec3::from(aabb.half_extents);

        let affine = global_transform.affine();
        for corner_sign in [
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(1.0, -1.0, -1.0),
            Vec3::new(-1.0, 1.0, -1.0),
            Vec3::new(1.0, 1.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
            Vec3::new(1.0, -1.0, 1.0),
            Vec3::new(-1.0, 1.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
        ] {
            let local_point = center + half * corner_sign;
            let world_point = affine.transform_point3(local_point);
            *min = min.min(world_point);
            *max = max.max(world_point);
        }
        *found_any = true;
    }

    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            collect_aabbs(child, children_query, aabb_query, min, max, found_any);
        }
    }
}

/// Slowly rotate the preview scene when framed.
fn rotate_prefab_preview(
    mut state: ResMut<PrefabPreviewState>,
    time: Res<Time>,
    mut root_query: Query<&mut Transform, With<PrefabPreviewRoot>>,
) {
    if !state.framed || state.current_path.is_none() {
        return;
    }

    state.rotation_angle += PREVIEW_ROTATION_SPEED * time.delta_secs();
    if let Ok(mut transform) = root_query.single_mut() {
        apply_preview_rotation(
            state.rotation_angle,
            &mut transform,
            state.fit_scale,
            state.fit_translation,
        );
    }
}
