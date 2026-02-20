// Compact pass: rebuild the alive index list from the particle buffer.
//
// Scans all particles — those with age < lifetime are alive and get added
// to the alive buffer. Updates alive_count in the counter buffer.
// Also resets spawn_count to 0 for the next frame.

@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(1) var<storage, read_write> alive: array<u32>;
@group(0) @binding(2) var<storage, read_write> dead: array<u32>;
@group(0) @binding(3) var<storage, read_write> counters: EmitterCounters;
@group(0) @binding(4) var<uniform> params: EmitterParams;

// Counter reset (alive_count, dead_count) is done via clear_buffer on the CPU
// between the spawn+update pass and this compact pass, ensuring correct
// cross-workgroup synchronization without storageBarrier().
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let total = arrayLength(&particles);

    if idx >= total {
        return;
    }

    let p = particles[idx];

    // Uninitialized particles (lifetime == 0) are dead
    if p.lifetime <= 0.0 {
        let dead_idx = atomicAdd(&counters.dead_count, 1u);
        dead[dead_idx] = idx;
        return;
    }

    if p.age < p.lifetime {
        // Alive: add to alive buffer
        let alive_idx = atomicAdd(&counters.alive_count, 1u);
        alive[alive_idx] = idx;
    } else {
        // Dead: add to dead buffer
        let dead_idx = atomicAdd(&counters.dead_count, 1u);
        dead[dead_idx] = idx;
    }
}
