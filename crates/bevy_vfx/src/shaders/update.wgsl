// Update pass: apply forces, drag, noise, kill expired particles.
//
// One thread per particle slot. Only processes alive particles (age < lifetime).

@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(1) var<storage, read_write> alive: array<u32>;
@group(0) @binding(2) var<storage, read_write> dead: array<u32>;
@group(0) @binding(3) var<storage, read_write> counters: EmitterCounters;
@group(0) @binding(4) var<uniform> params: EmitterParams;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= arrayLength(&particles) {
        return;
    }

    var p = particles[idx];

    // Skip dead particles
    if p.age >= p.lifetime && p.lifetime > 0.0 {
        return;
    }

    // Skip uninitialized particles (lifetime == 0 means never spawned)
    if p.lifetime <= 0.0 {
        return;
    }

    let dt = params.dt;

    // Increment age
    p.age += dt;

    // Kill expired
    if p.age >= p.lifetime {
        // Push to dead buffer
        let dead_idx = atomicAdd(&counters.dead_count, 1u);
        dead[dead_idx] = idx;
        particles[idx] = p;
        return;
    }

    let normalized_life = p.age / p.lifetime;

    // For Local mode, position-relative modifiers use the emitter's world position
    // as their reference origin instead of world (0,0,0). Axis directions are also
    // transformed by the emitter's orientation.
    var ref_origin = vec3<f32>(0.0);
    if params.sim_space == 1u {
        ref_origin = params.emitter_transform[3].xyz;
    }

    // --- Apply update modules based on flags ---

    // Gravity (always world-space direction)
    if (params.flags & FLAG_GRAVITY) != 0u {
        p.velocity += params.gravity * dt;
    }

    // Constant force (always world-space direction)
    if (params.flags & FLAG_CONSTANT_FORCE) != 0u {
        p.velocity += params.constant_force * dt;
    }

    // Drag
    if (params.flags & FLAG_DRAG) != 0u {
        let speed = length(p.velocity);
        if speed > 0.001 {
            let drag_force = params.drag * speed;
            let decel = min(drag_force * dt, speed);
            p.velocity -= normalize(p.velocity) * decel;
        }
    }

    // Radial acceleration (origin offset by emitter position in Local mode)
    if (params.flags & FLAG_RADIAL_ACCEL) != 0u {
        let origin = params.radial_accel_origin + ref_origin;
        let to_particle = p.position - origin;
        let dist = length(to_particle);
        if dist > 0.001 {
            let dir = to_particle / dist;
            p.velocity += dir * params.radial_accel_value * dt;
        }
    }

    // Tangent acceleration (origin + axis transformed for Local mode)
    if (params.flags & FLAG_TANGENT_ACCEL) != 0u {
        let origin = params.tangent_accel_origin + ref_origin;
        let to_particle = p.position - origin;
        var axis = normalize(params.tangent_accel_axis);
        if params.sim_space == 1u {
            axis = normalize((params.emitter_transform * vec4<f32>(axis, 0.0)).xyz);
        }
        let tangent = cross(axis, to_particle);
        let tang_len = length(tangent);
        if tang_len > 0.001 {
            p.velocity += (tangent / tang_len) * params.tangent_accel_value * dt;
        }
    }

    // Orbit (center + axis transformed for Local mode)
    if (params.flags & FLAG_ORBIT) != 0u {
        var axis = normalize(params.orbit_axis);
        if params.sim_space == 1u {
            axis = normalize((params.emitter_transform * vec4<f32>(axis, 0.0)).xyz);
        }
        let angle = params.orbit_speed * dt;
        let cos_a = cos(angle);
        let sin_a = sin(angle);
        // Rodrigues' rotation around ref_origin
        let rel = p.position - ref_origin;
        let rotated = rel * cos_a + cross(axis, rel) * sin_a + axis * dot(axis, rel) * (1.0 - cos_a);
        p.position = ref_origin + rotated;
        if params.orbit_radius_decay > 0.0 {
            let dist = length(p.position - ref_origin);
            if dist > 0.001 {
                p.position = ref_origin + (p.position - ref_origin) * (1.0 - params.orbit_radius_decay * dt);
            }
        }
    }

    // Attract (target offset by emitter position in Local mode)
    if (params.flags & FLAG_ATTRACT) != 0u {
        let attract_pos = params.attract_target + ref_origin;
        let to_target = attract_pos - p.position;
        let dist = length(to_target);
        if dist > 0.001 {
            let dir = to_target / dist;
            let strength = params.attract_strength / max(pow(dist, params.attract_falloff), 0.001);
            p.velocity += dir * strength * dt;
        }
    }

    // Noise (simplified curl noise approximation)
    if (params.flags & FLAG_NOISE) != 0u {
        var seed = p.seed + bitcast<u32>(params.time * 100.0);
        let noise_offset = vec3<f32>(
            rand_float(&seed) * 2.0 - 1.0,
            rand_float(&seed) * 2.0 - 1.0,
            rand_float(&seed) * 2.0 - 1.0,
        );
        p.velocity += noise_offset * params.noise_strength * dt;
    }

    // Integrate position
    p.position += p.velocity * dt;

    // Kill zone (center offset by emitter position in Local mode)
    if (params.flags & FLAG_KILL_ZONE) != 0u {
        let kz_center = params.kill_zone_center + ref_origin;
        var inside = false;
        switch params.kill_zone_type {
            case 1u: { // Sphere
                let dist = length(p.position - kz_center);
                inside = dist < params.kill_zone_param.x;
            }
            case 2u: { // Box
                let d = abs(p.position - kz_center);
                inside = d.x < params.kill_zone_param.x && d.y < params.kill_zone_param.y && d.z < params.kill_zone_param.z;
            }
            default: {}
        }
        let should_kill = select(inside, !inside, params.kill_zone_invert != 0u);
        if should_kill {
            p.age = p.lifetime; // Mark as dead
            let dead_idx = atomicAdd(&counters.dead_count, 1u);
            dead[dead_idx] = idx;
            particles[idx] = p;
            return;
        }
    }

    // Size over life
    if (params.flags & FLAG_SIZE_BY_LIFE) != 0u {
        p.size = sample_curve(params.size_curve, params.size_curve_count, normalized_life);
    }

    // Size by speed
    if (params.flags & FLAG_SIZE_BY_SPEED) != 0u {
        let speed = length(p.velocity);
        let t = clamp(
            (speed - params.size_by_speed_min_speed) / max(params.size_by_speed_max_speed - params.size_by_speed_min_speed, 0.001),
            0.0, 1.0
        );
        p.size = mix(params.size_by_speed_min_size, params.size_by_speed_max_size, t);
    }

    // Rotate by velocity
    if (params.flags & FLAG_ROTATE_BY_VELOCITY) != 0u {
        let speed = length(p.velocity.xz);
        if speed > 0.001 {
            p.rotation = atan2(p.velocity.x, p.velocity.z);
        }
    }

    // Color over life (gradient sampling)
    // Packed as 2 vec4 per key: [time,r,g,b] at index i*2, [a,0,0,0] at index i*2+1
    if (params.flags & FLAG_COLOR_BY_LIFE) != 0u {
        let count = params.color_gradient_count;
        if count > 0u {
            if count == 1u {
                let trgb = params.color_gradient[0];
                let a = params.color_gradient[1].x;
                p.color = vec4<f32>(trgb.y, trgb.z, trgb.w, a);
            } else {
                // Find the two keys surrounding normalized_life
                var found = false;
                for (var i = 0u; i < count - 1u; i++) {
                    let a_trgb = params.color_gradient[i * 2u];
                    let b_trgb = params.color_gradient[(i + 1u) * 2u];
                    if normalized_life >= a_trgb.x && normalized_life <= b_trgb.x {
                        let span = b_trgb.x - a_trgb.x;
                        var frac = 0.0;
                        if span > 0.0001 {
                            frac = (normalized_life - a_trgb.x) / span;
                        }
                        let a_alpha = params.color_gradient[i * 2u + 1u].x;
                        let b_alpha = params.color_gradient[(i + 1u) * 2u + 1u].x;
                        p.color = vec4<f32>(
                            mix(a_trgb.yzw, b_trgb.yzw, vec3<f32>(frac)),
                            mix(a_alpha, b_alpha, frac),
                        );
                        found = true;
                        break;
                    }
                }
                if !found {
                    // Past the last key
                    let last_trgb = params.color_gradient[(count - 1u) * 2u];
                    let last_a = params.color_gradient[(count - 1u) * 2u + 1u].x;
                    p.color = vec4<f32>(last_trgb.y, last_trgb.z, last_trgb.w, last_a);
                }
            }
        }
    }

    // Premultiply alpha and encode blend mode
    // Pipeline uses premultiplied alpha blending (src=ONE, dst=ONE_MINUS_SRC_ALPHA).
    // - Blend:    premultiply rgb, keep alpha → standard alpha blending
    // - Additive: premultiply rgb, alpha=0   → dst factor becomes 1.0 (additive)
    // - Opaque:   premultiply rgb, alpha=1   → dst factor becomes 0.0 (opaque)
    let a = p.color.a;
    p.color = vec4<f32>(p.color.rgb * a, a);
    if params.alpha_mode == 1u { // Additive
        p.color.a = 0.0;
    } else if params.alpha_mode == 4u { // Opaque
        p.color.a = 1.0;
    }

    // Write back
    particles[idx] = p;
}
