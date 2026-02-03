//! Multi-pass JFA outline effect with custom render nodes.
//!
//! Uses a true Jump Flood Algorithm for efficient distance field computation:
//! 1. Init pass: Convert silhouette to seed coordinates
//! 2. JFA passes: Propagate seeds with exponentially decreasing step sizes
//! 3. Composite pass: Use distance field to render outline

use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, RenderTarget},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            binding_types::{sampler as sampler_layout, texture_2d, texture_storage_2d, uniform_buffer},
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            Buffer, CachedComputePipelineId, CachedRenderPipelineId, ColorTargetState, ColorWrites,
            ComputePassDescriptor, ComputePipelineDescriptor, Extent3d, FragmentState,
            MultisampleState, Operations, PipelineCache, PrimitiveState, RenderPassColorAttachment,
            RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, StorageTextureAccess, TextureDimension,
            TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        texture::GpuImage,
        view::ViewTarget,
        Extract, Render, RenderApp,
    },
};

use crate::components::{MeshOutline, OutlineSettings};
use crate::silhouette_material::SilhouetteMaterial;

/// Render layer for silhouette rendering (layer 31 to avoid conflicts)
pub const OUTLINE_RENDER_LAYER: usize = 31;

/// GPU uniform settings for the outline composite shader.
#[derive(Clone, Copy, Default, PartialEq, ShaderType, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct OutlineShaderSettings {
    pub color: [f32; 4],
    pub width: f32,
    pub enabled: f32,
    pub _padding: [f32; 2],
}

/// GPU uniform for JFA step pass
#[derive(Clone, Copy, Default, ShaderType, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct JfaStepParams {
    pub step_size: f32,
    pub _padding: [f32; 3],
}

/// Links the main camera to its silhouette camera and textures
#[derive(Component, Clone)]
pub struct OutlineCameraLink {
    pub silhouette_camera: Entity,
    pub silhouette_texture: Handle<Image>,
    pub jfa_ping_texture: Handle<Image>,
    pub jfa_pong_texture: Handle<Image>,
}

/// Extracted outline data for render world
#[derive(Component, Clone)]
pub struct ExtractedOutlineData {
    pub silhouette_texture: Handle<Image>,
    pub jfa_ping_texture: Handle<Image>,
    pub jfa_pong_texture: Handle<Image>,
    pub settings: OutlineShaderSettings,
}

/// Cached GPU resources for outline rendering (per-camera)
/// These are created once and reused each frame to avoid allocation overhead
#[derive(Component)]
#[allow(dead_code)] // step_buffers kept alive to maintain bind group validity
pub struct OutlineRenderResources {
    pub ping_view: TextureView,
    pub pong_view: TextureView,
    pub init_bind_group: BindGroup,
    pub step_bind_groups: Vec<BindGroup>,
    pub step_buffers: Vec<Buffer>,
    pub settings_buffer: Buffer,
    /// Cached values to detect when resources need recreation
    pub cached_width: f32,
    pub cached_texture_size: (u32, u32),
    /// Cached settings to avoid unnecessary buffer writes
    pub cached_settings: OutlineShaderSettings,
}

/// Marker for silhouette cameras
#[derive(Component)]
pub struct SilhouetteCamera;

/// Marker for silhouette mesh copies
#[derive(Component)]
pub struct SilhouetteMesh;

/// Marker component added to source entities that have a silhouette mesh spawned
#[derive(Component)]
pub struct HasSilhouetteMesh {
    pub silhouette: Entity,
}

/// Render label for the outline node
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct OutlineNodeLabel;

/// Resource holding the silhouette material
#[derive(Resource, Clone)]
pub struct SilhouetteWhiteMaterial(pub Handle<SilhouetteMaterial>);

/// System to set up silhouette camera for main cameras with OutlineSettings
pub fn setup_outline_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<SilhouetteMaterial>>,
    cameras: Query<
        (Entity, &Camera, &Transform, &Projection, Option<&RenderTarget>),
        (With<OutlineSettings>, Without<OutlineCameraLink>),
    >,
    windows: Query<&Window>,
) {
    for (entity, _camera, transform, projection, render_target) in cameras.iter() {
        // Get the camera's target size
        let size = match render_target {
            Some(RenderTarget::Window(window_ref)) => {
                let window = match window_ref {
                    bevy::window::WindowRef::Primary => windows.iter().next(),
                    bevy::window::WindowRef::Entity(e) => windows.get(*e).ok(),
                };
                window.map(|w| UVec2::new(w.physical_width(), w.physical_height()))
            }
            Some(RenderTarget::Image(image_target)) => {
                images.get(&image_target.handle).map(|img| img.size())
            }
            _ => {
                // Default to primary window
                windows
                    .iter()
                    .next()
                    .map(|w| UVec2::new(w.physical_width(), w.physical_height()))
            }
        };

        let size = size.unwrap_or(UVec2::new(1920, 1080));

        // Create silhouette render texture
        let mut silhouette_image = Image::new_fill(
            Extent3d {
                width: size.x.max(1),
                height: size.y.max(1),
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        silhouette_image.texture_descriptor.usage =
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
        let silhouette_handle = images.add(silhouette_image);

        // Create JFA ping-pong textures (RG16Float to store UV coordinates)
        let jfa_extent = Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        };

        // JFA textures need STORAGE_BINDING for compute shaders
        // Using Rg16Unorm instead of Rg16Float - sufficient for UV coords in [0,1] range
        let mut jfa_ping_image = Image::new_fill(
            jfa_extent,
            TextureDimension::D2,
            &[0; 4], // 2 x u16 = 4 bytes
            TextureFormat::Rg16Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        jfa_ping_image.texture_descriptor.usage =
            TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING;
        let jfa_ping_handle = images.add(jfa_ping_image);

        let mut jfa_pong_image = Image::new_fill(
            jfa_extent,
            TextureDimension::D2,
            &[0; 4],
            TextureFormat::Rg16Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        jfa_pong_image.texture_descriptor.usage =
            TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING;
        let jfa_pong_handle = images.add(jfa_pong_image);

        // Create silhouette material (minimal shader, no PBR)
        let white_material = materials.add(SilhouetteMaterial::default());

        // Store the white material handle for silhouette meshes
        commands.insert_resource(SilhouetteWhiteMaterial(white_material));

        // Spawn silhouette camera
        let silhouette_camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: -1, // Render before main camera
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(silhouette_handle.clone().into()),
                *transform,
                projection.clone(),
                RenderLayers::layer(OUTLINE_RENDER_LAYER),
                SilhouetteCamera,
            ))
            .id();

        // Link main camera to silhouette camera and textures
        commands.entity(entity).insert(OutlineCameraLink {
            silhouette_camera,
            silhouette_texture: silhouette_handle,
            jfa_ping_texture: jfa_ping_handle,
            jfa_pong_texture: jfa_pong_handle,
        });
    }
}

/// System to sync silhouette meshes with outlined entities
pub fn sync_outline_meshes(
    mut commands: Commands,
    white_material: Option<Res<SilhouetteWhiteMaterial>>,
    // Only query entities that don't already have a silhouette spawned
    outlined: Query<
        (Entity, &Mesh3d, &GlobalTransform),
        (With<MeshOutline>, Without<HasSilhouetteMesh>),
    >,
    mut silhouettes: Query<(Entity, &mut Transform), (With<SilhouetteMesh>, Without<MeshOutline>)>,
    // Only query sources with changed transforms
    changed_sources: Query<(Entity, &GlobalTransform), (With<MeshOutline>, Changed<GlobalTransform>)>,
    // Track entities that had MeshOutline removed
    mut removed: RemovedComponents<MeshOutline>,
    // Query to get the silhouette entity from source
    sources_with_silhouettes: Query<(Entity, &HasSilhouetteMesh)>,
) {
    let Some(white_material) = white_material else {
        return;
    };

    // Add silhouette meshes for new outlined entities
    for (entity, mesh, global_transform) in outlined.iter() {
        let (scale, rotation, translation) = global_transform.to_scale_rotation_translation();

        let silhouette_entity = commands
            .spawn((
                SilhouetteMesh,
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(white_material.0.clone()),
                Transform {
                    translation,
                    rotation,
                    scale,
                },
                RenderLayers::layer(OUTLINE_RENDER_LAYER),
            ))
            .id();

        // Mark the source entity as having a silhouette
        commands.entity(entity).insert(HasSilhouetteMesh {
            silhouette: silhouette_entity,
        });
    }

    // Update silhouette transforms - O(n) by iterating changed sources directly
    for (source_entity, global_transform) in changed_sources.iter() {
        if let Ok((_, has_silhouette)) = sources_with_silhouettes.get(source_entity) {
            if let Ok((_, mut sil_transform)) = silhouettes.get_mut(has_silhouette.silhouette) {
                let (scale, rotation, translation) = global_transform.to_scale_rotation_translation();
                sil_transform.translation = translation;
                sil_transform.rotation = rotation;
                sil_transform.scale = scale;
            }
        }
    }

    // Remove silhouette meshes for removed outlines
    for entity in removed.read() {
        if let Ok((_, has_silhouette)) = sources_with_silhouettes.get(entity) {
            commands.entity(has_silhouette.silhouette).despawn();
            // Remove HasSilhouetteMesh so outline can be re-added later
            commands.entity(entity).remove::<HasSilhouetteMesh>();
        }
    }
}

/// Syncs silhouette camera transform with main camera
pub fn sync_silhouette_cameras(
    main_cameras: Query<(&Transform, &Projection, &OutlineCameraLink), Changed<Transform>>,
    mut silhouette_cameras: Query<
        (&mut Transform, &mut Projection),
        (With<SilhouetteCamera>, Without<OutlineCameraLink>),
    >,
) {
    for (main_transform, main_projection, link) in main_cameras.iter() {
        if let Ok((mut sil_transform, mut sil_projection)) =
            silhouette_cameras.get_mut(link.silhouette_camera)
        {
            *sil_transform = *main_transform;
            *sil_projection = main_projection.clone();
        }
    }
}

/// Resizes silhouette and JFA textures when the window size changes
pub fn resize_silhouette_textures(
    mut images: ResMut<Assets<Image>>,
    cameras: Query<(Option<&RenderTarget>, &OutlineCameraLink), With<OutlineSettings>>,
    windows: Query<&Window>,
) {
    for (render_target, link) in cameras.iter() {
        // Get current window size
        let target_size = match render_target {
            Some(RenderTarget::Window(window_ref)) => {
                let window = match window_ref {
                    bevy::window::WindowRef::Primary => windows.iter().next(),
                    bevy::window::WindowRef::Entity(e) => windows.get(*e).ok(),
                };
                window.map(|w| UVec2::new(w.physical_width(), w.physical_height()))
            }
            Some(RenderTarget::Image(image_target)) => {
                images.get(&image_target.handle).map(|img| img.size())
            }
            _ => windows
                .iter()
                .next()
                .map(|w| UVec2::new(w.physical_width(), w.physical_height())),
        };

        let Some(target_size) = target_size else {
            continue;
        };

        // Skip if size is zero
        if target_size.x == 0 || target_size.y == 0 {
            continue;
        }

        let extent = Extent3d {
            width: target_size.x,
            height: target_size.y,
            depth_or_array_layers: 1,
        };

        // Resize silhouette texture
        if let Some(silhouette_image) = images.get(&link.silhouette_texture) {
            if silhouette_image.size() != target_size {
                if let Some(img) = images.get_mut(&link.silhouette_texture) {
                    img.resize(extent);
                }
            }
        }

        // Resize JFA ping texture
        if let Some(jfa_ping_image) = images.get(&link.jfa_ping_texture) {
            if jfa_ping_image.size() != target_size {
                if let Some(img) = images.get_mut(&link.jfa_ping_texture) {
                    img.resize(extent);
                }
            }
        }

        // Resize JFA pong texture
        if let Some(jfa_pong_image) = images.get(&link.jfa_pong_texture) {
            if jfa_pong_image.size() != target_size {
                if let Some(img) = images.get_mut(&link.jfa_pong_texture) {
                    img.resize(extent);
                }
            }
        }
    }
}

/// Extract outline data to render world
pub fn extract_outline_data(
    mut commands: Commands,
    cameras: Extract<Query<(Entity, &OutlineCameraLink, &OutlineSettings)>>,
    outlines: Extract<Query<&MeshOutline>>,
    render_entity_lookup: Extract<Query<&bevy::render::sync_world::RenderEntity>>,
) {
    // Early exit if no outlined entities - skip all rendering
    let Some(first_outline) = outlines.iter().next() else {
        return;
    };

    let color = [
        first_outline.color.red,
        first_outline.color.green,
        first_outline.color.blue,
        first_outline.color.alpha,
    ];
    let width = first_outline.width;

    for (entity, link, settings) in cameras.iter() {
        // Get the render entity for this camera
        let Ok(render_entity) = render_entity_lookup.get(entity) else {
            continue;
        };

        commands.entity(render_entity.id()).insert(ExtractedOutlineData {
            silhouette_texture: link.silhouette_texture.clone(),
            jfa_ping_texture: link.jfa_ping_texture.clone(),
            jfa_pong_texture: link.jfa_pong_texture.clone(),
            settings: OutlineShaderSettings {
                color,
                width,
                enabled: if settings.enabled { 1.0 } else { 0.0 },
                _padding: [0.0; 2],
            },
        });
    }
}

/// Prepare system that creates/updates cached GPU resources for outline rendering
pub fn prepare_outline_resources(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    outline_pipeline: Res<OutlinePipeline>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    query: Query<(Entity, &ExtractedOutlineData, Option<&OutlineRenderResources>)>,
) {
    for (entity, outline_data, existing_resources) in query.iter() {
        // Get GPU textures
        let Some(silhouette_gpu) = gpu_images.get(&outline_data.silhouette_texture) else {
            continue;
        };
        let Some(jfa_ping_gpu) = gpu_images.get(&outline_data.jfa_ping_texture) else {
            continue;
        };
        let Some(jfa_pong_gpu) = gpu_images.get(&outline_data.jfa_pong_texture) else {
            continue;
        };

        let tex_width = jfa_ping_gpu.texture.width();
        let tex_height = jfa_ping_gpu.texture.height();
        let width = outline_data.settings.width;

        // Check if we can reuse existing resources
        if let Some(existing) = existing_resources {
            if existing.cached_width == width
                && existing.cached_texture_size == (tex_width, tex_height)
            {
                // Only update settings buffer if settings actually changed
                if existing.cached_settings != outline_data.settings {
                    render_queue.write_buffer(
                        &existing.settings_buffer,
                        0,
                        bytemuck::bytes_of(&outline_data.settings),
                    );
                }
                continue;
            }
        }

        // Need to create or recreate resources
        let ping_view = jfa_ping_gpu
            .texture
            .create_view(&TextureViewDescriptor::default());
        let pong_view = jfa_pong_gpu
            .texture
            .create_view(&TextureViewDescriptor::default());

        // Calculate pass count
        let actual_width = width.ceil() as u32;
        let pass_count = if actual_width > 0 {
            ((actual_width as f32).log2().ceil() as u32).max(1)
        } else {
            0
        };

        // Create init bind group
        let init_bind_group = render_device.create_bind_group(
            "jfa_init_compute_bind_group",
            &outline_pipeline.init_layout,
            &BindGroupEntries::sequential((&silhouette_gpu.texture_view, &ping_view)),
        );

        // Create step buffers and bind groups
        let mut step_buffers = Vec::with_capacity(pass_count as usize);
        let mut step_bind_groups = Vec::with_capacity(pass_count as usize);

        for pass_idx in 0..pass_count {
            let step_size = (actual_width >> (pass_idx + 1)).max(1) as f32;
            let read_from_ping = pass_idx % 2 == 0;

            let (input_view, output_view) = if read_from_ping {
                (&ping_view, &pong_view)
            } else {
                (&pong_view, &ping_view)
            };

            let step_buffer = render_device.create_buffer_with_data(
                &bevy::render::render_resource::BufferInitDescriptor {
                    label: Some("jfa_step_params_buffer"),
                    contents: bytemuck::bytes_of(&JfaStepParams {
                        step_size,
                        _padding: [0.0; 3],
                    }),
                    usage: bevy::render::render_resource::BufferUsages::UNIFORM,
                },
            );

            let step_bind_group = render_device.create_bind_group(
                "jfa_step_compute_bind_group",
                &outline_pipeline.step_layout,
                &BindGroupEntries::sequential((
                    input_view,
                    output_view,
                    step_buffer.as_entire_binding(),
                )),
            );

            step_buffers.push(step_buffer);
            step_bind_groups.push(step_bind_group);
        }

        // Create settings buffer
        let settings_buffer = render_device.create_buffer_with_data(
            &bevy::render::render_resource::BufferInitDescriptor {
                label: Some("outline_settings_buffer"),
                contents: bytemuck::bytes_of(&outline_data.settings),
                usage: bevy::render::render_resource::BufferUsages::UNIFORM
                    | bevy::render::render_resource::BufferUsages::COPY_DST,
            },
        );

        commands.entity(entity).insert(OutlineRenderResources {
            ping_view,
            pong_view,
            init_bind_group,
            step_bind_groups,
            step_buffers,
            settings_buffer,
            cached_width: width,
            cached_texture_size: (tex_width, tex_height),
            cached_settings: outline_data.settings,
        });
    }
}

/// Pipeline resource for outline rendering
#[derive(Resource)]
pub struct OutlinePipeline {
    // Init pass - COMPUTE shader
    pub init_layout: BindGroupLayout,
    pub init_pipeline_id: CachedComputePipelineId,

    // JFA step pass - COMPUTE shader
    pub step_layout: BindGroupLayout,
    pub step_pipeline_id: CachedComputePipelineId,

    // Composite pass - fragment shader
    pub composite_layout: BindGroupLayout,
    pub composite_pipeline_id: CachedRenderPipelineId,
    pub composite_pipeline_id_hdr: CachedRenderPipelineId,

    pub sampler: Sampler,
}

impl FromWorld for OutlinePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let asset_server = world.resource::<AssetServer>();
        let pipeline_cache = world.resource::<PipelineCache>();

        let sampler = render_device.create_sampler(&SamplerDescriptor::default());

        // Shaders
        let vertex_shader = asset_server
            .load("embedded://bevy_core_pipeline/fullscreen_vertex_shader/fullscreen.wgsl");
        let composite_shader =
            asset_server.load("embedded://bevy_outliner/shaders/jfa_composite.wgsl");

        // ========== Init Compute Pipeline ==========
        let init_compute_shader =
            asset_server.load("embedded://bevy_outliner/shaders/jfa_init_compute.wgsl");

        let init_layout_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                // Silhouette texture (read)
                texture_2d(TextureSampleType::Float { filterable: false }),
                // Output texture (write)
                texture_storage_2d(TextureFormat::Rg16Unorm, StorageTextureAccess::WriteOnly),
            ),
        );

        let init_layout = render_device.create_bind_group_layout(
            Some("jfa_init_compute_bind_group_layout"),
            &init_layout_entries,
        );

        let init_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("jfa_init_compute_pipeline".into()),
            layout: vec![BindGroupLayoutDescriptor::new(
                "jfa_init_compute_bind_group_layout",
                &init_layout_entries,
            )],
            shader: init_compute_shader,
            shader_defs: vec![],
            entry_point: Some("main".into()),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });

        // ========== Step Compute Pipeline ==========
        let step_compute_shader =
            asset_server.load("embedded://bevy_outliner/shaders/jfa_step_compute.wgsl");

        let step_layout_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                // JFA input texture (read)
                texture_2d(TextureSampleType::Float { filterable: false }),
                // Output texture (write)
                texture_storage_2d(TextureFormat::Rg16Unorm, StorageTextureAccess::WriteOnly),
                // Step params uniform
                uniform_buffer::<JfaStepParams>(false),
            ),
        );

        let step_layout = render_device.create_bind_group_layout(
            Some("jfa_step_compute_bind_group_layout"),
            &step_layout_entries,
        );

        let step_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("jfa_step_compute_pipeline".into()),
            layout: vec![BindGroupLayoutDescriptor::new(
                "jfa_step_compute_bind_group_layout",
                &step_layout_entries,
            )],
            shader: step_compute_shader,
            shader_defs: vec![],
            entry_point: Some("main".into()),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });

        // ========== Composite Pipeline ==========
        let composite_layout_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // Scene texture
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Scene sampler
                sampler_layout(SamplerBindingType::Filtering),
                // JFA result texture
                texture_2d(TextureSampleType::Float { filterable: true }),
                // JFA sampler
                sampler_layout(SamplerBindingType::Filtering),
                // Silhouette texture
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Silhouette sampler
                sampler_layout(SamplerBindingType::Filtering),
                // Settings uniform
                uniform_buffer::<OutlineShaderSettings>(false),
            ),
        );

        let composite_layout = render_device.create_bind_group_layout(
            Some("jfa_composite_bind_group_layout"),
            &composite_layout_entries,
        );

        let composite_layout_desc = BindGroupLayoutDescriptor::new(
            "jfa_composite_bind_group_layout",
            &composite_layout_entries,
        );

        let composite_pipeline_id =
            pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("jfa_composite_pipeline".into()),
                layout: vec![composite_layout_desc.clone()],
                vertex: bevy::render::render_resource::VertexState {
                    shader: vertex_shader.clone(),
                    shader_defs: vec![],
                    entry_point: Some("fullscreen_vertex_shader".into()),
                    buffers: vec![],
                },
                fragment: Some(FragmentState {
                    shader: composite_shader.clone(),
                    shader_defs: vec![],
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format: TextureFormat::bevy_default(),
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                push_constant_ranges: vec![],
                zero_initialize_workgroup_memory: false,
            });

        let composite_pipeline_id_hdr =
            pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("jfa_composite_pipeline_hdr".into()),
                layout: vec![composite_layout_desc],
                vertex: bevy::render::render_resource::VertexState {
                    shader: vertex_shader,
                    shader_defs: vec![],
                    entry_point: Some("fullscreen_vertex_shader".into()),
                    buffers: vec![],
                },
                fragment: Some(FragmentState {
                    shader: composite_shader,
                    shader_defs: vec![],
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format: ViewTarget::TEXTURE_FORMAT_HDR,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                push_constant_ranges: vec![],
                zero_initialize_workgroup_memory: false,
            });

        Self {
            init_layout,
            init_pipeline_id,
            step_layout,
            step_pipeline_id,
            composite_layout,
            composite_pipeline_id,
            composite_pipeline_id_hdr,
            sampler,
        }
    }
}

/// The outline render node - runs JFA passes and composites the result
/// Uses cached resources from OutlineRenderResources to avoid per-frame allocations
#[derive(Default)]
pub struct OutlineNode;

impl ViewNode for OutlineNode {
    type ViewQuery = (
        &'static ViewTarget,
        Option<&'static ExtractedOutlineData>,
        Option<&'static OutlineRenderResources>,
    );

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (view_target, outline_data, render_resources): bevy::ecs::query::QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let Some(outline_data) = outline_data else {
            return Ok(());
        };
        let Some(render_resources) = render_resources else {
            // Resources not yet prepared, skip this frame
            return Ok(());
        };

        let outline_pipeline = world.resource::<OutlinePipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let gpu_images = world.resource::<RenderAssets<GpuImage>>();

        // Get silhouette texture for composite pass
        let Some(silhouette_gpu) = gpu_images.get(&outline_data.silhouette_texture) else {
            return Ok(());
        };
        let Some(jfa_ping_gpu) = gpu_images.get(&outline_data.jfa_ping_texture) else {
            return Ok(());
        };

        // Get compute pipelines
        let Some(init_pipeline) = pipeline_cache.get_compute_pipeline(outline_pipeline.init_pipeline_id) else {
            return Ok(());
        };
        let Some(step_pipeline) = pipeline_cache.get_compute_pipeline(outline_pipeline.step_pipeline_id) else {
            return Ok(());
        };

        let composite_pipeline_id = if view_target.is_hdr() {
            outline_pipeline.composite_pipeline_id_hdr
        } else {
            outline_pipeline.composite_pipeline_id
        };
        let Some(composite_pipeline) = pipeline_cache.get_render_pipeline(composite_pipeline_id) else {
            return Ok(());
        };

        // ========== Run compute passes using cached resources ==========

        // Calculate workgroup count (8x8 workgroups)
        let tex_width = jfa_ping_gpu.texture.width();
        let tex_height = jfa_ping_gpu.texture.height();
        let workgroups_x = (tex_width + 7) / 8;
        let workgroups_y = (tex_height + 7) / 8;

        // Init Compute Pass: Convert silhouette to seed coordinates
        {
            let mut compute_pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("jfa_init_compute_pass"),
                        timestamp_writes: None,
                    });

            compute_pass.set_pipeline(init_pipeline);
            compute_pass.set_bind_group(0, &render_resources.init_bind_group, &[]);
            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        // JFA Step Compute Passes: Propagate seeds with decreasing step sizes
        for step_bind_group in &render_resources.step_bind_groups {
            let mut compute_pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("jfa_step_compute_pass"),
                        timestamp_writes: None,
                    });

            compute_pass.set_pipeline(step_pipeline);
            compute_pass.set_bind_group(0, step_bind_group, &[]);
            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        // Determine which texture has the final JFA result
        let pass_count = render_resources.step_bind_groups.len();
        let jfa_result_view = if pass_count % 2 == 0 {
            &render_resources.ping_view
        } else {
            &render_resources.pong_view
        };

        // Composite Pass: Blend outline over scene using JFA distance field
        // Note: composite_bind_group must be created each frame because post_process.source changes
        {
            let post_process = view_target.post_process_write();

            let composite_bind_group = render_context.render_device().create_bind_group(
                "jfa_composite_bind_group",
                &outline_pipeline.composite_layout,
                &BindGroupEntries::sequential((
                    post_process.source,
                    &outline_pipeline.sampler,
                    jfa_result_view,
                    &outline_pipeline.sampler,
                    &silhouette_gpu.texture_view,
                    &outline_pipeline.sampler,
                    render_resources.settings_buffer.as_entire_binding(),
                )),
            );

            let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("jfa_composite_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: post_process.destination,
                    resolve_target: None,
                    ops: Operations::default(),
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_render_pipeline(composite_pipeline);
            render_pass.set_bind_group(0, &composite_bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }

        Ok(())
    }
}

/// Plugin that sets up the outline render node
pub struct OutlineRenderPlugin;

impl Plugin for OutlineRenderPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .add_systems(ExtractSchedule, extract_outline_data)
            .add_systems(Render, prepare_outline_resources)
            .add_render_graph_node::<ViewNodeRunner<OutlineNode>>(Core3d, OutlineNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::Tonemapping,
                    OutlineNodeLabel,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<OutlinePipeline>();
    }
}
