// JFA Composite Shader
// Reads distance field from JFA result and composites outline over scene

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var jfa_texture: texture_2d<f32>;
@group(0) @binding(3) var jfa_sampler: sampler;
@group(0) @binding(4) var silhouette_texture: texture_2d<f32>;
@group(0) @binding(5) var silhouette_sampler: sampler;

struct OutlineSettings {
    color: vec4<f32>,
    width: f32,
    enabled: f32,
    _padding: vec2<f32>,
};

@group(0) @binding(6) var<uniform> settings: OutlineSettings;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene_color = textureSample(scene_texture, scene_sampler, in.uv);

    if settings.enabled < 0.5 {
        return scene_color;
    }

    // Check JFA first - most pixels have no valid seed (cheaper than silhouette sample)
    // With rg16unorm format, invalid seeds are (0, 0) since valid UVs are at pixel centers
    let seed_uv = textureSample(jfa_texture, jfa_sampler, in.uv).xy;
    if seed_uv.x <= 0.0 {
        return scene_color;
    }

    // Calculate distance and early-out if beyond outline width
    let tex_size = vec2<f32>(textureDimensions(jfa_texture));
    let diff = (in.uv - seed_uv) * tex_size;
    let dist = length(diff);
    if dist > settings.width {
        return scene_color;
    }

    // Only sample silhouette for pixels potentially in the outline
    let silhouette = textureSample(silhouette_texture, silhouette_sampler, in.uv).a;
    if silhouette > 0.5 {
        return scene_color;
    }

    // Smooth outline with 1-pixel AA falloff
    let outline_strength = 1.0 - smoothstep(settings.width - 1.0, settings.width, dist);
    return mix(scene_color, settings.color, outline_strength * settings.color.a);
}
