// Spawn pass: initialize new particles from the dead pool.
//
// Each thread pops one index from DeadBuffer (atomically), then initializes
// the particle at that index using InitModule parameters from EmitterParams.

@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(1) var<storage, read_write> alive: array<u32>;
@group(0) @binding(2) var<storage, read_write> dead: array<u32>;
@group(0) @binding(3) var<storage, read_write> counters: EmitterCounters;
@group(0) @binding(4) var<uniform> params: EmitterParams;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let thread_id = gid.x;

    // Only spawn up to spawn_count particles
    if thread_id >= counters.spawn_count {
        return;
    }

    // Atomically decrement dead count and get the index
    let dead_idx = atomicSub(&counters.dead_count, 1u);
    if dead_idx == 0u {
        // No more free slots
        atomicAdd(&counters.dead_count, 1u);
        return;
    }
    let slot = dead[dead_idx - 1u];

    // Initialize RNG seed from thread_id + time + per-emitter salt
    var seed = thread_id * 1973u + bitcast<u32>(params.time * 1000.0) + params.emitter_seed;

    // Initialize particle
    var p: Particle;
    p.age = 0.0;
    p.seed = seed;
    p._pad = 0u;

    // Lifetime
    p.lifetime = rand_range(&seed, params.lifetime_min, params.lifetime_max);

    // Position from shape emitter
    switch params.shape_type {
        case 0u: { // Point
            p.position = params.shape_param0;
        }
        case 1u: { // Sphere
            let dir = rand_unit_sphere(&seed);
            let r = rand_range(&seed, params.shape_radius_min, params.shape_radius_max);
            p.position = params.shape_param0 + dir * r;
        }
        case 2u: { // Box
            let center = params.shape_param0;
            let half = params.shape_param1;
            p.position = center + vec3<f32>(
                rand_range(&seed, -half.x, half.x),
                rand_range(&seed, -half.y, half.y),
                rand_range(&seed, -half.z, half.z),
            );
        }
        case 3u: { // Cone
            let angle = params.shape_param0.x;
            let radius = params.shape_param0.y;
            let height = params.shape_param0.z;
            let theta = rand_float(&seed) * 6.28318530718;
            let h = rand_float(&seed) * height;
            let r = radius * (1.0 - h / max(height, 0.001)) * rand_float(&seed);
            p.position = vec3<f32>(r * cos(theta), h, r * sin(theta));
        }
        case 4u: { // Circle
            let center = params.shape_param0;
            let axis = normalize(params.shape_param1);
            let r = rand_range(&seed, params.shape_radius_min, params.shape_radius_max);
            let theta = rand_float(&seed) * 6.28318530718;
            // Build perpendicular vectors to axis
            var tangent: vec3<f32>;
            if abs(axis.y) < 0.99 {
                tangent = normalize(cross(axis, vec3<f32>(0.0, 1.0, 0.0)));
            } else {
                tangent = normalize(cross(axis, vec3<f32>(1.0, 0.0, 0.0)));
            }
            let bitangent = cross(axis, tangent);
            p.position = center + (tangent * cos(theta) + bitangent * sin(theta)) * r;
        }
        case 5u: { // Edge
            let t = rand_float(&seed);
            p.position = mix(params.shape_param0, params.shape_param1, vec3<f32>(t));
        }
        default: {
            p.position = vec3<f32>(0.0);
        }
    }

    // Velocity (computed in local space BEFORE coordinate-space transform)
    switch params.velocity_mode {
        case 0u: { // Radial
            let dir = normalize(p.position - params.velocity_param0);
            let spd = rand_range(&seed, params.velocity_speed_min, params.velocity_speed_max);
            p.velocity = dir * spd;
        }
        case 1u: { // Directional
            let dir = normalize(params.velocity_param0);
            let spd = rand_range(&seed, params.velocity_speed_min, params.velocity_speed_max);
            p.velocity = dir * spd;
        }
        case 2u: { // Tangent
            let axis = normalize(params.velocity_param0);
            let to_particle = p.position;
            let tangent_dir = normalize(cross(axis, to_particle));
            let spd = rand_range(&seed, params.velocity_speed_min, params.velocity_speed_max);
            p.velocity = tangent_dir * spd;
        }
        case 3u: { // Cone
            let dir = normalize(params.velocity_param0);
            let cone_angle = params.velocity_cone_angle;
            // Random direction within cone
            let rand_dir = rand_unit_sphere(&seed);
            let blended = normalize(mix(dir, rand_dir, cone_angle / 3.14159));
            let spd = rand_range(&seed, params.velocity_speed_min, params.velocity_speed_max);
            p.velocity = blended * spd;
        }
        case 4u: { // Random
            let dir = rand_unit_sphere(&seed);
            let spd = rand_range(&seed, params.velocity_speed_min, params.velocity_speed_max);
            p.velocity = dir * spd;
        }
        default: {
            p.velocity = vec3<f32>(0.0);
        }
    }

    // Apply coordinate-space transform (after both position and velocity are in local space)
    if params.sim_space == 1u {
        // Local: transform position and velocity into world space
        p.position = (params.emitter_transform * vec4<f32>(p.position, 1.0)).xyz;
        p.velocity = (params.emitter_transform * vec4<f32>(p.velocity, 0.0)).xyz;
    } else {
        // World: offset spawn position by emitter location
        p.position += params.emitter_transform[3].xyz;
    }

    // Color
    p.color = params.init_color;

    // Size
    p.size = rand_range(&seed, params.init_size_min, params.init_size_max);

    // Rotation
    p.rotation = rand_range(&seed, params.init_rotation_min, params.init_rotation_max);

    // Store the particle
    particles[slot] = p;

    // Add to alive list
    let alive_idx = atomicAdd(&counters.alive_count, 1u);
    alive[alive_idx] = slot;
}
