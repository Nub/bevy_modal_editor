// JFA Step Compute Shader
// One pass of the Jump Flood Algorithm
// Samples 9 neighbors at step_size offset and propagates closest seed

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rg16unorm, write>;

struct JfaParams {
    step_size: f32,
    _padding1: f32,
    _padding2: f32,
    _padding3: f32,
};

@group(0) @binding(2) var<uniform> params: JfaParams;

// Invalid seed marker - 0.0 works since valid UVs are at pixel centers (always > 0)
const INVALID_SEED: vec2<f32> = vec2<f32>(0.0, 0.0);

// Check if a seed coordinate is valid (valid UVs are at pixel centers, always > 0)
fn is_valid_seed(seed: vec2<f32>) -> bool {
    return seed.x > 0.0;
}

// Calculate squared distance from pixel UV to seed UV (in pixel space)
fn seed_distance_sq(pixel_uv: vec2<f32>, seed_uv: vec2<f32>, tex_size: vec2<f32>) -> f32 {
    let diff = (pixel_uv - seed_uv) * tex_size;
    return dot(diff, diff);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let tex_size_i = vec2<i32>(textureDimensions(input_texture));

    // Bounds check
    if global_id.x >= u32(tex_size_i.x) || global_id.y >= u32(tex_size_i.y) {
        return;
    }

    let coord = vec2<i32>(global_id.xy);
    let uv = (vec2<f32>(global_id.xy) + 0.5) / tex_size;

    let step_size = i32(params.step_size);

    var best_seed = INVALID_SEED;
    var best_dist_sq = 1e20;

    // Sample 9 neighbors: self + 8 directions at step_size offset
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            let sample_coord = coord + vec2<i32>(dx, dy) * step_size;

            // Skip out-of-bounds samples
            if sample_coord.x < 0 || sample_coord.x >= tex_size_i.x ||
               sample_coord.y < 0 || sample_coord.y >= tex_size_i.y {
                continue;
            }

            let seed = textureLoad(input_texture, sample_coord, 0).xy;

            if is_valid_seed(seed) {
                let dist_sq = seed_distance_sq(uv, seed, tex_size);
                if dist_sq < best_dist_sq {
                    best_dist_sq = dist_sq;
                    best_seed = seed;
                }
            }
        }
    }

    textureStore(output_texture, coord, vec4<f32>(best_seed, 0.0, 0.0));
}
