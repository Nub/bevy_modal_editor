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

use bevy::prelude::*;
use bevy::reflect::GetTypeRegistration;

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
}

/// Registry of game-defined custom entity types that can be spawned from the editor.
#[derive(Resource, Default)]
pub struct CustomEntityRegistry {
    pub types: Vec<CustomEntityType>,
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

impl RegisterCustomEntityExt for App {
    fn register_custom_entity<T: Component + GetTypeRegistration>(
        &mut self,
        entity_type: CustomEntityType,
    ) -> &mut Self {
        // Register component for scene serialization
        self.register_scene_component::<T>();

        // Add to custom entity registry
        let mut registry = self
            .world_mut()
            .get_resource_or_insert_with(CustomEntityRegistry::default);
        registry.types.push(entity_type);
        self
    }
}
