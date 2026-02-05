//! Shared render-to-texture infrastructure for 3D preview viewports.
//!
//! Each preview system (insert, GLTF, material) has unique update logic, but
//! the scene setup, texture registration, rotation, and fit-to-frame math are
//! identical. This module eliminates that duplication.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::MeshAabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy_egui::{egui, EguiTextureHandle, EguiUserTextures};

/// Resolution of all preview render textures.
pub const PREVIEW_TEXTURE_SIZE: u32 = 512;

/// Rotation speed (radians per second) for spinning previews.
pub const PREVIEW_ROTATION_SPEED: f32 = 0.3;

/// The maximum extent (largest AABB dimension) the preview object should occupy.
pub const FIT_TARGET_SIZE: f32 = 1.6;

/// The Y coordinate the camera looks at — object centers are placed here.
pub const LOOK_AT_Y: f32 = 0.5;

/// Embeddable struct replacing duplicated `render_texture` + `egui_texture_id` fields.
pub struct PreviewTexture {
    pub render_texture: Handle<Image>,
    pub egui_texture_id: Option<egui::TextureId>,
}

/// Parameterizes all setup differences between preview scenes.
pub struct PreviewSceneConfig {
    pub render_layer: usize,
    pub camera_order: isize,
    pub camera_position: Vec3,
    pub look_at: Vec3,
    pub clear_color: ClearColorConfig,
    pub directional_illuminance: f32,
    pub spawn_point_light: bool,
    pub spawn_ground_plane: bool,
}

impl PreviewSceneConfig {
    /// Transparent background, 2000 lux directional + point light fill, no ground.
    /// Suitable for insert and GLTF previews.
    pub fn object_preview(layer: usize, order: isize) -> Self {
        Self {
            render_layer: layer,
            camera_order: order,
            camera_position: Vec3::new(2.0, 1.5, 3.5),
            look_at: Vec3::new(0.0, LOOK_AT_Y, 0.0),
            clear_color: ClearColorConfig::Custom(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            directional_illuminance: 2000.0,
            spawn_point_light: true,
            spawn_ground_plane: false,
        }
    }

    /// Dark gray opaque background, 3000 lux directional, ground plane, no point light.
    /// Suitable for material preview spheres.
    pub fn material_studio(layer: usize, order: isize) -> Self {
        Self {
            render_layer: layer,
            camera_order: order,
            camera_position: Vec3::new(0.0, 0.5, 2.5),
            look_at: Vec3::ZERO,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.12, 0.14, 0.18)),
            directional_illuminance: 3000.0,
            spawn_point_light: false,
            spawn_ground_plane: true,
        }
    }
}

/// Handles returned from `spawn_preview_scene` so callers can insert their own
/// marker components.
pub struct PreviewSceneHandles {
    pub render_texture: Handle<Image>,
    pub camera: Entity,
    pub lights: Vec<Entity>,
    pub ground: Option<Entity>,
}

/// Spawn a complete preview scene (camera, lights, optional ground) on a
/// dedicated render layer. Returns handles so callers can attach markers.
pub fn spawn_preview_scene(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    config: &PreviewSceneConfig,
) -> PreviewSceneHandles {
    let size = Extent3d {
        width: PREVIEW_TEXTURE_SIZE,
        height: PREVIEW_TEXTURE_SIZE,
        depth_or_array_layers: 1,
    };

    // Transparent vs opaque initial fill
    let fill = match &config.clear_color {
        ClearColorConfig::Custom(c) => {
            let linear = c.to_linear();
            [
                (linear.red * 255.0) as u8,
                (linear.green * 255.0) as u8,
                (linear.blue * 255.0) as u8,
                (linear.alpha * 255.0) as u8,
            ]
        }
        _ => [0, 0, 0, 0],
    };

    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &fill,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
    let render_texture = images.add(image);

    let layer = RenderLayers::layer(config.render_layer);

    // Camera
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: config.camera_order,
                clear_color: config.clear_color.clone(),
                ..default()
            },
            RenderTarget::Image(render_texture.clone().into()),
            Transform::from_translation(config.camera_position)
                .looking_at(config.look_at, Vec3::Y),
            layer.clone(),
        ))
        .id();

    let mut lights = Vec::new();

    // Directional light
    let dir_light = commands
        .spawn((
            DirectionalLight {
                illuminance: config.directional_illuminance,
                shadows_enabled: false,
                ..default()
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
            layer.clone(),
        ))
        .id();
    lights.push(dir_light);

    // Optional point light fill
    if config.spawn_point_light {
        let point_light = commands
            .spawn((
                PointLight {
                    intensity: 100_000.0,
                    range: 20.0,
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(-2.0, 3.0, 1.0),
                layer.clone(),
            ))
            .id();
        lights.push(point_light);
    }

    // Optional ground plane
    let ground = if config.spawn_ground_plane {
        let ground_mesh = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(3.0)));
        let ground_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.15, 0.15, 0.17),
            ..default()
        });
        let ground_entity = commands
            .spawn((
                Mesh3d(ground_mesh),
                MeshMaterial3d(ground_material),
                Transform::from_xyz(0.0, -0.7, 0.0),
                layer,
            ))
            .id();
        Some(ground_entity)
    } else {
        None
    };

    PreviewSceneHandles {
        render_texture,
        camera,
        lights,
        ground,
    }
}

/// One-shot registration of a preview render texture with bevy_egui.
/// Returns `true` if already registered (or just registered).
pub fn register_preview_egui_texture(
    texture: &mut PreviewTexture,
    user_textures: &mut EguiUserTextures,
) -> bool {
    if texture.egui_texture_id.is_some() {
        return true;
    }
    let id =
        user_textures.add_image(EguiTextureHandle::Strong(texture.render_texture.clone()));
    texture.egui_texture_id = Some(id);
    true
}

/// Apply Y-axis rotation to a preview entity while preserving its
/// fit-to-frame scale and translation offset.
pub fn apply_preview_rotation(
    angle: f32,
    transform: &mut Transform,
    fit_scale: Vec3,
    fit_translation: Vec3,
) {
    let rotation = Quat::from_rotation_y(angle);
    transform.rotation = rotation;
    transform.scale = fit_scale;
    transform.translation = rotation * fit_translation;
}

/// Compute a transform that centers a mesh's AABB at `(0, LOOK_AT_Y, 0)` and
/// uniformly scales it so its largest dimension equals `FIT_TARGET_SIZE`.
pub fn fit_transform_from_mesh(mesh: &Mesh) -> Transform {
    let Some(aabb) = mesh.compute_aabb() else {
        return Transform::IDENTITY;
    };
    let center = Vec3::from(aabb.center);
    let extents = Vec3::from(aabb.half_extents) * 2.0;
    let (scale, translation) = fit_transform_from_extents(center, extents);
    Transform {
        translation,
        scale,
        ..default()
    }
}

/// Given a world-space AABB center and full extents, compute the uniform scale
/// and translation needed to fit the object into the preview frame.
///
/// Returns `(scale_vec, translation_vec)`.
pub fn fit_transform_from_extents(center: Vec3, extents: Vec3) -> (Vec3, Vec3) {
    let max_dim = extents.x.max(extents.y).max(extents.z).max(0.001);
    let scale = FIT_TARGET_SIZE / max_dim;
    let translation = Vec3::new(
        -center.x * scale,
        LOOK_AT_Y - center.y * scale,
        -center.z * scale,
    );
    (Vec3::splat(scale), translation)
}
