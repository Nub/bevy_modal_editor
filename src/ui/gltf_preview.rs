//! 3D preview viewport for GLTF models in the asset browser, rendered to a
//! texture and displayed in the palette's side panel via egui.

use bevy::camera::visibility::RenderLayers;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::camera::primitives::Aabb;
use bevy::scene::SceneRoot;
use bevy_egui::EguiUserTextures;

use crate::ui::preview_common::{
    apply_preview_rotation, fit_transform_from_extents, register_preview_egui_texture,
    spawn_preview_scene, PreviewSceneConfig, PreviewTexture, PREVIEW_ROTATION_SPEED,
};

/// Render layer for the GLTF preview scene.
const GLTF_PREVIEW_RENDER_LAYER: usize = 27;

#[derive(Component)]
struct GltfPreviewCamera;

#[derive(Component)]
struct GltfPreviewLight;

#[derive(Component)]
struct GltfPreviewRoot;

/// State resource for the GLTF preview system.
#[derive(Resource)]
pub struct GltfPreviewState {
    /// Shared render texture + egui texture id.
    pub texture: PreviewTexture,
    /// Set by the asset browser to indicate what to preview.
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
    /// Counter for frames since model was spawned (async load delay).
    frames_since_spawn: u32,
}

pub struct GltfPreviewPlugin;

impl Plugin for GltfPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, setup_gltf_preview)
            .add_systems(
                Update,
                (
                    register_gltf_preview_texture,
                    sync_gltf_preview_model,
                    propagate_gltf_preview_layers,
                    frame_gltf_preview_camera,
                    rotate_gltf_preview,
                ),
            );
    }
}

/// Create the GLTF preview scene: camera, lights, and root entity on a dedicated render layer.
fn setup_gltf_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let config = PreviewSceneConfig::object_preview(GLTF_PREVIEW_RENDER_LAYER, -5);
    let layer = RenderLayers::layer(GLTF_PREVIEW_RENDER_LAYER);
    let handles = spawn_preview_scene(&mut commands, &mut images, &mut meshes, &mut materials, &config);

    // Tag spawned entities with local markers
    commands.entity(handles.camera).insert(GltfPreviewCamera);
    for &light in &handles.lights {
        commands.entity(light).insert(GltfPreviewLight);
    }

    // Empty root entity — GLTF scenes get spawned as children of this
    commands.spawn((
        GltfPreviewRoot,
        Transform::IDENTITY,
        Visibility::Inherited,
        layer,
    ));

    commands.insert_resource(GltfPreviewState {
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
        frames_since_spawn: 0,
    });
}

/// Register the render texture with bevy_egui once.
fn register_gltf_preview_texture(
    mut state: ResMut<GltfPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    register_preview_egui_texture(&mut state.texture, &mut user_textures);
}

/// Swap the GLTF model when the highlighted asset changes.
fn sync_gltf_preview_model(
    mut state: ResMut<GltfPreviewState>,
    root_query: Query<(Entity, Option<&Children>), With<GltfPreviewRoot>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    if state.current_path == state.last_loaded_path {
        return;
    }
    state.last_loaded_path = state.current_path.clone();

    let Ok((root_entity, children)) = root_query.single() else {
        return;
    };

    // Despawn all children of the root
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    // Reset framing state
    state.framed = false;
    state.frames_since_spawn = 0;
    state.rotation_angle = 0.0;
    state.fit_scale = Vec3::ONE;
    state.fit_translation = Vec3::ZERO;

    // Reset root transform
    commands.entity(root_entity).insert(Transform::IDENTITY);

    // Spawn new model if path is set
    if let Some(path) = state.current_path.clone() {
        let scene_handle = asset_server.load(GltfAssetLabel::Scene(0).from_asset(path));
        let layer = RenderLayers::layer(GLTF_PREVIEW_RENDER_LAYER);
        commands.entity(root_entity).with_child((
            SceneRoot(scene_handle),
            Transform::IDENTITY,
            Visibility::Inherited,
            layer,
        ));
    }
}

/// Walk all descendants of the GLTF preview root and ensure they have the
/// correct render layer. GLTF scenes spawn children asynchronously, so we
/// keep running this until framing is complete.
fn propagate_gltf_preview_layers(
    state: Res<GltfPreviewState>,
    root_query: Query<Entity, With<GltfPreviewRoot>>,
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

    let layer = RenderLayers::layer(GLTF_PREVIEW_RENDER_LAYER);
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

/// Compute framing once the GLTF scene has loaded and has valid AABBs.
/// Keeps re-checking for a while after initial framing so late-loading
/// meshes are captured (GLTF scenes load asynchronously and may take many
/// frames to fully instantiate).
fn frame_gltf_preview_camera(
    mut state: ResMut<GltfPreviewState>,
    root_query: Query<Entity, With<GltfPreviewRoot>>,
    children_query: Query<&Children>,
    aabb_query: Query<(&Aabb, &GlobalTransform)>,
    mut transform_query: Query<&mut Transform>,
) {
    if state.current_path.is_none() {
        return;
    }

    // Stop rechecking once framed and enough time has passed for all meshes
    if state.framed && state.frames_since_spawn > 30 {
        return;
    }

    state.frames_since_spawn += 1;

    // Wait a few frames for scene instantiation + transform propagation
    if state.frames_since_spawn < 5 {
        return;
    }

    let Ok(root_entity) = root_query.single() else {
        return;
    };

    // Collect world-space AABBs from all descendants
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
        // Give up after 120 frames — model may be broken
        if state.frames_since_spawn >= 120 {
            state.framed = true;
        }
        return;
    }

    // Compute uniform scale and centering via shared helper
    let center = (min + max) * 0.5;
    let extents = max - min;
    let (scale, translation) = fit_transform_from_extents(center, extents);

    // Update framing if this is first time, or if the AABB grew significantly
    // (more meshes may have loaded since last frame)
    let scale_changed = (state.fit_scale - scale).length() > 0.01;
    if !state.framed || scale_changed {
        state.fit_scale = scale;
        state.fit_translation = translation;
        state.framed = true;

        // Apply to root transform
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

        // Transform AABB corners to world space
        let gt: &GlobalTransform = global_transform;
        let affine = gt.affine();
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

/// Slowly rotate the preview model when framed, preserving the fit-to-frame
/// scale and translation.
fn rotate_gltf_preview(
    mut state: ResMut<GltfPreviewState>,
    time: Res<Time>,
    mut root_query: Query<&mut Transform, With<GltfPreviewRoot>>,
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
