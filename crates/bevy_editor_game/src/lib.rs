//! # Bevy Editor Game API
//!
//! Game-facing types for interacting with the bevy_modal_editor runtime.
//! This crate contains only types, events, and traits — no systems or plugins.
//!
//! Games depend on this crate to:
//! - Read/react to `GameState` (Editing, Playing, Paused)
//! - Tag entities with `GameCamera`, `GameEntity`
//! - Register custom entity types via `register_custom_entity::<T>()`
//! - Send `PlayEvent`, `PauseEvent`, `ResetEvent` messages
//! - Listen for lifecycle events (`GameStartedEvent`, etc.)
//! - Register custom components for scene serialization

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::reflect::GetTypeRegistration;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The current game/editor state.
///
/// Controls whether physics is running and the editor is active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum GameState {
    /// Physics paused, editor active (default state)
    #[default]
    Editing,
    /// Physics running, editor hidden, game logic runs
    Playing,
    /// Physics paused, editor overlays shown
    Paused,
}

// ---------------------------------------------------------------------------
// Component markers
// ---------------------------------------------------------------------------

/// Marker component for game cameras that should be disabled when the editor is active.
///
/// Add this to your game's camera. The editor will automatically manage its
/// `is_active` flag based on editor/game state.
///
/// # Example
///
/// ```ignore
/// commands.spawn((
///     Camera3d::default(),
///     GameCamera,
///     Transform::from_xyz(0.0, 5.0, 10.0),
/// ));
/// ```
#[derive(Component)]
pub struct GameCamera;

/// Marker component for entities spawned at runtime by game logic.
///
/// Entities tagged with `GameEntity` are automatically despawned when the
/// game resets (transitions back to Editing state). Use this for anything
/// created during play that should not persist into the editor.
///
/// # Example
///
/// ```ignore
/// commands.spawn((
///     GameEntity,
///     Name::new("Player Projectile"),
///     // ... other components
/// ));
/// ```
#[derive(Component, Default)]
pub struct GameEntity;

// ---------------------------------------------------------------------------
// Input messages (games send these to trigger transitions)
// ---------------------------------------------------------------------------

/// Event to start playing (or resume from paused)
#[derive(Message)]
pub struct PlayEvent;

/// Event to pause while playing
#[derive(Message)]
pub struct PauseEvent;

/// Event to reset scene to pre-play state
#[derive(Message)]
pub struct ResetEvent;

// ---------------------------------------------------------------------------
// Lifecycle events (editor fires these, games react)
// ---------------------------------------------------------------------------

/// Fired when the game starts playing from the Editing state.
#[derive(Message)]
pub struct GameStartedEvent;

/// Fired when the game resumes from the Paused state.
#[derive(Message)]
pub struct GameResumedEvent;

/// Fired when the game is paused (Playing -> Paused).
#[derive(Message)]
pub struct GamePausedEvent;

/// Fired when the game resets back to Editing state.
#[derive(Message)]
pub struct GameResetEvent;

// ---------------------------------------------------------------------------
// Component registration
// ---------------------------------------------------------------------------

/// Registry for game-defined components that should be included in scene
/// serialization (save/load and undo/redo snapshots).
///
/// Games register their custom components via [`RegisterSceneComponentExt`].
#[derive(Resource, Default)]
pub struct SceneComponentRegistry {
    appliers: Vec<fn(DynamicSceneBuilder) -> DynamicSceneBuilder>,
}

impl SceneComponentRegistry {
    /// Register a component type for scene serialization.
    pub fn register<T: Component>(&mut self) {
        self.appliers
            .push(|builder| builder.allow_component::<T>());
    }

    /// Apply all registered component allowances to a scene builder.
    pub fn apply<'w>(&self, mut builder: DynamicSceneBuilder<'w>) -> DynamicSceneBuilder<'w> {
        for applier in &self.appliers {
            builder = applier(builder);
        }
        builder
    }
}

/// Extension trait for registering game components for scene serialization.
///
/// # Example
///
/// ```ignore
/// use bevy::prelude::*;
/// use bevy_editor_game::RegisterSceneComponentExt;
///
/// #[derive(Component, Reflect, Default, serde::Serialize, serde::Deserialize)]
/// #[reflect(Component)]
/// struct MyGameComponent;
///
/// fn main() {
///     App::new()
///         .register_scene_component::<MyGameComponent>()
///         .run();
/// }
/// ```
pub trait RegisterSceneComponentExt {
    fn register_scene_component<T: Component + GetTypeRegistration>(&mut self) -> &mut Self;
}

impl RegisterSceneComponentExt for App {
    fn register_scene_component<T: Component + GetTypeRegistration>(&mut self) -> &mut Self {
        self.register_type::<T>();

        let mut registry = self
            .world_mut()
            .get_resource_or_insert_with(SceneComponentRegistry::default);
        registry.register::<T>();
        self
    }
}

// ---------------------------------------------------------------------------
// Custom entity registration — fn pointer types
// ---------------------------------------------------------------------------

/// Inspector widget function for a custom entity type.
/// Called with exclusive world access, the entity, and the egui UI.
/// Returns `true` if any property was changed.
/// Should early-return `false` if the entity doesn't have the relevant component.
pub type InspectorWidgetFn = fn(&mut World, Entity, &mut egui::Ui) -> bool;

/// Gizmo draw function for a custom entity type.
/// Called once per matching entity with the entity's global transform.
pub type GizmoDrawFn = fn(&mut Gizmos, &GlobalTransform);

/// Regenerate function for a custom entity type.
/// Called with exclusive world access after scene restore.
/// Should check internally if regeneration is needed
/// (e.g., `world.get::<Visibility>(entity).is_none()`).
pub type RegenerateFn = fn(&mut World, Entity);

// ---------------------------------------------------------------------------
// Custom entity registration
// ---------------------------------------------------------------------------

/// Describes a custom entity type that can be placed in the editor scene.
///
/// Games register these via [`RegisterCustomEntityExt::register_custom_entity`].
/// The editor integrates them into the command palette and spawn pipeline.
pub struct CustomEntityType {
    /// Display name in the command palette (e.g. "Spawn Point")
    pub name: &'static str,
    /// Category for grouping in the command palette (e.g. "Game")
    pub category: &'static str,
    /// Additional keywords for fuzzy search
    pub keywords: &'static [&'static str],
    /// Default spawn position offset from origin
    pub default_position: Vec3,
    /// Function that spawns the entity's game-specific components.
    /// The editor automatically adds: `SceneEntity`, `Name`, `Selected`.
    /// The function should add: marker component(s), `Transform`, `Visibility`,
    /// and any physics components (`Collider`, etc.).
    pub spawn: fn(&mut Commands, Vec3, Quat) -> Entity,
    /// Optional custom inspector widget for this entity type.
    pub draw_inspector: Option<InspectorWidgetFn>,
    /// Optional gizmo drawing function for this entity type.
    pub draw_gizmo: Option<GizmoDrawFn>,
    /// Optional regeneration function called after scene restore.
    pub regenerate: Option<RegenerateFn>,
}

/// A registry entry pairing user-provided `CustomEntityType` data with
/// auto-generated type information.
pub struct CustomEntityEntry {
    /// The user-provided entity type configuration.
    pub entity_type: CustomEntityType,
    /// Auto-populated function that checks if a given entity has the marker component.
    pub has_component: fn(&World, Entity) -> bool,
}

/// Registry of game-defined custom entity types that can be spawned from the editor.
#[derive(Resource, Default)]
pub struct CustomEntityRegistry {
    pub entries: Vec<CustomEntityEntry>,
}

/// Extension trait for registering custom entity types with the editor.
///
/// # Example
///
/// ```ignore
/// use bevy::prelude::*;
/// use bevy_editor_game::{CustomEntityType, RegisterCustomEntityExt};
///
/// #[derive(Component, Reflect, Default, serde::Serialize, serde::Deserialize)]
/// #[reflect(Component)]
/// struct SpawnPoint;
///
/// fn main() {
///     App::new()
///         .register_custom_entity::<SpawnPoint>(CustomEntityType {
///             name: "Spawn Point",
///             category: "Game",
///             keywords: &["start", "player"],
///             default_position: Vec3::new(0.0, 1.0, 0.0),
///             spawn: |commands, position, rotation| {
///                 commands.spawn((
///                     SpawnPoint,
///                     Transform::from_translation(position).with_rotation(rotation),
///                     Visibility::default(),
///                 )).id()
///             },
///             draw_inspector: None,
///             draw_gizmo: None,
///             regenerate: None,
///         })
///         .run();
/// }
/// ```
pub trait RegisterCustomEntityExt {
    fn register_custom_entity<T: Component + GetTypeRegistration>(
        &mut self,
        entity_type: CustomEntityType,
    ) -> &mut Self;
}

/// Monomorphized helper that checks whether a given entity has component `T`.
fn has_component<T: Component>(world: &World, entity: Entity) -> bool {
    world.get::<T>(entity).is_some()
}

impl RegisterCustomEntityExt for App {
    fn register_custom_entity<T: Component + GetTypeRegistration>(
        &mut self,
        entity_type: CustomEntityType,
    ) -> &mut Self {
        // Register component for scene serialization
        self.register_scene_component::<T>();

        // Add to custom entity registry with auto-populated has_component
        let entry = CustomEntityEntry {
            entity_type,
            has_component: has_component::<T>,
        };
        let mut registry = self
            .world_mut()
            .get_resource_or_insert_with(CustomEntityRegistry::default);
        registry.entries.push(entry);
        self
    }
}

// ---------------------------------------------------------------------------
// Material system types
// ---------------------------------------------------------------------------

/// Serializable alpha mode (mirrors Bevy's AlphaMode without the non-serializable variants)
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect, Default)]
pub enum AlphaModeValue {
    #[default]
    Opaque,
    Mask,
    Blend,
    AlphaToCoverage,
}

impl AlphaModeValue {
    pub fn to_alpha_mode(self, cutoff: f32) -> AlphaMode {
        match self {
            AlphaModeValue::Opaque => AlphaMode::Opaque,
            AlphaModeValue::Mask => AlphaMode::Mask(cutoff),
            AlphaModeValue::Blend => AlphaMode::Blend,
            AlphaModeValue::AlphaToCoverage => AlphaMode::AlphaToCoverage,
        }
    }

    pub fn from_alpha_mode(mode: &AlphaMode) -> Self {
        match mode {
            AlphaMode::Opaque => AlphaModeValue::Opaque,
            AlphaMode::Mask(_) => AlphaModeValue::Mask,
            AlphaMode::Blend => AlphaModeValue::Blend,
            AlphaMode::AlphaToCoverage => AlphaModeValue::AlphaToCoverage,
            _ => AlphaModeValue::Opaque,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AlphaModeValue::Opaque => "Opaque",
            AlphaModeValue::Mask => "Mask",
            AlphaModeValue::Blend => "Blend",
            AlphaModeValue::AlphaToCoverage => "Alpha to Coverage",
        }
    }

    pub const ALL: [AlphaModeValue; 4] = [
        AlphaModeValue::Opaque,
        AlphaModeValue::Mask,
        AlphaModeValue::Blend,
        AlphaModeValue::AlphaToCoverage,
    ];
}

/// Base PBR material properties that map to Bevy's StandardMaterial.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct BaseMaterialProps {
    pub base_color: Color,
    pub emissive: LinearRgba,
    pub metallic: f32,
    pub perceptual_roughness: f32,
    pub reflectance: f32,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
    pub unlit: bool,
    pub alpha_mode: AlphaModeValue,
}

impl Default for BaseMaterialProps {
    fn default() -> Self {
        Self {
            base_color: Color::srgb(0.5, 0.5, 0.5),
            emissive: LinearRgba::BLACK,
            metallic: 0.0,
            perceptual_roughness: 0.5,
            reflectance: 0.5,
            alpha_cutoff: 0.5,
            double_sided: false,
            unlit: false,
            alpha_mode: AlphaModeValue::Opaque,
        }
    }
}

impl BaseMaterialProps {
    /// Create from a Bevy StandardMaterial
    pub fn from_standard_material(mat: &StandardMaterial) -> Self {
        let alpha_mode = AlphaModeValue::from_alpha_mode(&mat.alpha_mode);
        let alpha_cutoff = match mat.alpha_mode {
            AlphaMode::Mask(c) => c,
            _ => 0.5,
        };
        Self {
            base_color: mat.base_color,
            emissive: mat.emissive,
            metallic: mat.metallic,
            perceptual_roughness: mat.perceptual_roughness,
            reflectance: mat.reflectance,
            alpha_cutoff,
            double_sided: mat.double_sided,
            unlit: mat.unlit,
            alpha_mode,
        }
    }

    /// Convert to a Bevy StandardMaterial
    pub fn to_standard_material(&self) -> StandardMaterial {
        StandardMaterial {
            base_color: self.base_color,
            emissive: self.emissive,
            metallic: self.metallic,
            perceptual_roughness: self.perceptual_roughness,
            reflectance: self.reflectance,
            alpha_mode: self.alpha_mode.to_alpha_mode(self.alpha_cutoff),
            double_sided: self.double_sided,
            unlit: self.unlit,
            ..default()
        }
    }

    /// Apply these properties to an existing StandardMaterial
    pub fn apply_to(&self, mat: &mut StandardMaterial) {
        mat.base_color = self.base_color;
        mat.emissive = self.emissive;
        mat.metallic = self.metallic;
        mat.perceptual_roughness = self.perceptual_roughness;
        mat.reflectance = self.reflectance;
        mat.alpha_mode = self.alpha_mode.to_alpha_mode(self.alpha_cutoff);
        mat.double_sided = self.double_sided;
        mat.unlit = self.unlit;
    }
}

/// RON-serialized extension data for a material extension type.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct MaterialExtensionData {
    /// The registered type name (e.g. "grid", "checkerboard")
    pub type_name: String,
    /// RON-serialized extension properties
    pub data: String,
}

/// A complete material definition: base PBR properties + optional extension.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct MaterialDefinition {
    pub base: BaseMaterialProps,
    pub extension: Option<MaterialExtensionData>,
}

impl MaterialDefinition {
    /// Create a standard (no extension) material with the given color
    pub fn standard(color: Color) -> Self {
        Self {
            base: BaseMaterialProps {
                base_color: color,
                ..default()
            },
            extension: None,
        }
    }

    /// Create a material with a named extension
    pub fn with_extension(base: BaseMaterialProps, type_name: &str, data: String) -> Self {
        Self {
            base,
            extension: Some(MaterialExtensionData {
                type_name: type_name.to_string(),
                data,
            }),
        }
    }
}

/// Component that references a material — either by library name or inline definition.
///
/// This is the single serializable component for materials on scene entities.
/// It replaces the old `PrimitiveMaterial` + `MaterialType` pair.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Reflect)]
#[reflect(Component)]
pub enum MaterialRef {
    /// References a named material in the MaterialLibrary
    Library(String),
    /// Inline material definition stored directly on the entity
    Inline(MaterialDefinition),
}

impl Default for MaterialRef {
    fn default() -> Self {
        MaterialRef::Inline(MaterialDefinition {
            base: BaseMaterialProps::default(),
            extension: None,
        })
    }
}

/// A named collection of shared material definitions.
///
/// Saved/loaded as part of the scene metadata sidecar file.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, Default, Reflect)]
pub struct MaterialLibrary {
    pub materials: HashMap<String, MaterialDefinition>,
}

// ---------------------------------------------------------------------------
// Validation system
// ---------------------------------------------------------------------------

/// Severity level for a validation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    Info,
    Warning,
    Error,
}

/// A single validation message produced by a validation rule.
#[derive(Debug, Clone)]
pub struct ValidationMessage {
    pub severity: ValidationSeverity,
    pub message: String,
    pub entity: Option<Entity>,
}

/// A named validation rule with a function that checks the world.
pub struct ValidationRule {
    pub name: &'static str,
    pub validate: fn(&mut World) -> Vec<ValidationMessage>,
}

/// Registry of validation rules registered by game code.
#[derive(Resource, Default)]
pub struct ValidationRegistry {
    pub rules: Vec<ValidationRule>,
}

/// Extension trait for registering validation rules.
pub trait RegisterValidationExt {
    fn register_validation(&mut self, rule: ValidationRule) -> &mut Self;
}

impl RegisterValidationExt for App {
    fn register_validation(&mut self, rule: ValidationRule) -> &mut Self {
        let mut registry = self
            .world_mut()
            .get_resource_or_insert_with(ValidationRegistry::default);
        registry.rules.push(rule);
        self
    }
}

// ---------------------------------------------------------------------------
// Asset references
// ---------------------------------------------------------------------------

/// The type of asset referenced by an `AssetRef`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Reflect)]
pub enum AssetType {
    Mesh,
    Audio,
    Texture,
    Scene,
}

/// Component that references an external asset by path.
///
/// The editor's regeneration system will automatically load Scene-type assets
/// and insert a `SceneRoot` when the entity is restored from a snapshot.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct AssetRef {
    pub path: String,
    pub asset_type: AssetType,
}
