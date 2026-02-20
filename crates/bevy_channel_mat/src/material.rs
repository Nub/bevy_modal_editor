use bevy::{
    asset::embedded_asset,
    pbr::{MaterialExtension, MaterialPlugin, StandardMaterial},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

pub use bevy::pbr::ExtendedMaterial;

/// Plugin that registers the [`ChannelThresholdMaterial`] with Bevy's rendering system.
pub struct ChannelThresholdPlugin;

impl Plugin for ChannelThresholdPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "channel_threshold.wgsl");
        app.add_plugins(MaterialPlugin::<
            ExtendedMaterial<StandardMaterial, ChannelThresholdMaterial>,
        >::default());
    }
}

/// Uniform data sent to the GPU for the channel threshold shader.
#[derive(Clone, Copy, ShaderType, Debug)]
pub struct ChannelThresholdUniform {
    /// Which color channel to threshold: 0=R, 1=G, 2=B
    pub channel: u32,
    /// Cutoff value (0.0–1.0)
    pub threshold: f32,
    /// Width of the smooth transition zone
    pub smoothing: f32,
    /// Whether to invert the alpha (0 or 1)
    pub invert: u32,
}

/// A material that controls opacity based on a color channel threshold.
///
/// Extends Bevy's `StandardMaterial` to discard or fade fragments based on
/// whether the selected color channel (R, G, or B) of the base color exceeds
/// a configurable threshold, with optional smoothing and inversion.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct ChannelThresholdMaterial {
    #[uniform(100)]
    pub uniform: ChannelThresholdUniform,
}

impl Default for ChannelThresholdMaterial {
    fn default() -> Self {
        Self {
            uniform: ChannelThresholdUniform {
                channel: 0,
                threshold: 0.5,
                smoothing: 0.05,
                invert: 0,
            },
        }
    }
}

impl MaterialExtension for ChannelThresholdMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_channel_mat/channel_threshold.wgsl".into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        "embedded://bevy_channel_mat/channel_threshold.wgsl".into()
    }
}
