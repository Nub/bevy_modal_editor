use bevy::prelude::*;

use super::types::{SplineEvaluator, SplineType};

/// A 3D spline component that can be attached to entities.
/// Fully serializable with Bevy's scene system.
#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct Spline {
    /// The type of spline interpolation.
    pub spline_type: SplineType,
    /// Control points defining the spline shape.
    pub control_points: Vec<Vec3>,
    /// Whether the spline forms a closed loop.
    pub closed: bool,
}

impl Spline {
    /// Create a new spline with the given type and control points.
    pub fn new(spline_type: SplineType, control_points: Vec<Vec3>) -> Self {
        Self {
            spline_type,
            control_points,
            closed: false,
        }
    }

    /// Create a new closed spline.
    pub fn closed(spline_type: SplineType, control_points: Vec<Vec3>) -> Self {
        Self {
            spline_type,
            control_points,
            closed: true,
        }
    }

    /// Evaluate the spline at parameter t (0.0 to 1.0).
    pub fn evaluate(&self, t: f32) -> Option<Vec3> {
        self.spline_type
            .evaluate(&self.control_points, t, self.closed)
    }

    /// Evaluate the tangent at parameter t.
    pub fn evaluate_tangent(&self, t: f32) -> Option<Vec3> {
        self.spline_type
            .evaluate_tangent(&self.control_points, t, self.closed)
    }

    /// Get the number of segments in this spline.
    pub fn segment_count(&self) -> usize {
        self.spline_type
            .segment_count(&self.control_points, self.closed)
    }

    /// Check if the spline has enough points to be valid.
    pub fn is_valid(&self) -> bool {
        self.control_points.len() >= self.spline_type.min_points()
    }

    /// Sample the spline into a series of points for rendering.
    pub fn sample(&self, samples_per_segment: usize) -> Vec<Vec3> {
        let segment_count = self.segment_count();
        if segment_count == 0 {
            return Vec::new();
        }

        let total_samples = segment_count * samples_per_segment + 1;
        let mut points = Vec::with_capacity(total_samples);

        for i in 0..total_samples {
            let t = i as f32 / (total_samples - 1) as f32;
            if let Some(point) = self.evaluate(t) {
                points.push(point);
            }
        }

        points
    }

    /// Add a control point at the given position.
    pub fn add_point(&mut self, position: Vec3) {
        self.control_points.push(position);
    }

    /// Insert a control point at the given index.
    pub fn insert_point(&mut self, index: usize, position: Vec3) {
        if index <= self.control_points.len() {
            self.control_points.insert(index, position);
        }
    }

    /// Remove the control point at the given index.
    pub fn remove_point(&mut self, index: usize) -> Option<Vec3> {
        if index < self.control_points.len() {
            Some(self.control_points.remove(index))
        } else {
            None
        }
    }

    /// Toggle between closed and open spline.
    pub fn toggle_closed(&mut self) {
        self.closed = !self.closed;
    }

    /// Cycle to the next spline type.
    pub fn cycle_type(&mut self) {
        self.spline_type = self.spline_type.next();
    }
}

/// Marker component for the currently selected spline.
#[derive(Component, Debug, Clone, Copy, Reflect, Default)]
#[reflect(Component)]
pub struct SelectedSpline;

/// Marker component identifying a control point gizmo entity.
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct ControlPointMarker {
    /// The entity that owns the spline.
    pub spline_entity: Entity,
    /// The index of this control point in the spline.
    pub index: usize,
}

/// Marker component for selected control points.
#[derive(Component, Debug, Clone, Copy, Reflect, Default)]
#[reflect(Component)]
pub struct SelectedControlPoint;

/// Cached sampled points for a spline curve.
///
/// This component stores pre-computed sample points to avoid
/// resampling the spline every frame during gizmo rendering.
#[derive(Component, Default, Clone, Debug)]
pub struct CachedSplineCurve {
    /// The sampled points along the curve.
    pub points: Vec<Vec3>,
    /// The resolution used when sampling (to detect settings changes).
    pub resolution: usize,
}
