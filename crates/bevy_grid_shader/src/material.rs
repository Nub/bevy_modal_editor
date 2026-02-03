use bevy::{
    asset::embedded_asset,
    pbr::{MaterialExtension, MaterialPlugin, StandardMaterial},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use bitflags::bitflags;

pub use bevy::pbr::ExtendedMaterial;

/// Plugin that registers the [`GridMaterial`] with Bevy's rendering system.
pub struct GridMaterialPlugin;

impl Plugin for GridMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "grid.wgsl");
        app.add_plugins(MaterialPlugin::<
            ExtendedMaterial<StandardMaterial, GridMaterial>,
        >::default());
    }
}

bitflags! {
    /// Determines which world-space axes the grid is projected onto.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct GridAxes: u32 {
        /// Grid on the XZ plane (horizontal, like a floor)
        const XZ = 0b001;
        /// Grid on the XY plane (vertical, facing Z)
        const XY = 0b010;
        /// Grid on the YZ plane (vertical, facing X)
        const YZ = 0b100;
        /// Grid on all three planes
        const ALL = Self::XZ.bits() | Self::XY.bits() | Self::YZ.bits();
    }
}

/// Uniform data sent to the GPU for the grid shader.
#[derive(Clone, Copy, ShaderType, Debug)]
pub struct GridMaterialUniform {
    /// Color of minor grid lines
    pub line_color: LinearRgba,
    /// Color of major grid lines
    pub major_line_color: LinearRgba,
    /// Width of minor grid lines in screen-space pixels
    pub line_width: f32,
    /// Width of major grid lines in screen-space pixels
    pub major_line_width: f32,
    /// Size of grid cells in world units
    pub grid_scale: f32,
    /// Draw a major (thicker) line every N cells
    pub major_line_every: u32,
    /// Which axes to project the grid onto (bitmask)
    pub axes: u32,
    /// Distance at which grid starts to fade out (0 = no fade)
    pub fade_distance: f32,
    /// How quickly the grid fades (higher = sharper falloff)
    pub fade_strength: f32,
    pub _padding: f32,
}

/// A material that renders a world-space aligned grid with PBR lighting.
///
/// This material extends Bevy's `StandardMaterial` to add procedural grid lines
/// that align with world coordinates rather than object UVs.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct GridMaterial {
    /// Uniform data for the grid shader
    #[uniform(100)]
    pub uniform: GridMaterialUniform,
}

impl Default for GridMaterial {
    fn default() -> Self {
        Self {
            uniform: GridMaterialUniform {
                line_color: LinearRgba::new(0.0, 0.0, 0.0, 0.5),
                major_line_color: LinearRgba::new(0.0, 0.0, 0.0, 0.5),
                line_width: 1.0,
                major_line_width: 3.0,
                grid_scale: 1.0,
                major_line_every: 10,
                axes: GridAxes::XZ.bits(),
                fade_distance: 0.0,
                fade_strength: 1.0,
                _padding: 0.0,
            },
        }
    }
}

impl GridMaterial {
    /// Creates a new grid material with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the color of the grid lines.
    pub fn with_line_color(mut self, color: impl Into<LinearRgba>) -> Self {
        self.uniform.line_color = color.into();
        self
    }

    /// Sets the width of minor grid lines in screen-space pixels.
    pub fn with_line_width(mut self, width: f32) -> Self {
        self.uniform.line_width = width;
        self
    }

    /// Sets the size of grid cells in world units.
    pub fn with_grid_scale(mut self, scale: f32) -> Self {
        self.uniform.grid_scale = scale;
        self
    }

    /// Sets how often major (thicker) lines appear.
    /// For example, `5` means every 5th line is a major line.
    pub fn with_major_line_every(mut self, n: u32) -> Self {
        self.uniform.major_line_every = n;
        self
    }

    /// Sets the color of major grid lines.
    pub fn with_major_line_color(mut self, color: impl Into<LinearRgba>) -> Self {
        self.uniform.major_line_color = color.into();
        self
    }

    /// Sets the width of major grid lines in screen-space pixels.
    pub fn with_major_line_width(mut self, width: f32) -> Self {
        self.uniform.major_line_width = width;
        self
    }

    /// Sets which world-space axes the grid is projected onto.
    pub fn with_axes(mut self, axes: GridAxes) -> Self {
        self.uniform.axes = axes.bits();
        self
    }

    /// Sets the distance at which the grid starts fading out.
    /// Set to 0 to disable fading.
    pub fn with_fade_distance(mut self, distance: f32) -> Self {
        self.uniform.fade_distance = distance;
        self
    }

    /// Sets how quickly the grid fades out (higher = sharper falloff).
    pub fn with_fade_strength(mut self, strength: f32) -> Self {
        self.uniform.fade_strength = strength;
        self
    }
}

impl MaterialExtension for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_grid_shader/grid.wgsl".into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        "embedded://bevy_grid_shader/grid.wgsl".into()
    }
}
