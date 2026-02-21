//! # bevy_vfx
//!
//! GPU-driven Niagara-style VFX system for Bevy.
//!
//! All particle simulation runs on the GPU via compute shaders. The CPU only
//! uploads emitter parameters and spawn counts — no particle data is ever
//! read back. This design scales to hundreds of thousands of particles.
//!
//! ## Quick Start
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_vfx::{VfxPlugin, VfxSystem};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(VfxPlugin)
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn((
//!         VfxSystem::default(),
//!         Transform::from_xyz(0.0, 1.0, 0.0),
//!     ));
//! }
//! ```

pub mod curve;
pub mod data;
pub mod gpu;
pub mod mesh_particles;
pub mod presets;
pub mod render;

// Re-export core types
pub use curve::{Curve, CurveKey, Gradient, GradientKey, Interp};
pub use data::*;

use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::prelude::*;
use bevy::render::render_graph::{RenderGraph, RenderGraphExt, ViewNodeRunner};
use bevy::render::render_resource::PipelineCache;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::{Render, RenderApp, RenderSystems};

use gpu::extract::{extract_vfx_systems, load_vfx_textures, VfxTextureCache};
use gpu::node::{VfxComputeLabel, VfxComputeNode};
use gpu::pipeline::VfxComputePipelines;
use gpu::prepare::prepare_vfx_buffers;
use render::billboard::{VfxBillboardLabel, VfxBillboardNode, VfxBillboardPipeline};

/// Embedded shader handles.
const COMMON_SHADER: &str = include_str!("shaders/common.wgsl");
const SPAWN_SHADER_SRC: &str = include_str!("shaders/spawn.wgsl");
const UPDATE_SHADER_SRC: &str = include_str!("shaders/update.wgsl");
const COMPACT_SHADER_SRC: &str = include_str!("shaders/compact.wgsl");
const BILLBOARD_SHADER_SRC: &str = include_str!("shaders/billboard.wgsl");

/// Auto-insert `VfxStartTime` on VFX systems that don't have one yet.
fn vfx_init_start_time(
    mut commands: Commands,
    time: Res<Time>,
    query: Query<Entity, (With<VfxSystem>, Without<VfxStartTime>)>,
) {
    let t = time.elapsed_secs();
    for entity in &query {
        commands.entity(entity).insert(VfxStartTime(t));
    }
}

/// Handle `VfxRestart`: despawn CPU particles, reset emitter state, remove
/// start time (will be re-inserted next frame), and remove the marker.
fn vfx_handle_restart(
    mut commands: Commands,
    mut query: Query<(Entity, Option<&mut mesh_particles::MeshParticleStates>), With<VfxRestart>>,
) {
    for (entity, mesh_states) in &mut query {
        // Reset CPU mesh particle state: despawn all live particle entities
        if let Some(mut states) = mesh_states {
            for state in &mut states.entries {
                for p in state.particles.drain(..) {
                    commands.entity(p.entity).try_despawn();
                }
                state.spawn_accumulator = 0.0;
                state.burst_cycle = 0;
                state.burst_timer = 0.0;
                state.once_fired = false;
            }
        }
        // Remove start time so it gets re-initialized next frame
        commands
            .entity(entity)
            .remove::<VfxRestart>()
            .remove::<VfxStartTime>();
    }
}

/// Main VFX plugin. Registers types, compute pipelines, and the render graph node.
pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        // Register reflectable types
        app.register_type::<VfxSystem>()
            .register_type::<EmitterDef>()
            .register_type::<SpawnModule>()
            .register_type::<InitModule>()
            .register_type::<UpdateModule>()
            .register_type::<RenderModule>()
            .register_type::<SimSpace>()
            .register_type::<VfxAlphaMode>()
            .register_type::<ScalarRange>()
            .register_type::<ShapeEmitter>()
            .register_type::<VelocityMode>()
            .register_type::<ColorSource>()
            .register_type::<KillShape>()
            .register_type::<BillboardConfig>()
            .register_type::<BillboardOrient>()
            .register_type::<FlipbookConfig>()
            .register_type::<RibbonConfig>()
            .register_type::<RibbonTextureMode>()
            .register_type::<MeshShape>()
            .register_type::<MeshParticleConfig>()
            .register_type::<OrientMode>()
            .register_type::<VfxParam>()
            .register_type::<VfxParamValue>()
            .register_type::<Curve<f32>>()
            .register_type::<CurveKey<f32>>()
            .register_type::<Gradient>()
            .register_type::<GradientKey>()
            .register_type::<Interp>()
            .init_resource::<VfxLibrary>()
            .init_resource::<VfxTextureCache>()
            .init_resource::<mesh_particles::MeshParticleAssets>()
            .add_systems(PostUpdate, load_vfx_textures)
            .add_systems(
                Update,
                (
                    vfx_init_start_time,
                    vfx_handle_restart,
                    mesh_particles::auto_insert_mesh_particle_state,
                    mesh_particles::cpu_mesh_particle_spawn,
                    mesh_particles::cpu_mesh_particle_update,
                    mesh_particles::cpu_mesh_particle_sync,
                    mesh_particles::cpu_mesh_particle_uv_scroll,
                    mesh_particles::cpu_mesh_particle_color_sync,
                    mesh_particles::cpu_mesh_particle_cleanup,
                )
                    .chain(),
            );

        // Set up render app if available
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<gpu::buffers::ActiveEmitterBuffers>()
                .init_resource::<gpu::extract::ExtractedVfxData>()
                .init_resource::<gpu::prepare::EmitterBufferCache>();
            render_app.add_systems(
                bevy::render::ExtractSchedule,
                extract_vfx_systems,
            );
            render_app.add_systems(
                Render,
                prepare_vfx_buffers.in_set(RenderSystems::Prepare),
            );
        }
    }

    fn finish(&self, app: &mut App) {
        // Load shaders into main world's Assets<Shader> (render world doesn't have this resource).
        // Compute shaders prepend common.wgsl; billboard shader is self-contained (uses
        // plain u32 counters instead of atomic<u32>, compatible with var<storage, read>).
        let spawn_src = format!("{}\n{}", COMMON_SHADER, SPAWN_SHADER_SRC);
        let update_src = format!("{}\n{}", COMMON_SHADER, UPDATE_SHADER_SRC);
        let compact_src = format!("{}\n{}", COMMON_SHADER, COMPACT_SHADER_SRC);

        let (spawn_shader, update_shader, compact_shader, billboard_shader) = {
            let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
            (
                shaders.add(Shader::from_wgsl(spawn_src, "vfx_spawn.wgsl")),
                shaders.add(Shader::from_wgsl(update_src, "vfx_update.wgsl")),
                shaders.add(Shader::from_wgsl(compact_src, "vfx_compact.wgsl")),
                shaders.add(Shader::from_wgsl(
                    BILLBOARD_SHADER_SRC.to_string(),
                    "vfx_billboard.wgsl",
                )),
            )
        };

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        let render_device = render_app.world().resource::<RenderDevice>().clone();
        let render_queue = render_app.world().resource::<RenderQueue>().clone();

        // Create compute pipelines (spawn, update, compact passes)
        let compute_pipelines = {
            let pipeline_cache = render_app.world().resource::<PipelineCache>();
            VfxComputePipelines::new(
                &render_device,
                &pipeline_cache,
                spawn_shader,
                update_shader,
                compact_shader,
            )
        };
        render_app.insert_resource(compute_pipelines);

        // Create billboard render pipeline (needs queue to initialize fallback texture)
        let billboard_pipeline = {
            let pipeline_cache = render_app.world().resource::<PipelineCache>();
            VfxBillboardPipeline::new(&render_device, &render_queue, pipeline_cache, billboard_shader)
        };
        render_app.insert_resource(billboard_pipeline);

        // Add compute node to top-level render graph
        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        render_graph.add_node(VfxComputeLabel, VfxComputeNode);

        // Add billboard draw node to Core3d sub-graph (after transparent pass, before EndMainPass)
        render_app
            .add_render_graph_node::<ViewNodeRunner<VfxBillboardNode>>(
                Core3d,
                VfxBillboardLabel,
            )
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::MainTransparentPass,
                    VfxBillboardLabel,
                    Node3d::EndMainPass,
                ),
            );
    }
}
