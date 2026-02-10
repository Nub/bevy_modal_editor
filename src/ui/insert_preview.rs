//! 3D preview viewport for the Insert Object palette, rendered to a texture
//! and displayed in the palette's side panel via egui.

use bevy::prelude::*;
use bevy_egui::EguiUserTextures;

use crate::scene::blockout::{
    blockout_colors, generate_arch_mesh, generate_lshape_mesh, generate_ramp_mesh,
    generate_stairs_mesh, ArchMarker, LShapeMarker, RampMarker, StairsMarker,
};
use crate::scene::PrimitiveShape;
use crate::ui::command_palette::{CommandPaletteState, PaletteMode};
use crate::ui::preview_common::{
    apply_preview_rotation, fit_transform_from_mesh, register_preview_egui_texture,
    spawn_preview_scene, PreviewSceneConfig, PreviewTexture, PREVIEW_ROTATION_SPEED,
};

/// Render layer for the insert preview scene.
const INSERT_PREVIEW_RENDER_LAYER: usize = 28;

#[derive(Component)]
struct InsertPreviewCamera;

#[derive(Component)]
struct InsertPreviewMesh;

#[derive(Component)]
struct InsertPreviewLight;

/// What kind of object to show in the insert preview.
#[derive(Clone, PartialEq)]
pub enum InsertPreviewKind {
    Primitive(PrimitiveShape),
    PointLight,
    DirectionalLight,
    Group,
    Spline,
    FogVolume,
    Stairs,
    Ramp,
    Arch,
    LShape,
    Decal,
}

/// State resource for the insert preview system.
#[derive(Resource)]
pub struct InsertPreviewState {
    /// Shared render texture + egui texture id.
    pub texture: PreviewTexture,
    /// Current rotation angle.
    rotation_angle: f32,
    /// Set by the palette to indicate what to preview.
    pub current_kind: Option<InsertPreviewKind>,
    /// Change detection — tracks last applied kind.
    last_applied_kind: Option<InsertPreviewKind>,
    /// Computed fit-to-frame scale (preserved across rotation updates).
    fit_scale: Vec3,
    /// Computed fit-to-frame translation (preserved across rotation updates).
    fit_translation: Vec3,
}

pub struct InsertPreviewPlugin;

impl Plugin for InsertPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, setup_insert_preview)
            .add_systems(
                Update,
                (
                    register_insert_preview_texture,
                    sync_insert_preview_mesh,
                    rotate_insert_preview,
                ),
            );
    }
}

/// Create the insert preview scene: camera, mesh, and light on a dedicated render layer.
fn setup_insert_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let config = PreviewSceneConfig::object_preview(INSERT_PREVIEW_RENDER_LAYER, -4);
    let layer = bevy::camera::visibility::RenderLayers::layer(INSERT_PREVIEW_RENDER_LAYER);
    let handles = spawn_preview_scene(&mut commands, &mut images, &mut meshes, &mut materials, &config);

    // Tag spawned entities with local markers
    commands.entity(handles.camera).insert(InsertPreviewCamera);
    for &light in &handles.lights {
        commands.entity(light).insert(InsertPreviewLight);
    }

    // Default mesh (unit cube)
    let default_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let default_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        ..default()
    });
    commands.spawn((
        InsertPreviewMesh,
        Mesh3d(default_mesh),
        MeshMaterial3d(default_material),
        Transform::IDENTITY,
        layer,
    ));

    commands.insert_resource(InsertPreviewState {
        texture: PreviewTexture {
            render_texture: handles.render_texture,
            egui_texture_id: None,
        },
        rotation_angle: 0.0,
        current_kind: None,
        last_applied_kind: None,
        fit_scale: Vec3::ONE,
        fit_translation: Vec3::ZERO,
    });
}

/// Register the render texture with bevy_egui once.
fn register_insert_preview_texture(
    mut state: ResMut<InsertPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    register_preview_egui_texture(&mut state.texture, &mut user_textures);
}

/// Update the preview mesh when the highlighted insert item changes.
fn sync_insert_preview_mesh(
    mut state: ResMut<InsertPreviewState>,
    palette_state: Res<CommandPaletteState>,
    mesh_entity: Query<Entity, With<InsertPreviewMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    // Only run when insert palette is open
    if !palette_state.open || palette_state.mode != PaletteMode::Insert {
        return;
    }

    // Skip if unchanged
    if state.current_kind == state.last_applied_kind {
        return;
    }
    state.last_applied_kind = state.current_kind.clone();

    let Ok(entity) = mesh_entity.single() else {
        return;
    };

    let Some(kind) = &state.current_kind else {
        return;
    };

    // Build mesh + material for every kind
    let (mesh, mat) = match kind {
        InsertPreviewKind::Primitive(shape) => {
            let mesh = shape.create_mesh();
            let mat = materials.add(StandardMaterial {
                base_color: shape.default_color(),
                ..default()
            });
            (mesh, mat)
        }
        InsertPreviewKind::Stairs => {
            let mesh = generate_stairs_mesh(&StairsMarker::default());
            let mat = materials.add(StandardMaterial { base_color: blockout_colors::STAIRS, ..default() });
            (mesh, mat)
        }
        InsertPreviewKind::Ramp => {
            let mesh = generate_ramp_mesh(&RampMarker::default());
            let mat = materials.add(StandardMaterial { base_color: blockout_colors::RAMP, ..default() });
            (mesh, mat)
        }
        InsertPreviewKind::Arch => {
            let mesh = generate_arch_mesh(&ArchMarker::default());
            let mat = materials.add(StandardMaterial { base_color: blockout_colors::ARCH, ..default() });
            (mesh, mat)
        }
        InsertPreviewKind::LShape => {
            let mesh = generate_lshape_mesh(&LShapeMarker::default());
            let mat = materials.add(StandardMaterial { base_color: blockout_colors::LSHAPE, ..default() });
            (mesh, mat)
        }
        InsertPreviewKind::PointLight => {
            let mesh: Mesh = Sphere::new(0.3).into();
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.9, 0.6),
                emissive: LinearRgba::new(4.0, 3.6, 2.4, 1.0),
                ..default()
            });
            (mesh, mat)
        }
        InsertPreviewKind::DirectionalLight => {
            let mesh: Mesh = Sphere::new(0.5).into();
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.95, 0.8),
                emissive: LinearRgba::new(4.0, 3.8, 3.2, 1.0),
                ..default()
            });
            (mesh, mat)
        }
        InsertPreviewKind::Group => {
            let mesh: Mesh = Cuboid::new(0.5, 0.5, 0.5).into();
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgba(0.3, 0.8, 0.3, 0.5),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            (mesh, mat)
        }
        InsertPreviewKind::Spline => {
            let mesh: Mesh = Capsule3d::new(0.1, 2.0).into();
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgb(0.3, 0.5, 1.0),
                ..default()
            });
            (mesh, mat)
        }
        InsertPreviewKind::FogVolume => {
            let mesh: Mesh = Cuboid::new(1.0, 1.0, 1.0).into();
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgba(0.7, 0.8, 1.0, 0.4),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            (mesh, mat)
        }
        InsertPreviewKind::Decal => {
            let mesh: Mesh = Cuboid::new(1.0, 1.0, 1.0).into();
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgba(0.4, 0.9, 0.6, 0.5),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            (mesh, mat)
        }
    };

    // Compute fit-to-frame transform and store base values for the rotate system
    let fit = fit_transform_from_mesh(&mesh);
    state.fit_scale = fit.scale;
    state.fit_translation = fit.translation;
    state.rotation_angle = 0.0;

    commands.entity(entity).insert((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(mat),
        fit,
    ));
}

/// Slowly rotate the preview mesh when the insert palette is open,
/// preserving the fit-to-frame scale and translation.
fn rotate_insert_preview(
    mut state: ResMut<InsertPreviewState>,
    time: Res<Time>,
    palette_state: Res<CommandPaletteState>,
    mut mesh: Query<&mut Transform, With<InsertPreviewMesh>>,
) {
    if !palette_state.open || palette_state.mode != PaletteMode::Insert {
        return;
    }

    state.rotation_angle += PREVIEW_ROTATION_SPEED * time.delta_secs();
    if let Ok(mut transform) = mesh.single_mut() {
        apply_preview_rotation(
            state.rotation_angle,
            &mut transform,
            state.fit_scale,
            state.fit_translation,
        );
    }
}
