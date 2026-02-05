#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
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

struct CheckerboardUniform {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    scale: f32,
    _padding: vec3<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> checker: CheckerboardUniform;

@fragment
fn fragment(
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex_output;

    // Get PBR input from the standard material
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // World-space checkerboard
    let world_pos = in.world_position.xyz * checker.scale;
    let ix = i32(floor(world_pos.x));
    let iy = i32(floor(world_pos.y));
    let iz = i32(floor(world_pos.z));
    let check = (ix + iy + iz) & 1;

    let checker_color = select(checker.color_a, checker.color_b, check == 1);

    // Multiply base color by checker pattern
    pbr_input.material.base_color = pbr_input.material.base_color * checker_color;

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
