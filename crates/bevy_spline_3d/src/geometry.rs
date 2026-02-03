//! Geometry utilities for spline-based calculations.

use bevy::prelude::*;

/// A local coordinate frame defined by tangent, right, and up vectors.
///
/// Used for transforming points along splines and computing orientations.
/// The frame is constructed from a tangent direction with automatic handling
/// of degenerate cases (e.g., when tangent is parallel to the preferred up).
#[derive(Debug, Clone, Copy)]
pub struct CoordinateFrame {
    /// The tangent (forward along spline) direction.
    pub tangent: Vec3,
    /// The right direction (perpendicular to tangent and up).
    pub right: Vec3,
    /// The corrected up direction (perpendicular to tangent and right).
    pub up: Vec3,
}

impl CoordinateFrame {
    /// Build a coordinate frame from a tangent direction using Y-up convention.
    ///
    /// Handles degenerate cases where tangent is parallel to Y by falling back
    /// to using X as the reference axis.
    pub fn from_tangent(tangent: Vec3) -> Self {
        Self::from_tangent_with_up(tangent, Vec3::Y)
    }

    /// Build a coordinate frame from a tangent and preferred up direction.
    ///
    /// The actual up vector may differ from `preferred_up` to maintain
    /// orthogonality with the tangent.
    pub fn from_tangent_with_up(tangent: Vec3, preferred_up: Vec3) -> Self {
        let tangent = tangent.normalize_or_zero();

        // Compute right as tangent × up (for road-style frames)
        let right = tangent.cross(preferred_up).normalize_or_zero();
        let up = right.cross(tangent).normalize_or_zero();

        // Handle degenerate case: tangent parallel to preferred_up
        let (right, up) = if right.length_squared() < 0.001 {
            let right = tangent.cross(Vec3::X).normalize_or_zero();
            let up = right.cross(tangent).normalize_or_zero();
            (right, up)
        } else {
            (right, up)
        };

        Self { tangent, right, up }
    }

    /// Build a coordinate frame from a forward direction (Bevy convention: -Z is forward).
    ///
    /// This is useful for entity orientations where you want the entity's
    /// forward (-Z) to point along the spline.
    pub fn from_forward(forward: Vec3, preferred_up: Vec3) -> Self {
        let forward = forward.normalize_or_zero();

        // Compute right as up × forward (for look-at style frames)
        let right = preferred_up.cross(forward).normalize_or_zero();
        let up = forward.cross(right).normalize_or_zero();

        // Handle degenerate case: forward parallel to preferred_up
        let (right, up) = if right.length_squared() < 0.001 {
            let right = Vec3::X.cross(forward).normalize_or_zero();
            let up = forward.cross(right).normalize_or_zero();
            (right, up)
        } else {
            (right, up)
        };

        // Store tangent as -forward (tangent points along spline, forward is -Z)
        Self {
            tangent: -forward,
            right,
            up,
        }
    }

    /// Check if this frame is valid (non-degenerate).
    pub fn is_valid(&self) -> bool {
        self.right.length_squared() > 0.001 && self.up.length_squared() > 0.001
    }

    /// Convert to a rotation quaternion.
    ///
    /// The rotation orients an entity so that:
    /// - Its local -Z (forward) points along `-tangent` (i.e., along the spline direction)
    /// - Its local +Y (up) points along `up`
    /// - Its local +X (right) points along `right`
    pub fn to_rotation(&self) -> Quat {
        if !self.is_valid() {
            return Quat::IDENTITY;
        }
        let forward = -self.tangent;
        Quat::from_mat3(&Mat3::from_cols(self.right, self.up, forward))
    }

    /// Convert to a rotation quaternion with custom forward direction.
    ///
    /// Use `direction = 1.0` for forward along tangent, `-1.0` for reverse.
    pub fn to_rotation_with_direction(&self, direction: f32) -> Quat {
        if !self.is_valid() {
            return Quat::IDENTITY;
        }
        let forward = if direction >= 0.0 {
            -self.tangent
        } else {
            self.tangent
        };
        // Recompute right and up for the new forward
        let right = self.up.cross(forward).normalize_or_zero();
        if right.length_squared() < 0.001 {
            return Quat::IDENTITY;
        }
        let up = forward.cross(right).normalize_or_zero();
        Quat::from_mat3(&Mat3::from_cols(right, up, forward))
    }

    /// Transform a local point to world space relative to an origin.
    ///
    /// Local coordinates map as: X → right, Y → up, Z → tangent.
    pub fn transform_point(&self, origin: Vec3, local: Vec3) -> Vec3 {
        origin + self.right * local.x + self.up * local.y + self.tangent * local.z
    }

    /// Get the world offset for a local 2D point (x, y) in the cross-section plane.
    ///
    /// This is commonly used for road mesh generation where the profile is 2D.
    pub fn transform_profile_point(&self, local_x: f32, local_y: f32) -> Vec3 {
        self.right * local_x + self.up * local_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_tangent_basic() {
        let frame = CoordinateFrame::from_tangent(Vec3::Z);
        assert!(frame.is_valid());
        assert!((frame.tangent - Vec3::Z).length() < 0.001);
        assert!((frame.up - Vec3::Y).length() < 0.001);
        assert!((frame.right - Vec3::NEG_X).length() < 0.001);
    }

    #[test]
    fn test_from_tangent_degenerate() {
        // Tangent parallel to Y should still produce valid frame
        let frame = CoordinateFrame::from_tangent(Vec3::Y);
        assert!(frame.is_valid());
    }

    #[test]
    fn test_transform_point() {
        let frame = CoordinateFrame::from_tangent(Vec3::Z);
        let origin = Vec3::new(10.0, 0.0, 0.0);
        let local = Vec3::new(1.0, 2.0, 3.0);
        let world = frame.transform_point(origin, local);

        // Should offset by right*1, up*2, tangent*3
        let expected = origin + frame.right + frame.up * 2.0 + frame.tangent * 3.0;
        assert!((world - expected).length() < 0.001);
    }
}
