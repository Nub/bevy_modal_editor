//! Compute pipeline creation and bind group layouts.

use bevy::prelude::*;
use bevy::render::render_resource::*;
use bevy::render::render_resource::binding_types::{storage_buffer_sized, uniform_buffer_sized};
use bevy::render::renderer::RenderDevice;

/// Cached compute pipelines and bind group layout for VFX simulation.
#[derive(Resource)]
pub struct VfxComputePipelines {
    pub bind_group_layout: BindGroupLayout,
    pub spawn_pipeline: CachedComputePipelineId,
    pub update_pipeline: CachedComputePipelineId,
    pub compact_pipeline: CachedComputePipelineId,
}

impl VfxComputePipelines {
    pub fn new(
        device: &RenderDevice,
        pipeline_cache: &PipelineCache,
        spawn_shader: Handle<Shader>,
        update_shader: Handle<Shader>,
        compact_shader: Handle<Shader>,
    ) -> Self {
        // All five bindings visible to compute stage
        let layout_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                // @binding(0) ParticleBuffer (storage, read_write)
                storage_buffer_sized(false, None),
                // @binding(1) AliveBuffer (storage, read_write)
                storage_buffer_sized(false, None),
                // @binding(2) DeadBuffer (storage, read_write)
                storage_buffer_sized(false, None),
                // @binding(3) CounterBuffer (storage, read_write)
                storage_buffer_sized(false, None),
                // @binding(4) EmitterParams (uniform)
                uniform_buffer_sized(false, None),
            ),
        );

        let bind_group_layout = device.create_bind_group_layout(
            Some("vfx_compute_bind_group_layout"),
            &layout_entries,
        );

        let layout_desc = BindGroupLayoutDescriptor::new(
            "vfx_compute_bind_group_layout",
            &layout_entries,
        );

        let spawn_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("vfx_spawn_pipeline".into()),
            layout: vec![layout_desc.clone()],
            push_constant_ranges: vec![],
            shader: spawn_shader,
            shader_defs: vec![],
            entry_point: Some("main".into()),
            zero_initialize_workgroup_memory: true,
        });

        let update_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("vfx_update_pipeline".into()),
            layout: vec![layout_desc.clone()],
            push_constant_ranges: vec![],
            shader: update_shader,
            shader_defs: vec![],
            entry_point: Some("main".into()),
            zero_initialize_workgroup_memory: true,
        });

        let compact_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("vfx_compact_pipeline".into()),
            layout: vec![layout_desc],
            push_constant_ranges: vec![],
            shader: compact_shader,
            shader_defs: vec![],
            entry_point: Some("main".into()),
            zero_initialize_workgroup_memory: true,
        });

        Self {
            bind_group_layout,
            spawn_pipeline,
            update_pipeline,
            compact_pipeline,
        }
    }
}
