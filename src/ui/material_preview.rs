//! 3D material preview viewport rendered to a texture and displayed in the
//! material editor panel via egui.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_egui::EguiUserTextures;
use bevy_editor_game::{MaterialDefinition, MaterialLibrary, MaterialRef};

use crate::editor::{EditorMode, EditorState};
use crate::materials::{
    apply_material_def_standalone, remove_all_material_components, resolve_material_ref,
};
use crate::selection::Selected;
use crate::ui::material_editor::EditingPreset;
use crate::ui::preview_common::{
    register_preview_egui_texture, spawn_preview_scene, PreviewSceneConfig, PreviewTexture,
    PREVIEW_ROTATION_SPEED,
};

/// Render layer for the material editor preview scene (outliner uses layer 31).
const PREVIEW_RENDER_LAYER: usize = 30;

/// Render layer for the preset palette preview scene.
const PRESET_PREVIEW_RENDER_LAYER: usize = 29;

/// Marker for the material editor preview camera entity.
#[derive(Component)]
struct PreviewCamera;

/// Marker for the material editor preview sphere entity.
#[derive(Component)]
struct PreviewSphere;

/// Marker for the material editor preview ground plane entity.
#[derive(Component)]
struct PreviewGround;

/// Marker for the material editor preview light entity.
#[derive(Component)]
struct PreviewLight;

/// Marker for the preset palette preview camera entity.
#[derive(Component)]
struct PresetPreviewCamera;

/// Marker for the preset palette preview sphere entity.
#[derive(Component)]
struct PresetPreviewSphere;

/// Marker for the preset palette preview ground entity.
#[derive(Component)]
struct PresetPreviewGround;

/// Marker for the preset palette preview light entity.
#[derive(Component)]
struct PresetPreviewLight;

/// State resource for the material preview system.
#[derive(Resource)]
pub struct MaterialPreviewState {
    /// Shared render texture + egui texture id.
    pub texture: PreviewTexture,
    /// Whether the preview scene has been set up.
    pub initialized: bool,
    /// Serialized form of last applied material for change detection.
    last_applied_hash: Option<String>,
    /// Current rotation angle of the sphere.
    rotation_angle: f32,
}

/// State resource for the preset palette preview (separate scene).
#[derive(Resource)]
pub struct PresetPreviewState {
    /// Shared render texture + egui texture id.
    pub texture: PreviewTexture,
    /// Serialized form of last applied material for change detection.
    last_applied_hash: Option<String>,
    /// Current rotation angle of the sphere.
    rotation_angle: f32,
    /// The material definition to preview. Set by the preset palette.
    pub current_def: Option<MaterialDefinition>,
}

pub struct MaterialPreviewPlugin;

impl Plugin for MaterialPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, (setup_material_preview, setup_preset_preview))
            .add_systems(
                Update,
                (
                    register_preview_texture_system,
                    sync_preview_material,
                    rotate_preview_sphere,
                    register_preset_preview_texture_system,
                    sync_preset_preview_material,
                    rotate_preset_preview_sphere,
                ),
            );
    }
}

/// Create the preview scene: camera, sphere, ground, and light on a dedicated render layer.
fn setup_material_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let config = PreviewSceneConfig::material_studio(PREVIEW_RENDER_LAYER, -2);
    let layer = RenderLayers::layer(PREVIEW_RENDER_LAYER);
    let handles = spawn_preview_scene(&mut commands, &mut images, &mut meshes, &mut materials, &config);

    // Tag spawned entities with local markers
    commands.entity(handles.camera).insert(PreviewCamera);
    for &light in &handles.lights {
        commands.entity(light).insert(PreviewLight);
    }
    if let Some(ground) = handles.ground {
        commands.entity(ground).insert(PreviewGround);
    }

    // Preview sphere
    let sphere_mesh = meshes.add(Sphere::new(0.7).mesh().ico(5).unwrap());
    let default_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        ..default()
    });
    commands.spawn((
        PreviewSphere,
        Mesh3d(sphere_mesh),
        MeshMaterial3d(default_material),
        Transform::IDENTITY,
        layer,
    ));

    commands.insert_resource(MaterialPreviewState {
        texture: PreviewTexture {
            render_texture: handles.render_texture,
            egui_texture_id: None,
        },
        initialized: true,
        last_applied_hash: None,
        rotation_angle: 0.0,
    });
}

/// Register the render texture with bevy_egui once (runs until successful).
fn register_preview_texture_system(
    mut state: ResMut<MaterialPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    register_preview_egui_texture(&mut state.texture, &mut user_textures);
}

/// Keep the preview sphere's material in sync with the selected entity's material.
fn sync_preview_material(
    mut state: ResMut<MaterialPreviewState>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
    selected: Query<&MaterialRef, With<Selected>>,
    library: Res<MaterialLibrary>,
    editing_preset: Res<EditingPreset>,
    sphere: Query<Entity, With<PreviewSphere>>,
    mut world_commands: Commands,
) {
    // Only sync in Material mode with UI enabled
    if !editor_state.ui_enabled || *mode.get() != EditorMode::Material {
        return;
    }

    let Ok(sphere_entity) = sphere.single() else {
        return;
    };

    // Resolve from selected entity, or from EditingPreset if no entity selected
    let current_def = selected
        .iter()
        .next()
        .and_then(|mat_ref| resolve_material_ref(mat_ref, &library))
        .cloned()
        .or_else(|| {
            editing_preset
                .0
                .as_ref()
                .and_then(|name| library.materials.get(name))
                .cloned()
        });

    // Hash via RON serialization for change detection
    let current_hash = current_def
        .as_ref()
        .and_then(|d| ron::to_string(d).ok());

    // Skip if unchanged
    if state.last_applied_hash == current_hash {
        return;
    }
    state.last_applied_hash = current_hash;

    // We need exclusive world access to apply materials, so defer via command
    let def_for_command = current_def;
    world_commands.queue(move |world: &mut World| {
        remove_all_material_components(world, sphere_entity);
        if let Some(def) = def_for_command {
            apply_material_def_standalone(world, sphere_entity, &def);
        } else {
            // Default gray material
            let handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial {
                    base_color: Color::srgb(0.5, 0.5, 0.5),
                    ..default()
                });
            if let Ok(mut e) = world.get_entity_mut(sphere_entity) {
                e.insert(MeshMaterial3d(handle));
            }
        }
    });
}

/// Slowly rotate the preview sphere when in Material mode.
fn rotate_preview_sphere(
    mut state: ResMut<MaterialPreviewState>,
    time: Res<Time>,
    mode: Res<State<EditorMode>>,
    mut sphere: Query<&mut Transform, With<PreviewSphere>>,
) {
    if *mode.get() != EditorMode::Material {
        return;
    }

    state.rotation_angle += PREVIEW_ROTATION_SPEED * time.delta_secs();
    if let Ok(mut transform) = sphere.single_mut() {
        transform.rotation = Quat::from_rotation_y(state.rotation_angle);
    }
}

// ---------------------------------------------------------------------------
// Preset palette preview (separate scene on its own render layer)
// ---------------------------------------------------------------------------

/// Create the preset preview scene: camera, sphere, ground, and light.
fn setup_preset_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let config = PreviewSceneConfig::material_studio(PRESET_PREVIEW_RENDER_LAYER, -3);
    let layer = RenderLayers::layer(PRESET_PREVIEW_RENDER_LAYER);
    let handles = spawn_preview_scene(&mut commands, &mut images, &mut meshes, &mut materials, &config);

    // Tag spawned entities with local markers
    commands.entity(handles.camera).insert(PresetPreviewCamera);
    for &light in &handles.lights {
        commands.entity(light).insert(PresetPreviewLight);
    }
    if let Some(ground) = handles.ground {
        commands.entity(ground).insert(PresetPreviewGround);
    }

    // Preview sphere
    let sphere_mesh = meshes.add(Sphere::new(0.7).mesh().ico(5).unwrap());
    let default_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        ..default()
    });
    commands.spawn((
        PresetPreviewSphere,
        Mesh3d(sphere_mesh),
        MeshMaterial3d(default_material),
        Transform::IDENTITY,
        layer,
    ));

    commands.insert_resource(PresetPreviewState {
        texture: PreviewTexture {
            render_texture: handles.render_texture,
            egui_texture_id: None,
        },
        last_applied_hash: None,
        rotation_angle: 0.0,
        current_def: None,
    });
}

/// Register the preset preview render texture with bevy_egui.
fn register_preset_preview_texture_system(
    mut state: ResMut<PresetPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    register_preview_egui_texture(&mut state.texture, &mut user_textures);
}

/// Keep the preset preview sphere in sync with the palette's current selection.
fn sync_preset_preview_material(
    mut state: ResMut<PresetPreviewState>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
    sphere: Query<Entity, With<PresetPreviewSphere>>,
    mut world_commands: Commands,
) {
    if !editor_state.ui_enabled || *mode.get() != EditorMode::Material {
        return;
    }

    let Ok(sphere_entity) = sphere.single() else {
        return;
    };

    let current_def = state.current_def.clone();

    let current_hash = current_def
        .as_ref()
        .and_then(|d| ron::to_string(d).ok());

    if state.last_applied_hash == current_hash {
        return;
    }
    state.last_applied_hash = current_hash;

    world_commands.queue(move |world: &mut World| {
        remove_all_material_components(world, sphere_entity);
        if let Some(def) = current_def {
            apply_material_def_standalone(world, sphere_entity, &def);
        } else {
            let handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial {
                    base_color: Color::srgb(0.5, 0.5, 0.5),
                    ..default()
                });
            if let Ok(mut e) = world.get_entity_mut(sphere_entity) {
                e.insert(MeshMaterial3d(handle));
            }
        }
    });
}

/// Slowly rotate the preset preview sphere when in Material mode.
fn rotate_preset_preview_sphere(
    mut state: ResMut<PresetPreviewState>,
    time: Res<Time>,
    mode: Res<State<EditorMode>>,
    mut sphere: Query<&mut Transform, With<PresetPreviewSphere>>,
) {
    if *mode.get() != EditorMode::Material {
        return;
    }

    state.rotation_angle += PREVIEW_ROTATION_SPEED * time.delta_secs();
    if let Ok(mut transform) = sphere.single_mut() {
        transform.rotation = Quat::from_rotation_y(state.rotation_angle);
    }
}
