//! GPU buffer types for particle simulation.
//!
//! All particle state lives on the GPU. The CPU never reads particle data back.

use bevy::prelude::*;
use bevy::render::render_resource::*;
use bevy::render::renderer::RenderDevice;
use bytemuck::{Pod, Zeroable};

use bevy::asset::AssetId;
use bevy::image::Image;

use crate::data::EmitterDef;

/// Maximum number of curve keyframes packed into the GPU params buffer.
pub const MAX_CURVE_KEYS: usize = 8;
/// Maximum number of gradient keys packed into the GPU params buffer.
pub const MAX_GRADIENT_KEYS: usize = 8;

// ---------------------------------------------------------------------------
// GPU-side particle struct (matches common.wgsl)
// ---------------------------------------------------------------------------

/// Per-particle data stored in the GPU particle buffer.
/// Must match the `Particle` struct in `common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuParticle {
    /// World-space position.
    pub position: [f32; 3],
    /// Age in seconds (incremented each frame).
    pub age: f32,
    /// World-space velocity.
    pub velocity: [f32; 3],
    /// Maximum lifetime in seconds.
    pub lifetime: f32,
    /// RGBA color (linear).
    pub color: [f32; 4],
    /// Uniform size (billboard radius).
    pub size: f32,
    /// Rotation in radians.
    pub rotation: f32,
    /// Per-particle RNG seed.
    pub seed: u32,
    /// Padding to align to 16 bytes.
    pub _pad: u32,
}

/// Counters used for indirect dispatch and draw.
/// Must match `EmitterCounters` in `common.wgsl`.
///
/// The first 16 bytes are laid out as DrawIndirectArgs so the counter buffer
/// can be passed directly to `draw_indirect()`:
///   vertex_count (6), instance_count (alive_count), first_vertex (0), first_instance (0)
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuEmitterCounters {
    /// Vertex count for draw_indirect (always 6 — one billboard quad).
    pub vertex_count: u32,
    /// Number of alive particles (also serves as instance_count for draw_indirect).
    pub alive_count: u32,
    /// First vertex for draw_indirect (always 0).
    pub first_vertex: u32,
    /// First instance for draw_indirect (always 0).
    pub first_instance: u32,
    /// Number of dead (free) slots.
    pub dead_count: u32,
    /// Number of particles to spawn this frame (written by CPU).
    pub spawn_count: u32,
    /// Billboard rendering flags (bit 0 = has_texture). Read by billboard fragment shader.
    pub billboard_flags: u32,
    /// Padding to 32 bytes.
    pub _pad1: u32,
}

/// Packed emitter parameters uploaded from CPU each frame (when changed).
/// Must match `EmitterParams` in `common.wgsl`.
///
/// WGSL uniform buffer alignment rules:
/// - vec3<f32> has alignment 16, size 12 (scalars can fill the 4-byte tail)
/// - array elements must have stride that's a multiple of 16
/// - mat4x4<f32> has alignment 16
/// - struct size is rounded up to the struct's alignment (16)
///
/// Total size: 896 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuEmitterParams {
    // -- Spawn shape -- (offset 0)
    pub shape_type: u32,            // 0
    pub _pad0: [u32; 3],           // 4: align to 16 for shape_param0
    pub shape_param0: [f32; 3],    // 16: center / start
    pub _pad1: f32,                // 28: align to 16 for shape_param1
    pub shape_param1: [f32; 3],    // 32: half_extents / end / axis
    pub shape_radius_min: f32,     // 44: fills vec3 tail
    pub shape_radius_max: f32,     // 48

    // -- Velocity -- (offset 52)
    pub velocity_mode: u32,        // 52
    pub _pad2: [f32; 2],          // 56: align to 16 for velocity_param0
    pub velocity_param0: [f32; 3], // 64: center / direction / axis
    pub velocity_speed_min: f32,   // 76: fills vec3 tail
    pub velocity_speed_max: f32,   // 80
    pub velocity_cone_angle: f32,  // 84

    // -- Lifetime -- (offset 88)
    pub lifetime_min: f32,         // 88
    pub lifetime_max: f32,         // 92

    // -- Init color -- (offset 96, 16-aligned)
    pub init_color: [f32; 4],     // 96

    // -- Init size -- (offset 112)
    pub init_size_min: f32,        // 112
    pub init_size_max: f32,        // 116

    // -- Init rotation -- (offset 120)
    pub init_rotation_min: f32,    // 120
    pub init_rotation_max: f32,    // 124

    // -- Inherit velocity -- (offset 128)
    pub inherit_velocity_ratio: f32, // 128
    pub _pad3: [f32; 3],          // 132: align to 16 for gravity

    // -- Update: gravity / constant force -- (offset 144)
    pub gravity: [f32; 3],        // 144
    pub _pad4: f32,               // 156: align to 16 for constant_force
    pub constant_force: [f32; 3], // 160
    pub drag: f32,                // 172: fills vec3 tail

    // -- Update: noise -- (offset 176)
    pub noise_strength: f32,       // 176
    pub noise_frequency: f32,      // 180
    pub _pad5: [f32; 2],          // 184: align to 16 for noise_scroll
    pub noise_scroll: [f32; 3],   // 192
    pub _pad6: f32,               // 204: align to 16 for orbit_axis

    // -- Update: orbit -- (offset 208)
    pub orbit_axis: [f32; 3],     // 208
    pub orbit_speed: f32,          // 220: fills vec3 tail
    pub orbit_radius_decay: f32,   // 224
    pub _pad7: [f32; 3],          // 228: align to 16 for attract_target

    // -- Update: attract -- (offset 240)
    pub attract_target: [f32; 3], // 240
    pub attract_strength: f32,     // 252: fills vec3 tail
    pub attract_falloff: f32,      // 256

    // -- Update: kill zone -- (offset 260)
    pub kill_zone_type: u32,       // 260
    pub _pad8: [f32; 2],          // 264: align to 16 for kill_zone_center
    pub kill_zone_center: [f32; 3], // 272
    pub _pad9: f32,               // 284: align to 16 for kill_zone_param
    pub kill_zone_param: [f32; 3], // 288
    pub kill_zone_invert: u32,     // 300: fills vec3 tail

    // -- Update: tangent accel -- (offset 304, 16-aligned)
    pub tangent_accel_origin: [f32; 3], // 304
    pub _pad10: f32,              // 316: align to 16 for tangent_accel_axis
    pub tangent_accel_axis: [f32; 3],   // 320
    pub tangent_accel_value: f32,  // 332: fills vec3 tail

    // -- Update: radial accel -- (offset 336, 16-aligned)
    pub radial_accel_origin: [f32; 3], // 336
    pub radial_accel_value: f32,   // 348: fills vec3 tail

    // -- Flags -- (offset 352)
    pub flags: u32,               // 352

    // -- Size curve -- (offset 356)
    pub size_curve_count: u32,     // 356
    pub _pad11: [f32; 2],         // 360: align to 16 for array
    pub size_curve: [[f32; 4]; MAX_CURVE_KEYS], // 368 (128 bytes)

    // -- Color gradient -- (offset 496)
    pub color_gradient_count: u32,  // 496
    pub _pad12: [u32; 3],         // 500: align to 16 for array
    pub color_gradient: [[f32; 4]; MAX_GRADIENT_KEYS * 2], // 512 (256 bytes)

    // -- Size by speed -- (offset 768)
    pub size_by_speed_min_speed: f32,  // 768
    pub size_by_speed_max_speed: f32,  // 772
    pub size_by_speed_min_size: f32,   // 776
    pub size_by_speed_max_size: f32,   // 780

    // -- Billboard orient -- (offset 784)
    pub orient_mode: u32,          // 784
    pub alpha_mode: u32,           // 788
    pub sim_space: u32,            // 792

    // -- Time -- (offset 796)
    pub dt: f32,                   // 796
    pub time: f32,                 // 800
    pub _pad13: [f32; 3],         // 804: align to 16 for mat4x4

    // -- Emitter transform -- (offset 816)
    pub emitter_transform: [[f32; 4]; 4], // 816 (64 bytes)

    // -- Emitter seed -- (offset 880)
    /// Per-emitter RNG salt so identically-configured emitters don't produce
    /// identical particle sequences. Derived from entity ID + emitter index.
    pub emitter_seed: u32,        // 880
    pub _pad_tail: [f32; 7],     // 884: padding to match WGSL struct size (912)
}

const _: () = assert!(std::mem::size_of::<GpuEmitterParams>() == 912);

/// Dynamic subset of GpuEmitterParams that changes every frame.
/// Written via partial buffer upload at DYNAMIC_PARAMS_OFFSET.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuDynamicParams {
    pub dt: f32,
    pub time: f32,
    pub _pad13: [f32; 3],
    pub emitter_transform: [[f32; 4]; 4],
}

/// Byte offset of the dynamic fields within GpuEmitterParams.
pub const DYNAMIC_PARAMS_OFFSET: u64 =
    std::mem::offset_of!(GpuEmitterParams, dt) as u64;

const _: () = assert!(std::mem::size_of::<GpuDynamicParams>() == 84);
const _: () = assert!(DYNAMIC_PARAMS_OFFSET == 796);

// Bit flags for active update modules
pub const FLAG_GRAVITY: u32 = 1 << 0;
pub const FLAG_CONSTANT_FORCE: u32 = 1 << 1;
pub const FLAG_DRAG: u32 = 1 << 2;
pub const FLAG_NOISE: u32 = 1 << 3;
pub const FLAG_ORBIT: u32 = 1 << 4;
pub const FLAG_ATTRACT: u32 = 1 << 5;
pub const FLAG_KILL_ZONE: u32 = 1 << 6;
pub const FLAG_SIZE_BY_LIFE: u32 = 1 << 7;
pub const FLAG_COLOR_BY_LIFE: u32 = 1 << 8;
pub const FLAG_SIZE_BY_SPEED: u32 = 1 << 9;
pub const FLAG_ROTATE_BY_VELOCITY: u32 = 1 << 10;
pub const FLAG_TANGENT_ACCEL: u32 = 1 << 11;
pub const FLAG_RADIAL_ACCEL: u32 = 1 << 12;

/// Billboard counter flag: particle has a texture bound (vs procedural circle).
pub const BILLBOARD_FLAG_HAS_TEXTURE: u32 = 1;

// ---------------------------------------------------------------------------
// Per-emitter GPU resource set
// ---------------------------------------------------------------------------

/// All GPU buffers for a single emitter instance.
pub struct EmitterBuffers {
    /// Storage buffer holding all particles (sized to capacity).
    pub particle_buffer: Buffer,
    /// Storage buffer of alive particle indices.
    pub alive_buffer: Buffer,
    /// Storage buffer of dead (free) particle indices.
    pub dead_buffer: Buffer,
    /// Storage buffer with counters (alive, dead, spawn).
    pub counter_buffer: Buffer,
    /// Uniform buffer with emitter parameters.
    pub params_buffer: Buffer,
    /// The particle capacity this buffer set was created for.
    pub capacity: u32,
    /// Cached compute bind group (created once, reused every frame).
    pub compute_bind_group: Option<BindGroup>,
    /// Cached billboard bind group (created once, reused every frame).
    pub billboard_bind_group: Option<BindGroup>,
    /// Hash of the last uploaded static params (for dirty tracking).
    /// When the EmitterDef changes, we re-upload the full params buffer.
    pub static_params_hash: u64,
    /// Cached EmitterDef for fast change detection (PartialEq comparison
    /// avoids re-packing 912-byte GPU params struct every frame).
    pub last_emitter_def: Option<EmitterDef>,
    /// The texture AssetId currently bound in the billboard bind group (None = fallback).
    pub bound_texture: Option<AssetId<Image>>,
    /// Whether the bound texture is the real GpuImage (true) or fallback (false, still loading).
    pub bound_texture_resolved: bool,
}

/// Resource collecting all active emitter bind group data for the render graph node.
/// Populated during the Prepare phase; consumed by the compute node.
#[derive(Resource, Default)]
pub struct ActiveEmitterBuffers {
    pub entries: Vec<ActiveEmitterEntry>,
}

/// One entry per active emitter — pre-cached bind groups and draw metadata.
pub struct ActiveEmitterEntry {
    pub capacity: u32,
    /// Counter buffer reference (needed for clear_buffer between compute passes).
    pub counter_buffer: Buffer,
    /// Pre-cached compute bind group (avoids per-frame creation).
    pub compute_bind_group: BindGroup,
    /// Pre-cached billboard bind group (avoids per-frame creation).
    pub billboard_bind_group: BindGroup,
}

/// Camera uniforms for the billboard vertex shader.
/// Uploaded once per frame (shared across all emitters).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct VfxCameraUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub camera_position: [f32; 3],
    pub _pad0: f32,
    pub camera_right: [f32; 3],
    pub _pad1: f32,
    pub camera_up: [f32; 3],
    pub _pad2: f32,
}

impl EmitterBuffers {
    /// Create a new buffer set for the given capacity.
    pub fn new(device: &RenderDevice, capacity: u32) -> Self {
        let particle_size = std::mem::size_of::<GpuParticle>() as u64;
        let index_size = std::mem::size_of::<u32>() as u64;
        let counter_size = std::mem::size_of::<GpuEmitterCounters>() as u64;
        let params_size = std::mem::size_of::<GpuEmitterParams>() as u64;

        let particle_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vfx_particle_buffer"),
            size: particle_size * capacity as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let alive_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vfx_alive_buffer"),
            size: index_size * capacity as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let dead_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vfx_dead_buffer"),
            size: index_size * capacity as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let counter_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vfx_counter_buffer"),
            size: counter_size,
            usage: BufferUsages::STORAGE
                | BufferUsages::COPY_DST
                | BufferUsages::INDIRECT,
            mapped_at_creation: false,
        });

        let params_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vfx_params_buffer"),
            size: params_size,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            particle_buffer,
            alive_buffer,
            dead_buffer,
            counter_buffer,
            params_buffer,
            capacity,
            compute_bind_group: None,
            billboard_bind_group: None,
            static_params_hash: 0,
            last_emitter_def: None,
            bound_texture: None,
            bound_texture_resolved: false,
        }
    }
}
