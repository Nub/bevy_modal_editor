//! Render graph node that dispatches the spawn → update → compact compute passes.

use bevy::prelude::*;
use bevy::render::render_graph::{self, RenderLabel};
use bevy::render::render_resource::*;
use bevy::render::renderer::RenderContext;

use super::buffers::ActiveEmitterBuffers;
use super::pipeline::VfxComputePipelines;

/// Render graph label for the VFX compute node.
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct VfxComputeLabel;

/// Workgroup size used in compute shaders (must match WGSL @workgroup_size).
const WORKGROUP_SIZE: u32 = 64;

/// Render graph node that runs the three compute passes for each emitter.
pub struct VfxComputeNode;

impl render_graph::Node for VfxComputeNode {
    fn run<'w>(
        &self,
        _graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), render_graph::NodeRunError> {
        let Some(pipelines) = world.get_resource::<VfxComputePipelines>() else {
            return Ok(());
        };
        let Some(active_buffers) = world.get_resource::<ActiveEmitterBuffers>() else {
            return Ok(());
        };
        if active_buffers.entries.is_empty() {
            return Ok(());
        }

        let pipeline_cache = world.resource::<PipelineCache>();

        // Resolve pipelines
        let Some(spawn_pipeline) =
            pipeline_cache.get_compute_pipeline(pipelines.spawn_pipeline)
        else {
            return Ok(());
        };
        let Some(update_pipeline) =
            pipeline_cache.get_compute_pipeline(pipelines.update_pipeline)
        else {
            return Ok(());
        };
        let Some(compact_pipeline) =
            pipeline_cache.get_compute_pipeline(pipelines.compact_pipeline)
        else {
            return Ok(());
        };

        let encoder = render_context.command_encoder();

        // Pass 1: ALL spawns, then ALL updates — batched by pipeline to minimize
        // pipeline switches (2 switches instead of 2N). wgpu inserts storage
        // barriers between dispatches that share buffers automatically.
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("vfx_spawn_update_pass"),
                ..default()
            });

            // Batch: all spawn dispatches
            pass.set_pipeline(spawn_pipeline);
            for entry in &active_buffers.entries {
                let workgroups = (entry.capacity + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE;
                pass.set_bind_group(0, &entry.compute_bind_group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }

            // Batch: all update dispatches
            pass.set_pipeline(update_pipeline);
            for entry in &active_buffers.entries {
                let workgroups = (entry.capacity + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE;
                pass.set_bind_group(0, &entry.compute_bind_group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
        }

        // Between passes: clear alive_count (offset 4) and dead_count (offset 16).
        // Zeroes bytes 4..20, preserving vertex_count (6) and spawn_count.
        for entry in &active_buffers.entries {
            encoder.clear_buffer(&entry.counter_buffer, 4, Some(16));
        }

        // Pass 2: ALL compact dispatches (single pipeline set).
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("vfx_compact_pass"),
                ..default()
            });

            pass.set_pipeline(compact_pipeline);
            for entry in &active_buffers.entries {
                let workgroups = (entry.capacity + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE;
                pass.set_bind_group(0, &entry.compute_bind_group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
        }

        Ok(())
    }
}
