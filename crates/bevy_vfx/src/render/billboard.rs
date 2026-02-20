//! Billboard particle renderer using GPU instanced draw.
//!
//! `VfxBillboardNode` is a `ViewNode` that runs in the Core3d sub-graph after
//! the main transparent pass. For each active emitter, it issues an instanced
//! draw call — the vertex shader reads particle data from storage buffers and
//! constructs camera-facing quads. No CPU readback at any point.

use bevy::prelude::*;
use bevy::render::render_graph::{self, RenderGraphContext, RenderLabel, ViewNode};
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only_sized, texture_2d, uniform_buffer_sized,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};

use crate::gpu::buffers::{ActiveEmitterBuffers, VfxCameraUniforms};

/// Render graph label for the VFX billboard draw node.
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct VfxBillboardLabel;

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Cached billboard render pipeline and bind group layouts.
#[derive(Resource)]
pub struct VfxBillboardPipeline {
    /// Group 0: particle_buffer, alive_buffer, counter_buffer, texture, sampler.
    pub particle_bind_group_layout: BindGroupLayout,
    /// Group 1: camera uniforms (uniform buffer).
    pub camera_bind_group_layout: BindGroupLayout,
    /// The cached render pipeline ID.
    pub pipeline_id: CachedRenderPipelineId,
    /// 1x1 white fallback texture view (for emitters without a texture).
    pub fallback_texture_view: TextureView,
    /// Default linear sampler for the fallback texture.
    pub fallback_sampler: Sampler,
}

impl VfxBillboardPipeline {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        pipeline_cache: &PipelineCache,
        billboard_shader: Handle<Shader>,
    ) -> Self {
        // Group 0: particle data (3 storage buffers + texture + sampler)
        // VERTEX_FRAGMENT visibility so storage is accessible in vertex shader
        // and texture/sampler in fragment shader.
        let particle_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                storage_buffer_read_only_sized(false, None), // @binding(0) particles
                storage_buffer_read_only_sized(false, None), // @binding(1) alive
                storage_buffer_read_only_sized(false, None), // @binding(2) counter
                texture_2d(TextureSampleType::Float { filterable: true }), // @binding(3) texture
                sampler(SamplerBindingType::Filtering), // @binding(4) sampler
            ),
        );
        let particle_bind_group_layout = device.create_bind_group_layout(
            Some("vfx_billboard_particle_layout"),
            &particle_entries,
        );
        let particle_layout_desc = BindGroupLayoutDescriptor::new(
            "vfx_billboard_particle_layout",
            &particle_entries,
        );

        // Group 1: camera uniforms (1 uniform buffer)
        let camera_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX,
            (uniform_buffer_sized(false, None),),
        );
        let camera_bind_group_layout = device.create_bind_group_layout(
            Some("vfx_billboard_camera_layout"),
            &camera_entries,
        );
        let camera_layout_desc = BindGroupLayoutDescriptor::new(
            "vfx_billboard_camera_layout",
            &camera_entries,
        );

        let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("vfx_billboard_pipeline".into()),
            layout: vec![particle_layout_desc, camera_layout_desc],
            push_constant_ranges: vec![],
            vertex: VertexState {
                shader: billboard_shader.clone(),
                shader_defs: vec![],
                entry_point: Some("vertex_main".into()),
                buffers: vec![], // Procedural quads — no vertex buffers
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None, // Billboards are double-sided
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: false, // Transparent particles don't write depth
                depth_compare: CompareFunction::GreaterEqual, // Reverse-Z
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState {
                count: 4, // Match editor camera MSAA
                ..Default::default()
            },
            fragment: Some(FragmentState {
                shader: billboard_shader,
                shader_defs: vec![],
                entry_point: Some("fragment_main".into()),
                targets: vec![Some(ColorTargetState {
                    format: ViewTarget::TEXTURE_FORMAT_HDR,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            zero_initialize_workgroup_memory: false,
        });

        // Create 1x1 white fallback texture for emitters without a texture
        let fallback_texture = device.create_texture(&TextureDescriptor {
            label: Some("vfx_fallback_texture"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &fallback_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &[255, 255, 255, 255],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let fallback_texture_view =
            fallback_texture.create_view(&TextureViewDescriptor::default());
        let fallback_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("vfx_fallback_sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });

        Self {
            particle_bind_group_layout,
            camera_bind_group_layout,
            pipeline_id,
            fallback_texture_view,
            fallback_sampler,
        }
    }
}

// ---------------------------------------------------------------------------
// ViewNode
// ---------------------------------------------------------------------------

/// Billboard draw node — runs once per camera view in the Core3d sub-graph.
#[derive(Default)]
pub struct VfxBillboardNode;

impl ViewNode for VfxBillboardNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewDepthTexture,
        &'static ExtractedView,
    );

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (view_target, depth_texture, extracted_view): bevy::ecs::query::QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), render_graph::NodeRunError> {
        // Skip non-HDR views (outliner silhouette camera, shadow cameras, etc.)
        // The billboard pipeline targets Rgba16Float (HDR).
        if view_target.main_texture_format() != ViewTarget::TEXTURE_FORMAT_HDR {
            return Ok(());
        }

        let Some(pipeline_res) = world.get_resource::<VfxBillboardPipeline>() else {
            return Ok(());
        };
        let Some(active_buffers) = world.get_resource::<ActiveEmitterBuffers>() else {
            return Ok(());
        };
        if active_buffers.entries.is_empty() {
            return Ok(());
        }
        let pipeline_cache = world.resource::<PipelineCache>();
        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline_res.pipeline_id) else {
            return Ok(()); // Pipeline not compiled yet
        };

        // ---- Compute camera uniforms from this view ----
        let view_matrix = extracted_view.world_from_view.to_matrix();
        let view_proj = extracted_view.clip_from_world.unwrap_or_else(|| {
            extracted_view.clip_from_view * view_matrix.inverse()
        });
        let camera_right = view_matrix.col(0).truncate();
        let camera_up = view_matrix.col(1).truncate();
        let camera_position = view_matrix.col(3).truncate();

        let camera_uniforms = VfxCameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            camera_position: camera_position.to_array(),
            _pad0: 0.0,
            camera_right: camera_right.to_array(),
            _pad1: 0.0,
            camera_up: camera_up.to_array(),
            _pad2: 0.0,
        };

        // ---- Create all bind groups before starting the render pass ----
        let camera_buffer = render_context.render_device().create_buffer_with_data(
            &BufferInitDescriptor {
                label: Some("vfx_camera_uniform"),
                contents: bytemuck::bytes_of(&camera_uniforms),
                usage: BufferUsages::UNIFORM,
            },
        );

        let camera_bind_group = render_context.render_device().create_bind_group(
            "vfx_billboard_camera_bg",
            &pipeline_res.camera_bind_group_layout,
            &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        );

        // ---- Begin render pass ----
        let color_attachment = view_target.get_color_attachment();
        let depth_attachment = depth_texture.get_attachment(StoreOp::Store);

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("vfx_billboard_pass"),
            color_attachments: &[Some(color_attachment)],
            depth_stencil_attachment: Some(depth_attachment),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_render_pipeline(render_pipeline);
        render_pass.set_bind_group(1, &camera_bind_group, &[]);

        for entry in &active_buffers.entries {
            // Use pre-cached bind group (created once in prepare phase)
            render_pass.set_bind_group(0, &entry.billboard_bind_group, &[]);
            // Indirect draw: counter buffer starts with DrawIndirectArgs
            // (vertex_count=6, instance_count=alive_count, first_vertex=0, first_instance=0)
            // Only draws actual alive particles — no wasted GPU invocations for dead slots.
            render_pass.draw_indirect(&entry.counter_buffer, 0);
        }

        Ok(())
    }
}
