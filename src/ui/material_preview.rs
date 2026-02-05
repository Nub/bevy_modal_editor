//! 3D material preview viewport rendered to a texture and displayed in the
//! material editor panel via egui.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy_egui::{egui, EguiTextureHandle, EguiUserTextures};
use bevy_editor_game::{MaterialDefinition, MaterialLibrary, MaterialRef};

use crate::editor::{EditorMode, EditorState};
use crate::materials::{
    apply_material_def_standalone, remove_all_material_components, resolve_material_ref,
};
use crate::selection::Selected;
use crate::ui::material_editor::EditingPreset;

/// Render layer for the material editor preview scene (outliner uses layer 31).
const PREVIEW_RENDER_LAYER: usize = 30;

/// Render layer for the preset palette preview scene.
const PRESET_PREVIEW_RENDER_LAYER: usize = 29;

/// Resolution of the preview render texture.
const PREVIEW_TEXTURE_SIZE: u32 = 512;

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
    /// Handle to the render texture.
    pub render_texture: Handle<Image>,
    /// Egui texture id once registered.
    pub egui_texture_id: Option<egui::TextureId>,
    /// Whether the preview scene has been set up.
    pub initialized: bool,
    /// Serialized form of last applied material for change detection.
    last_applied_hash: Option<String>,
    /// Current rotation angle of the sphere.
    rotation_angle: f32,
    // NOTE: No preview_override — the preset palette uses its own PresetPreviewState.
}

/// State resource for the preset palette preview (separate scene).
#[derive(Resource)]
pub struct PresetPreviewState {
    /// Handle to the render texture.
    pub render_texture: Handle<Image>,
    /// Egui texture id once registered.
    pub egui_texture_id: Option<egui::TextureId>,
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
                    register_preview_texture,
                    sync_preview_material,
                    rotate_preview_sphere,
                    register_preset_preview_texture,
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
    // Create render texture
    let size = Extent3d {
        width: PREVIEW_TEXTURE_SIZE,
        height: PREVIEW_TEXTURE_SIZE,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
    let render_texture = images.add(image);

    let layer = RenderLayers::layer(PREVIEW_RENDER_LAYER);

    // Preview camera
    commands.spawn((
        PreviewCamera,
        Camera3d::default(),
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.12, 0.14, 0.18)),
            ..default()
        },
        RenderTarget::Image(render_texture.clone().into()),
        Transform::from_xyz(0.0, 0.5, 2.5).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
        layer.clone(),
    ));

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
        layer.clone(),
    ));

    // Ground plane
    let ground_mesh = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(3.0)));
    let ground_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.17),
        ..default()
    });
    commands.spawn((
        PreviewGround,
        Mesh3d(ground_mesh),
        MeshMaterial3d(ground_material),
        Transform::from_xyz(0.0, -0.7, 0.0),
        layer.clone(),
    ));

    // Directional light
    commands.spawn((
        PreviewLight,
        DirectionalLight {
            illuminance: 3000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
        layer,
    ));

    commands.insert_resource(MaterialPreviewState {
        render_texture,
        egui_texture_id: None,
        initialized: true,
        last_applied_hash: None,
        rotation_angle: 0.0,
    });
}

/// Register the render texture with bevy_egui once (runs until successful).
fn register_preview_texture(
    mut state: ResMut<MaterialPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    if state.egui_texture_id.is_some() {
        return;
    }
    let id = user_textures.add_image(EguiTextureHandle::Strong(state.render_texture.clone()));
    state.egui_texture_id = Some(id);
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

    state.rotation_angle += 0.3 * time.delta_secs();
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
    let size = Extent3d {
        width: PREVIEW_TEXTURE_SIZE,
        height: PREVIEW_TEXTURE_SIZE,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
    let render_texture = images.add(image);

    let layer = RenderLayers::layer(PRESET_PREVIEW_RENDER_LAYER);

    commands.spawn((
        PresetPreviewCamera,
        Camera3d::default(),
        Camera {
            order: -3,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.12, 0.14, 0.18)),
            ..default()
        },
        RenderTarget::Image(render_texture.clone().into()),
        Transform::from_xyz(0.0, 0.5, 2.5).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
        layer.clone(),
    ));

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
        layer.clone(),
    ));

    let ground_mesh = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(3.0)));
    let ground_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.17),
        ..default()
    });
    commands.spawn((
        PresetPreviewGround,
        Mesh3d(ground_mesh),
        MeshMaterial3d(ground_material),
        Transform::from_xyz(0.0, -0.7, 0.0),
        layer.clone(),
    ));

    commands.spawn((
        PresetPreviewLight,
        DirectionalLight {
            illuminance: 3000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
        layer,
    ));

    commands.insert_resource(PresetPreviewState {
        render_texture,
        egui_texture_id: None,
        last_applied_hash: None,
        rotation_angle: 0.0,
        current_def: None,
    });
}

/// Register the preset preview render texture with bevy_egui.
fn register_preset_preview_texture(
    mut state: ResMut<PresetPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    if state.egui_texture_id.is_some() {
        return;
    }
    let id = user_textures.add_image(EguiTextureHandle::Strong(state.render_texture.clone()));
    state.egui_texture_id = Some(id);
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

    state.rotation_angle += 0.3 * time.delta_secs();
    if let Ok(mut transform) = sphere.single_mut() {
        transform.rotation = Quat::from_rotation_y(state.rotation_angle);
    }
}
