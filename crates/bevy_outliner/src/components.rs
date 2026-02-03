use bevy::{prelude::*, render::extract_component::ExtractComponent};

/// Component that marks an entity to be outlined.
///
/// Add this component to any entity with a mesh to give it an outline.
#[derive(Component, Clone, Copy, ExtractComponent, Reflect)]
#[reflect(Component)]
pub struct MeshOutline {
    /// The color of the outline.
    pub color: LinearRgba,
    /// The width of the outline in pixels.
    pub width: f32,
}

impl Default for MeshOutline {
    fn default() -> Self {
        Self {
            color: LinearRgba::new(1.0, 0.5, 0.0, 1.0),
            width: 5.0,
        }
    }
}

impl MeshOutline {
    /// Create a new outline with the specified color and width.
    pub fn new(color: impl Into<LinearRgba>, width: f32) -> Self {
        Self {
            color: color.into(),
            width,
        }
    }

    /// Create an outline with default width and the specified color.
    pub fn with_color(color: impl Into<LinearRgba>) -> Self {
        Self {
            color: color.into(),
            ..Default::default()
        }
    }

    /// Create an outline with default color and the specified width.
    pub fn with_width(width: f32) -> Self {
        Self {
            width,
            ..Default::default()
        }
    }
}

/// Camera component that enables and configures outline rendering.
///
/// Add this to cameras that should render outlines.
#[derive(Component, Clone, Copy, ExtractComponent, Reflect)]
#[reflect(Component)]
pub struct OutlineSettings {
    /// Whether outline rendering is enabled.
    pub enabled: bool,
}

impl Default for OutlineSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

