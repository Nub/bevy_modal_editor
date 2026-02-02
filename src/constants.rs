//! Centralized constants for the editor
//!
//! This module contains all shared constants like colors, sizes, and default values
//! to ensure consistency across the codebase.

use bevy::prelude::*;

/// Default material colors for primitive shapes
pub mod primitive_colors {
    use super::*;

    pub const CUBE: Color = Color::srgb(0.8, 0.7, 0.6);
    pub const SPHERE: Color = Color::srgb(0.6, 0.7, 0.8);
    pub const CYLINDER: Color = Color::srgb(0.7, 0.8, 0.6);
    pub const CAPSULE: Color = Color::srgb(0.8, 0.6, 0.7);
    pub const PLANE: Color = Color::srgb(0.6, 0.6, 0.8);

    /// Get the default color for a primitive shape
    pub fn for_shape(shape: crate::scene::PrimitiveShape) -> Color {
        match shape {
            crate::scene::PrimitiveShape::Cube => CUBE,
            crate::scene::PrimitiveShape::Sphere => SPHERE,
            crate::scene::PrimitiveShape::Cylinder => CYLINDER,
            crate::scene::PrimitiveShape::Capsule => CAPSULE,
            crate::scene::PrimitiveShape::Plane => PLANE,
        }
    }
}

/// Default colors for light previews in insert mode
pub mod light_colors {
    use super::*;

    /// Point light preview color (warm yellow glow)
    pub const POINT_PREVIEW: Color = Color::srgba(1.0, 0.9, 0.5, 0.7);
    /// Directional/sun light preview color (bright warm)
    pub const DIRECTIONAL_PREVIEW: Color = Color::srgba(1.0, 0.95, 0.4, 0.7);

    /// Default point light color
    pub const POINT_DEFAULT: Color = Color::srgb(1.0, 0.95, 0.8);
    /// Default point light intensity
    pub const POINT_DEFAULT_INTENSITY: f32 = 80000.0;
    /// Default point light range
    pub const POINT_DEFAULT_RANGE: f32 = 30.0;

    /// Default directional light color
    pub const DIRECTIONAL_DEFAULT: Color = Color::srgb(1.0, 0.98, 0.9);
    /// Default directional light illuminance
    pub const DIRECTIONAL_DEFAULT_ILLUMINANCE: f32 = 10000.0;
}

/// Preview colors for insert mode
pub mod preview_colors {
    use super::*;

    /// Generic preview tint (semi-transparent blue)
    pub const GENERIC: Color = Color::srgba(0.3, 0.7, 1.0, 0.5);
    /// Group preview color (semi-transparent green)
    pub const GROUP: Color = Color::srgba(0.5, 1.0, 0.5, 0.3);
    /// Scene preview color (semi-transparent green)
    pub const SCENE: Color = Color::srgba(0.2, 0.8, 0.4, 0.5);
    /// Fallback/placeholder color (semi-transparent orange)
    pub const FALLBACK: Color = Color::srgba(1.0, 0.6, 0.2, 0.5);
}

/// Physics-related constants
pub mod physics {
    /// Radius of the invisible collider used for selecting lights via raycasting
    pub const LIGHT_COLLIDER_RADIUS: f32 = 0.5;
}

/// Default sizes for various operations
pub mod sizes {
    /// Default distance from camera when placing objects with no surface hit
    pub const INSERT_DEFAULT_DISTANCE: f32 = 10.0;
    /// Default half-height fallback when no collider is present
    pub const DEFAULT_HALF_HEIGHT: f32 = 0.5;
}
