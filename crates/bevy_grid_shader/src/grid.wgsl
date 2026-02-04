#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings::view,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

struct GridMaterialUniform {
    line_color: vec4<f32>,
    major_line_color: vec4<f32>,
    line_width: f32,
    major_line_width: f32,
    grid_scale: f32,
    major_line_every: u32,
    axes: u32,
    fade_distance: f32,
    fade_strength: f32,
    _padding: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> grid: GridMaterialUniform;

// Axis flags
const AXIS_XZ: u32 = 1u;
const AXIS_XY: u32 = 2u;
const AXIS_YZ: u32 = 4u;

// Result of grid line calculation
struct GridResult {
    minor: f32,  // Minor line intensity
    major: f32,  // Major line intensity
}

// Calculate anti-aliased grid lines for a 2D position
// Returns separate intensities for minor and major lines
fn grid_line_2d(pos: vec2<f32>) -> GridResult {
    let scaled = pos * grid.grid_scale;

    // Calculate screen-space derivatives (world units per pixel)
    let derivative = fwidth(scaled);

    // Distance to nearest grid line in world-space grid units
    // The 0.4999 is to fudge square numbers from causing a zfighting like problem; It should be exceedingly rare to magically align with this number to see it manifest
    let grid_pos = abs(fract(scaled - 0.49999) - 0.5);

    // Convert distance to screen pixels
    let dist_pixels = grid_pos / derivative;

    // Minor lines
    let half_width = grid.line_width * 0.5;
    let minor_aa = 1.0 - smoothstep(half_width - 0.5, half_width + 0.5, min(dist_pixels.x, dist_pixels.y));

    // Major lines (every N grid cells)
    let major_scale = f32(grid.major_line_every);
    let major_scaled = scaled / major_scale;
    let major_derivative = fwidth(major_scaled);
    let major_grid_pos = abs(fract(major_scaled - 0.5) - 0.5);
    let major_dist_pixels = major_grid_pos / major_derivative;

    let major_half_width = grid.major_line_width * 0.5;
    let major_aa = 1.0 - smoothstep(major_half_width - 0.5, major_half_width + 0.5, min(major_dist_pixels.x, major_dist_pixels.y));

    return GridResult(minor_aa, major_aa);
}

// Calculate grid from world position, returns separate minor/major intensities
fn calculate_grid(world_pos: vec3<f32>) -> GridResult {
    var result = GridResult(0.0, 0.0);

    // XZ plane (horizontal grid, like a floor)
    if (grid.axes & AXIS_XZ) != 0u {
        let r = grid_line_2d(world_pos.xz);
        result.minor = max(result.minor, r.minor);
        result.major = max(result.major, r.major);
    }

    // XY plane (vertical grid, facing Z)
    if (grid.axes & AXIS_XY) != 0u {
        let r = grid_line_2d(world_pos.xy);
        result.minor = max(result.minor, r.minor);
        result.major = max(result.major, r.major);
    }

    // YZ plane (vertical grid, facing X)
    if (grid.axes & AXIS_YZ) != 0u {
        let r = grid_line_2d(world_pos.yz);
        result.minor = max(result.minor, r.minor);
        result.major = max(result.major, r.major);
    }

    return result;
}

// Apply distance-based fading
fn apply_fade(result: GridResult, world_pos: vec3<f32>) -> GridResult {
    if grid.fade_distance <= 0.0 {
        return result;
    }

    let camera_pos = view.world_position.xyz;
    let dist = distance(world_pos, camera_pos);
    let fade = 1.0 - smoothstep(grid.fade_distance * 0.5, grid.fade_distance, dist * grid.fade_strength);

    return GridResult(result.minor * fade, result.major * fade);
}

@fragment
fn fragment(
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex_output;

    // Get PBR input from the standard material
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Calculate grid intensities at this world position
    let world_pos = in.world_position.xyz;
    var grid_result = calculate_grid(world_pos);
    grid_result = apply_fade(grid_result, world_pos);

    // Blend colors: base -> minor lines -> major lines (major on top)
    let base_color = pbr_input.material.base_color;
    let minor_color = grid.line_color;
    let major_color = grid.major_line_color;

    // Apply minor lines first
    var final_color = mix(base_color, minor_color, grid_result.minor * minor_color.a);
    // Apply major lines on top (they override minor lines where they overlap)
    final_color = mix(final_color, major_color, grid_result.major * major_color.a);

    pbr_input.material.base_color = final_color;

    // Alpha discard
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
