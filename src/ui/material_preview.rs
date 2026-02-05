//! 3D material preview viewport rendered to a texture and displayed in the
//! material editor panel via egui.

use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::Skybox;
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

// ---------------------------------------------------------------------------
// Preview settings (mesh shape + lighting preset)
// ---------------------------------------------------------------------------

/// Mesh shapes available for the material preview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewMeshShape {
    #[default]
    Sphere,
    Cube,
    Cylinder,
    Plane,
    Torus,
}

impl PreviewMeshShape {
    pub const ALL: &[Self] = &[
        Self::Sphere,
        Self::Cube,
        Self::Cylinder,
        Self::Plane,
        Self::Torus,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "Sphere",
            Self::Cube => "Cube",
            Self::Cylinder => "Cylinder",
            Self::Plane => "Plane",
            Self::Torus => "Torus",
        }
    }

    /// Create a mesh for this shape with generated tangents for normal mapping.
    pub fn create_mesh(self) -> Mesh {
        let mesh: Mesh = match self {
            Self::Sphere => Sphere::new(0.7).mesh().ico(5).unwrap(),
            Self::Cube => Cuboid::new(1.1, 1.1, 1.1).mesh().into(),
            Self::Cylinder => Cylinder::new(0.5, 1.2).mesh().resolution(32).into(),
            Self::Plane => Plane3d::new(Vec3::Y, Vec2::splat(0.9)).mesh().into(),
            Self::Torus => Torus::new(0.35, 0.7).mesh().minor_resolution(24).major_resolution(48).into(),
        };
        mesh.with_generated_tangents()
            .expect("preview mesh should support tangent generation")
    }
}

/// Lighting presets for the material preview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewLighting {
    #[default]
    Studio,
    Outdoor,
    Orbiting,
}

impl PreviewLighting {
    pub const ALL: &[Self] = &[Self::Studio, Self::Outdoor, Self::Orbiting];

    pub fn label(self) -> &'static str {
        match self {
            Self::Studio => "Studio",
            Self::Outdoor => "Outdoor",
            Self::Orbiting => "Orbiting",
        }
    }
}

/// Resource controlling the preview mesh shape and lighting preset.
#[derive(Resource)]
pub struct PreviewSettings {
    pub mesh_shape: PreviewMeshShape,
    pub lighting: PreviewLighting,
    pub dirty: bool,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            mesh_shape: PreviewMeshShape::Sphere,
            lighting: PreviewLighting::Studio,
            dirty: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Marker components
// ---------------------------------------------------------------------------

/// Marker for the material editor preview camera entity.
#[derive(Component)]
struct PreviewCamera;

/// Marker for the material editor preview sphere entity.
#[derive(Component)]
pub(crate) struct PreviewSphere;

/// Marker for the material editor preview light entity.
#[derive(Component)]
struct PreviewLight;

/// Marker for the material editor preview IBL probe entity.
#[derive(Component)]
struct PreviewProbe;

/// Marker for the preset palette preview camera entity.
#[derive(Component)]
struct PresetPreviewCamera;

/// Marker for the preset palette preview sphere entity.
#[derive(Component)]
pub(crate) struct PresetPreviewSphere;

/// Marker for the preset palette preview light entity.
#[derive(Component)]
struct PresetPreviewLight;

/// Marker for the preset palette preview IBL probe entity.
#[derive(Component)]
struct PresetPreviewProbe;

/// Marker for orbiting point lights in the main preview scene.
#[derive(Component)]
struct PreviewOrbitLight {
    /// Index of this light (used to offset its orbit phase).
    index: u32,
}

/// Marker for orbiting point lights in the preset preview scene.
#[derive(Component)]
struct PresetPreviewOrbitLight {
    index: u32,
}

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
        app.init_resource::<PreviewSettings>()
            .add_systems(PreStartup, (setup_material_preview, setup_preset_preview))
            .add_systems(
                Update,
                (
                    register_preview_texture_system,
                    sync_preview_material,
                    rotate_preview_sphere,
                    register_preset_preview_texture_system,
                    sync_preset_preview_material,
                    rotate_preset_preview_sphere,
                    apply_preview_settings,
                    animate_orbit_lights,
                ),
            );
    }
}

/// Create the preview scene: camera, sphere, and light on a dedicated render layer.
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

    // Preview sphere
    let sphere_mesh = meshes.add(PreviewMeshShape::Sphere.create_mesh());
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

/// Create the preset preview scene: camera, sphere, and light.
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

    // Preview sphere
    let sphere_mesh = meshes.add(PreviewMeshShape::Sphere.create_mesh());
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

// ---------------------------------------------------------------------------
// Apply preview settings (mesh shape + lighting changes)
// ---------------------------------------------------------------------------

/// When `PreviewSettings.dirty` is set, swap the preview mesh and/or lighting
/// on both the main and preset preview scenes.
fn apply_preview_settings(
    mut settings: ResMut<PreviewSettings>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    main_sphere: Query<Entity, With<PreviewSphere>>,
    preset_sphere: Query<Entity, With<PresetPreviewSphere>>,
    main_camera: Query<Entity, With<PreviewCamera>>,
    preset_camera: Query<Entity, With<PresetPreviewCamera>>,
    main_lights: Query<Entity, With<PreviewLight>>,
    preset_lights: Query<Entity, With<PresetPreviewLight>>,
    main_probe: Query<Entity, With<PreviewProbe>>,
    preset_probe: Query<Entity, With<PresetPreviewProbe>>,
    main_orbit: Query<Entity, With<PreviewOrbitLight>>,
    preset_orbit: Query<Entity, With<PresetPreviewOrbitLight>>,
) {
    if !settings.dirty {
        return;
    }
    settings.dirty = false;

    let new_mesh = meshes.add(settings.mesh_shape.create_mesh());

    // Swap mesh on both preview spheres
    for entity in main_sphere.iter().chain(preset_sphere.iter()) {
        commands.entity(entity).insert(Mesh3d(new_mesh.clone()));
    }

    // Cleanup helpers shared by all presets
    let cleanup_skybox = |commands: &mut Commands, main_camera: &Query<Entity, With<PreviewCamera>>, preset_camera: &Query<Entity, With<PresetPreviewCamera>>| {
        for entity in main_camera.iter().chain(preset_camera.iter()) {
            commands
                .entity(entity)
                .remove::<Skybox>()
                .remove::<EnvironmentMapLight>();
        }
    };
    let cleanup_probes = |commands: &mut Commands, main_probe: &Query<Entity, With<PreviewProbe>>, preset_probe: &Query<Entity, With<PresetPreviewProbe>>| {
        for entity in main_probe.iter().chain(preset_probe.iter()) {
            commands.entity(entity).despawn();
        }
    };
    let cleanup_orbit_lights = |commands: &mut Commands, main_orbit: &Query<Entity, With<PreviewOrbitLight>>, preset_orbit: &Query<Entity, With<PresetPreviewOrbitLight>>| {
        for entity in main_orbit.iter().chain(preset_orbit.iter()) {
            commands.entity(entity).despawn();
        }
    };

    // Apply lighting changes
    match settings.lighting {
        PreviewLighting::Studio => {
            cleanup_skybox(&mut commands, &main_camera, &preset_camera);
            cleanup_probes(&mut commands, &main_probe, &preset_probe);
            cleanup_orbit_lights(&mut commands, &main_orbit, &preset_orbit);

            // Set directional lights to 5000 lux
            for entity in main_lights.iter().chain(preset_lights.iter()) {
                commands.entity(entity).insert(DirectionalLight {
                    illuminance: 5000.0,
                    shadows_enabled: false,
                    ..default()
                });
            }
        }
        PreviewLighting::Outdoor => {
            cleanup_orbit_lights(&mut commands, &main_orbit, &preset_orbit);
            cleanup_probes(&mut commands, &main_probe, &preset_probe);

            let cubemap = asset_server.load("skybox/citrus_orchard_road_puresky_4k_cubemap.ktx2");
            let diffuse = asset_server.load("skybox/citrus_orchard_road_puresky_4k_diffuse.ktx2");
            let specular =
                asset_server.load("skybox/citrus_orchard_road_puresky_4k_specular.ktx2");

            // Add skybox + environment map to cameras
            for entity in main_camera.iter().chain(preset_camera.iter()) {
                commands.entity(entity).insert((
                    Skybox {
                        image: cubemap.clone(),
                        brightness: 1000.0,
                        rotation: Quat::IDENTITY,
                    },
                    EnvironmentMapLight {
                        diffuse_map: diffuse.clone(),
                        specular_map: specular.clone(),
                        intensity: 900.0,
                        rotation: Quat::IDENTITY,
                        affects_lightmapped_mesh_diffuse: true,
                    },
                ));
            }

            // Reduce directional to 1500 lux fill
            for entity in main_lights.iter().chain(preset_lights.iter()) {
                commands.entity(entity).insert(DirectionalLight {
                    illuminance: 1500.0,
                    shadows_enabled: false,
                    ..default()
                });
            }
        }
        PreviewLighting::Orbiting => {
            cleanup_skybox(&mut commands, &main_camera, &preset_camera);
            cleanup_probes(&mut commands, &main_probe, &preset_probe);
            cleanup_orbit_lights(&mut commands, &main_orbit, &preset_orbit);

            // Dim directional to low ambient fill
            for entity in main_lights.iter().chain(preset_lights.iter()) {
                commands.entity(entity).insert(DirectionalLight {
                    illuminance: 800.0,
                    shadows_enabled: false,
                    ..default()
                });
            }

            // Spawn 3 colored orbiting point lights per scene
            let orbit_colors = [
                Color::srgb(1.0, 0.4, 0.3),  // warm red-orange
                Color::srgb(0.3, 0.6, 1.0),  // cool blue
                Color::srgb(0.4, 1.0, 0.5),  // green
            ];

            let main_layer = RenderLayers::layer(PREVIEW_RENDER_LAYER);
            let preset_layer = RenderLayers::layer(PRESET_PREVIEW_RENDER_LAYER);

            for (i, &color) in orbit_colors.iter().enumerate() {
                let angle = (i as f32) * std::f32::consts::TAU / orbit_colors.len() as f32;
                let pos = Vec3::new(angle.cos() * ORBIT_RADIUS, ORBIT_HEIGHT, angle.sin() * ORBIT_RADIUS);

                commands.spawn((
                    PreviewOrbitLight { index: i as u32 },
                    PointLight {
                        color,
                        intensity: ORBIT_INTENSITY,
                        range: 8.0,
                        shadows_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(pos),
                    main_layer.clone(),
                ));

                commands.spawn((
                    PresetPreviewOrbitLight { index: i as u32 },
                    PointLight {
                        color,
                        intensity: ORBIT_INTENSITY,
                        range: 8.0,
                        shadows_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(pos),
                    preset_layer.clone(),
                ));
            }
        }
    }
}

/// Orbit radius, height, speed, and intensity for the orbiting lights preset.
const ORBIT_RADIUS: f32 = 1.8;
const ORBIT_HEIGHT: f32 = 0.6;
const ORBIT_SPEED: f32 = 0.7;
const ORBIT_INTENSITY: f32 = 80_000.0;
const ORBIT_LIGHT_COUNT: f32 = 3.0;

/// Animate orbiting point lights around the preview object.
fn animate_orbit_lights(
    time: Res<Time>,
    mode: Res<State<EditorMode>>,
    settings: Res<PreviewSettings>,
    mut main_lights: Query<(&PreviewOrbitLight, &mut Transform), Without<PresetPreviewOrbitLight>>,
    mut preset_lights: Query<(&PresetPreviewOrbitLight, &mut Transform), Without<PreviewOrbitLight>>,
) {
    if *mode.get() != EditorMode::Material || settings.lighting != PreviewLighting::Orbiting {
        return;
    }

    let t = time.elapsed_secs() * ORBIT_SPEED;

    for (orbit, mut transform) in main_lights.iter_mut() {
        let phase = (orbit.index as f32) * std::f32::consts::TAU / ORBIT_LIGHT_COUNT;
        let angle = t + phase;
        transform.translation = Vec3::new(
            angle.cos() * ORBIT_RADIUS,
            ORBIT_HEIGHT,
            angle.sin() * ORBIT_RADIUS,
        );
    }

    for (orbit, mut transform) in preset_lights.iter_mut() {
        let phase = (orbit.index as f32) * std::f32::consts::TAU / ORBIT_LIGHT_COUNT;
        let angle = t + phase;
        transform.translation = Vec3::new(
            angle.cos() * ORBIT_RADIUS,
            ORBIT_HEIGHT,
            angle.sin() * ORBIT_RADIUS,
        );
    }
}
