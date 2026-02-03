// Silhouette shader - renders outlined objects to a texture
// Stores the UV coordinates of each pixel as seed values for JFA

#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = mesh_position_local_to_clip(
        get_world_from_local(vertex.instance_index),
        vec4<f32>(vertex.position, 1.0),
    );
    return out;
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // Store the screen-space position as the seed value
    // We use the position directly - it will be normalized in the JFA passes
    return vec4<f32>(position.xy, 0.0, 1.0);
}
