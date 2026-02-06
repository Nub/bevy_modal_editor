//! Procedural placement component and types.

use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A template entity with a placement weight.
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct WeightedTemplate {
    /// The template entity to place instances of.
    pub entity: Entity,
    /// Relative weight for selection (higher = more likely).
    pub weight: f32,
}

impl WeightedTemplate {
    /// Create a new weighted template.
    pub fn new(entity: Entity, weight: f32) -> Self {
        Self { entity, weight }
    }
}

/// Sampling mode for procedural placement.
#[derive(Clone, Debug, Default, Reflect, Serialize, Deserialize)]
pub enum SamplingMode {
    /// Evenly distributed samples.
    #[default]
    Uniform,
    /// Randomly distributed samples with optional seed.
    Random {
        seed: Option<u64>,
    },
}

impl SamplingMode {
    /// Returns true if this is uniform sampling.
    pub fn is_uniform(&self) -> bool {
        matches!(self, SamplingMode::Uniform)
    }

    /// Returns the seed if random mode, None otherwise.
    pub fn seed(&self) -> Option<u64> {
        match self {
            SamplingMode::Uniform => None,
            SamplingMode::Random { seed } => *seed,
        }
    }
}

/// How to orient placed instances.
#[derive(Clone, Debug, Default, Reflect, Serialize, Deserialize)]
pub enum PlacementOrientation {
    /// Keep the template's original rotation.
    #[default]
    Identity,
    /// Align to the sample tangent (useful for splines).
    AlignToTangent {
        up: Vec3,
    },
    /// Align to surface normal (requires projection).
    AlignToSurface,
    /// Random rotation around Y axis.
    RandomYaw,
    /// Fully random rotation.
    RandomFull,
}

/// Configuration for surface projection.
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct SurfaceProjection {
    /// Whether projection is enabled.
    pub enabled: bool,
    /// Direction to cast rays (default: down).
    /// Use `local_space` to interpret this relative to the source entity.
    pub direction: Vec3,
    /// Whether direction is in local space (relative to source entity) or world space.
    /// When true, the direction is transformed by the source entity's rotation.
    pub local_space: bool,
    /// Offset from sample point (opposite to direction) to start ray.
    pub ray_origin_offset: f32,
    /// Maximum ray distance.
    pub max_distance: f32,
    /// Collision layers to consider (not serialized).
    #[serde(skip)]
    pub collision_layers: Option<LayerMask>,
    /// Entities to exclude from raycast (typically the source and instances).
    #[serde(skip)]
    #[reflect(ignore)]
    pub exclude_entities: Vec<Entity>,
}

impl Default for SurfaceProjection {
    fn default() -> Self {
        Self {
            enabled: false,
            direction: Vec3::NEG_Y,
            local_space: false,
            ray_origin_offset: 10.0,
            max_distance: 100.0,
            collision_layers: None,
            exclude_entities: Vec::new(),
        }
    }
}

/// Main component for procedural placement.
///
/// Add this to any entity with a `Mesh3d` or `Spline` component to sample
/// its surface/curve and place template instances.
#[derive(Component, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component, Default)]
pub struct ProceduralPlacer {
    /// Template entities to place, with weights for random selection.
    pub templates: Vec<WeightedTemplate>,
    /// Number of instances to place.
    pub count: usize,
    /// Sampling mode (uniform or random).
    pub mode: SamplingMode,
    /// How to orient placed instances.
    pub orientation: PlacementOrientation,
    /// Local offset applied to each instance.
    pub offset: Vec3,
    /// Surface projection configuration.
    pub projection: SurfaceProjection,
    /// Whether to offset instances by their bounds to prevent clipping.
    pub use_bounds_offset: bool,
    /// Whether placement is enabled.
    pub enabled: bool,
}

impl Default for ProceduralPlacer {
    fn default() -> Self {
        Self {
            templates: Vec::new(),
            count: 10,
            mode: SamplingMode::default(),
            orientation: PlacementOrientation::default(),
            offset: Vec3::ZERO,
            projection: SurfaceProjection::default(),
            use_bounds_offset: false,
            enabled: true,
        }
    }
}

impl ProceduralPlacer {
    /// Create a new placer with the given templates.
    pub fn new(templates: Vec<WeightedTemplate>) -> Self {
        Self {
            templates,
            ..default()
        }
    }

    /// Create a new placer with a single template.
    pub fn single(template: Entity) -> Self {
        Self::new(vec![WeightedTemplate::new(template, 1.0)])
    }

    /// Set the number of instances to place.
    pub fn with_count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// Set the sampling mode.
    pub fn with_mode(mut self, mode: SamplingMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the orientation mode.
    pub fn with_orientation(mut self, orientation: PlacementOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Set a local offset for all instances.
    pub fn with_offset(mut self, offset: Vec3) -> Self {
        self.offset = offset;
        self
    }

    /// Enable surface projection with default settings.
    pub fn with_projection(mut self) -> Self {
        self.projection.enabled = true;
        self
    }

    /// Set custom projection configuration.
    pub fn with_projection_config(mut self, projection: SurfaceProjection) -> Self {
        self.projection = projection;
        self
    }

    /// Enable bounds offset to prevent surface clipping.
    pub fn with_bounds_offset(mut self) -> Self {
        self.use_bounds_offset = true;
        self
    }

    /// Add a template with a weight.
    pub fn add_template(&mut self, entity: Entity, weight: f32) {
        self.templates.push(WeightedTemplate::new(entity, weight));
    }
}

/// Marker component for template entities.
///
/// Entities with this marker will be automatically hidden.
/// Add this to template entities that ProceduralPlacer references.
#[derive(Component, Clone, Debug, Default, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ProceduralTemplate;

/// Backwards-compatible alias for ProceduralTemplate.
#[deprecated(since = "0.1.0", note = "Use ProceduralTemplate instead")]
pub type PlacementTemplate = ProceduralTemplate;

/// Marker component for procedurally placed instances.
///
/// This is added to all entities spawned by a ProceduralPlacer.
#[derive(Component, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ProceduralEntity {
    /// The placer entity that created this instance.
    pub placer: Entity,
    /// The template entity this instance was created from.
    pub template: Entity,
    /// Index in the placement sequence.
    pub index: usize,
}

/// Backwards-compatible alias for ProceduralEntity.
#[deprecated(since = "0.1.0", note = "Use ProceduralEntity instead")]
pub type PlacedInstance = ProceduralEntity;
