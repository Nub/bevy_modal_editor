// Shared GPU structs, RNG, and curve/gradient sampling functions.

// Must match GpuParticle in buffers.rs
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

// Must match GpuEmitterCounters in buffers.rs
// First 16 bytes form DrawIndirectArgs: vertex_count, instance_count, first_vertex, first_instance
struct EmitterCounters {
    vertex_count: u32,
    alive_count: atomic<u32>,
    first_vertex: u32,
    first_instance: u32,
    dead_count: atomic<u32>,
    spawn_count: u32,
    billboard_flags: u32,
    _pad1: u32,
};

// Maximum curve/gradient keys (must match MAX_CURVE_KEYS / MAX_GRADIENT_KEYS)
const MAX_CURVE_KEYS: u32 = 8u;
const MAX_GRADIENT_KEYS: u32 = 8u;

// Must match GpuEmitterParams in buffers.rs
struct EmitterParams {
    shape_type: u32,
    shape_param0: vec3<f32>,
    shape_param1: vec3<f32>,
    shape_radius_min: f32,
    shape_radius_max: f32,

    velocity_mode: u32,
    velocity_param0: vec3<f32>,
    velocity_speed_min: f32,
    velocity_speed_max: f32,
    velocity_cone_angle: f32,

    lifetime_min: f32,
    lifetime_max: f32,

    init_color: vec4<f32>,

    init_size_min: f32,
    init_size_max: f32,

    init_rotation_min: f32,
    init_rotation_max: f32,

    inherit_velocity_ratio: f32,

    gravity: vec3<f32>,
    constant_force: vec3<f32>,

    drag: f32,

    noise_strength: f32,
    noise_frequency: f32,
    noise_scroll: vec3<f32>,

    orbit_axis: vec3<f32>,
    orbit_speed: f32,
    orbit_radius_decay: f32,

    attract_target: vec3<f32>,
    attract_strength: f32,
    attract_falloff: f32,

    kill_zone_type: u32,
    kill_zone_center: vec3<f32>,
    kill_zone_param: vec3<f32>,
    kill_zone_invert: u32,

    tangent_accel_origin: vec3<f32>,
    tangent_accel_axis: vec3<f32>,
    tangent_accel_value: f32,

    radial_accel_origin: vec3<f32>,
    radial_accel_value: f32,

    flags: u32,

    size_curve_count: u32,
    size_curve: array<vec4<f32>, 8>, // xy = [time, value], zw = padding

    color_gradient_count: u32,
    color_gradient: array<vec4<f32>, 16>, // 2 vec4 per key: [time,r,g,b], [a,0,0,0]

    size_by_speed_min_speed: f32,
    size_by_speed_max_speed: f32,
    size_by_speed_min_size: f32,
    size_by_speed_max_size: f32,

    orient_mode: u32,
    alpha_mode: u32,
    sim_space: u32,

    dt: f32,
    time: f32,

    emitter_transform: mat4x4<f32>,

    emitter_seed: u32,
    _pad: vec2<f32>,
};

// Bit flags for active update modules
const FLAG_GRAVITY: u32         = 1u;
const FLAG_CONSTANT_FORCE: u32  = 2u;
const FLAG_DRAG: u32            = 4u;
const FLAG_NOISE: u32           = 8u;
const FLAG_ORBIT: u32           = 16u;
const FLAG_ATTRACT: u32         = 32u;
const FLAG_KILL_ZONE: u32       = 64u;
const FLAG_SIZE_BY_LIFE: u32    = 128u;
const FLAG_COLOR_BY_LIFE: u32   = 256u;
const FLAG_SIZE_BY_SPEED: u32   = 512u;
const FLAG_ROTATE_BY_VELOCITY: u32 = 1024u;
const FLAG_TANGENT_ACCEL: u32   = 2048u;
const FLAG_RADIAL_ACCEL: u32    = 4096u;

// ---------------------------------------------------------------------------
// PCG random number generator
// ---------------------------------------------------------------------------

fn pcg_hash(input: u32) -> u32 {
    var state = input * 747796405u + 2891336453u;
    var word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// Returns a random float in [0, 1)
fn rand_float(seed: ptr<function, u32>) -> f32 {
    *seed = pcg_hash(*seed);
    return f32(*seed) / 4294967296.0;
}

// Returns a random float in [min, max)
fn rand_range(seed: ptr<function, u32>, min_val: f32, max_val: f32) -> f32 {
    return min_val + rand_float(seed) * (max_val - min_val);
}

// Returns a random unit vector on the unit sphere
fn rand_unit_sphere(seed: ptr<function, u32>) -> vec3<f32> {
    let theta = rand_float(seed) * 6.28318530718;
    let z = rand_range(seed, -1.0, 1.0);
    let r = sqrt(1.0 - z * z);
    return vec3<f32>(r * cos(theta), r * sin(theta), z);
}

// Returns a random point inside a unit sphere
fn rand_in_sphere(seed: ptr<function, u32>) -> vec3<f32> {
    let dir = rand_unit_sphere(seed);
    let r = pow(rand_float(seed), 1.0 / 3.0);
    return dir * r;
}

// ---------------------------------------------------------------------------
// Curve sampling (linear interpolation between keyframes)
// ---------------------------------------------------------------------------

fn sample_curve(keys: array<vec4<f32>, 8>, count: u32, t: f32) -> f32 {
    // keys[i].x = time, keys[i].y = value (zw unused padding)
    if count == 0u {
        return 1.0;
    }
    if count == 1u {
        return keys[0].y;
    }

    let t_clamped = clamp(t, 0.0, 1.0);

    if t_clamped <= keys[0].x {
        return keys[0].y;
    }
    if t_clamped >= keys[count - 1u].x {
        return keys[count - 1u].y;
    }

    for (var i = 0u; i < count - 1u; i++) {
        let a = keys[i];
        let b = keys[i + 1u];
        if t_clamped >= a.x && t_clamped <= b.x {
            let span = b.x - a.x;
            if span < 0.0001 {
                return a.y;
            }
            let frac = (t_clamped - a.x) / span;
            return mix(a.y, b.y, frac);
        }
    }

    return keys[count - 1u].y;
}
