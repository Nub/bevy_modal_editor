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

struct ChannelThresholdUniform {
    channel: u32,
    threshold: f32,
    smoothing: f32,
    invert: u32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> params: ChannelThresholdUniform;

@fragment
fn fragment(
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex_output;

    // Get PBR input from the standard material
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Read the selected channel from base_color
    let base = pbr_input.material.base_color;
    var channel_value: f32;
    if params.channel == 1u {
        channel_value = base.g;
    } else if params.channel == 2u {
        channel_value = base.b;
    } else {
        channel_value = base.r;
    }

    // Compute alpha with smoothstep
    var alpha = smoothstep(
        params.threshold - params.smoothing,
        params.threshold + params.smoothing,
        channel_value,
    );

    // Invert if requested
    if params.invert != 0u {
        alpha = 1.0 - alpha;
    }

    // Multiply into base_color alpha
    pbr_input.material.base_color.a *= alpha;

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
