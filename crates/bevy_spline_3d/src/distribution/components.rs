use bevy::prelude::*;

/// Component that defines how entities are distributed along a spline.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SplineDistribution {
    /// The spline entity to distribute along.
    pub spline: Entity,
    /// The source/template entity to clone.
    /// This entity should have a `DistributionSource` component.
    pub source: Entity,
    /// Number of copies to distribute along the spline.
    pub count: usize,
    /// How to orient the distributed copies.
    pub orientation: DistributionOrientation,
    /// How to space the distributed copies.
    pub spacing: DistributionSpacing,
    /// Offset applied to each instance in local space.
    pub offset: Vec3,
    /// Whether distribution is enabled.
    pub enabled: bool,
}

impl Default for SplineDistribution {
    fn default() -> Self {
        Self {
            spline: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
            count: 10,
            orientation: DistributionOrientation::default(),
            spacing: DistributionSpacing::default(),
            offset: Vec3::ZERO,
            enabled: true,
        }
    }
}

impl SplineDistribution {
    /// Create a new distribution along a spline.
    pub fn new(spline: Entity, source: Entity, count: usize) -> Self {
        Self {
            spline,
            source,
            count,
            ..default()
        }
    }

    /// Set the orientation mode.
    pub fn with_orientation(mut self, orientation: DistributionOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Set the local offset for each instance.
    pub fn with_offset(mut self, offset: Vec3) -> Self {
        self.offset = offset;
        self
    }

    /// Set the spacing mode.
    pub fn with_spacing(mut self, spacing: DistributionSpacing) -> Self {
        self.spacing = spacing;
        self
    }

    /// Use uniform arc-length spacing (recommended for even distribution).
    pub fn uniform(mut self) -> Self {
        self.spacing = DistributionSpacing::Uniform;
        self
    }
}

/// How to orient distributed entities along the spline.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Default)]
pub enum DistributionOrientation {
    /// Only set position, keep default rotation.
    #[default]
    PositionOnly,
    /// Align the entity's forward direction (-Z) to the spline tangent.
    /// The `up` vector is used to constrain the rotation.
    AlignToTangent {
        /// The up vector to use for orientation (typically `Vec3::Y`).
        up: Vec3,
    },
}

impl DistributionOrientation {
    /// Create an AlignToTangent orientation with Y as up.
    pub fn align_to_tangent() -> Self {
        Self::AlignToTangent { up: Vec3::Y }
    }

    /// Create an AlignToTangent orientation with a custom up vector.
    pub fn align_to_tangent_with_up(up: Vec3) -> Self {
        Self::AlignToTangent { up }
    }
}

/// How to space distributed entities along the spline.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Default)]
pub enum DistributionSpacing {
    /// Uniform spacing based on arc length (even visual distribution).
    /// This samples the spline to compute distances and distributes
    /// entities at equal arc-length intervals.
    #[default]
    Uniform,
    /// Parametric spacing based on spline t parameter (0 to 1).
    /// Faster but entities will bunch up in areas with closely
    /// spaced control points.
    Parametric,
}

/// Marker component for entities that serve as distribution templates.
///
/// Entities with this component will be automatically hidden when used
/// as a source for `SplineDistribution`.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct DistributionSource;

/// Marker component added to distributed instance entities.
///
/// This allows tracking which distribution an instance belongs to,
/// and enables cleanup when the distribution is removed.
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct DistributedInstance {
    /// The distribution controller entity.
    pub distribution: Entity,
    /// The index of this instance (0 to count-1).
    pub index: usize,
}

/// Internal component to track distribution state.
#[derive(Component, Debug, Clone)]
pub(crate) struct DistributionState {
    /// Currently spawned instance entities.
    pub instances: Vec<Entity>,
    /// Cached count to detect changes.
    pub cached_count: usize,
    /// Cached source to detect changes.
    pub cached_source: Entity,
}
