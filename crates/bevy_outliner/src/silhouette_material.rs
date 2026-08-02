//! Minimal material for silhouette rendering.
//!
//! This material outputs solid white with no lighting calculations,
//! replacing the heavyweight PBR shader for silhouette passes.

use bevy::{
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

/// A minimal material that outputs solid white.
/// Used for silhouette rendering where we only need object presence.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct SilhouetteMaterial {
    #[uniform(0)]
    _dummy: u32,
}

impl Material for SilhouetteMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_outliner/shaders/silhouette.wgsl".into()
    }
}
