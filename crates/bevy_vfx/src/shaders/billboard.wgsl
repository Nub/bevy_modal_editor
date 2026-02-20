// Billboard vertex/fragment shader for VFX particles.
//
// Self-contained — does NOT include common.wgsl (which uses atomic types
// incompatible with read-only storage buffers).
//
// Renders camera-facing quads via instanced draw. The vertex shader reads
// particle data from ParticleBuffer[AliveBuffer[instance_id]], offsets
// quad corners along camera right/up vectors, and projects to clip space.

// ---- Structs (must match GPU layout in buffers.rs / common.wgsl) ----

struct Particle {
    position: vec3<f32>,
    age: f32,
    velocity: vec3<f32>,
    lifetime: f32,
    color: vec4<f32>,
    size: f32,
    rotation: f32,
    seed: u32,
    _pad: u32,
};

// Read-only version of EmitterCounters (plain u32, not atomic<u32>).
// Same memory layout as the atomic version in common.wgsl.
// First 16 bytes match DrawIndirectArgs layout.
struct ReadCounters {
    vertex_count: u32,
    alive_count: u32,
    first_vertex: u32,
    first_instance: u32,
    dead_count: u32,
    spawn_count: u32,
    billboard_flags: u32,
    _pad1: u32,
};

struct CameraUniforms {
    view_proj: mat4x4<f32>,
    camera_position: vec3<f32>,
    _pad0: f32,
    camera_right: vec3<f32>,
    _pad1: f32,
    camera_up: vec3<f32>,
    _pad2: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

// ---- Bindings ----

@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> alive: array<u32>;
@group(0) @binding(2) var<storage, read> counter: ReadCounters;
@group(0) @binding(3) var particle_texture: texture_2d<f32>;
@group(0) @binding(4) var particle_sampler: sampler;

@group(1) @binding(0) var<uniform> camera: CameraUniforms;

// ---- Procedural quad geometry (2 triangles = 6 vertices) ----

const QUAD_POSITIONS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-0.5, -0.5),
    vec2<f32>( 0.5, -0.5),
    vec2<f32>( 0.5,  0.5),
    vec2<f32>(-0.5, -0.5),
    vec2<f32>( 0.5,  0.5),
    vec2<f32>(-0.5,  0.5),
);

const QUAD_UVS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 0.0),
);

// ---- Vertex shader ----

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_id: u32,
    @builtin(instance_index) instance_id: u32,
) -> VertexOutput {
    var out: VertexOutput;

    // Cull instances beyond alive count (shader-side culling)
    if instance_id >= counter.alive_count {
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        out.color = vec4<f32>(0.0);
        out.uv = vec2<f32>(0.0);
        return out;
    }

    let particle_idx = alive[instance_id];
    let p = particles[particle_idx];

    // Quad corner in local space (scaled by particle size)
    let local_pos = QUAD_POSITIONS[vertex_id] * p.size;

    // Apply per-particle rotation
    let cos_r = cos(p.rotation);
    let sin_r = sin(p.rotation);
    let rotated = vec2<f32>(
        local_pos.x * cos_r - local_pos.y * sin_r,
        local_pos.x * sin_r + local_pos.y * cos_r,
    );

    // Billboard: offset in camera right/up directions
    let world_pos = p.position
        + camera.camera_right * rotated.x
        + camera.camera_up * rotated.y;

    // Project world position to clip space
    out.position = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.color = p.color;
    out.uv = QUAD_UVS[vertex_id];

    return out;
}

// ---- Fragment shader ----

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (counter.billboard_flags & 1u) != 0u {
        // Textured particle — sample texture and multiply with particle color.
        let tex = textureSample(particle_texture, particle_sampler, in.uv);
        return in.color * tex;
    } else {
        // Soft circular particle (no texture bound).
        let dist = length(in.uv - vec2<f32>(0.5));
        let mask = 1.0 - smoothstep(0.4, 0.5, dist);
        return in.color * mask;
    }
}
