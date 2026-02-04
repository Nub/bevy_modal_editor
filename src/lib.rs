//! # Bevy Modal Editor
//!
//! A modal level editor plugin for Bevy games with Avian3D physics support.
//!
//! ## Quick Start
//!
//! Add the editor to your Bevy app:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_modal_editor::EditorPlugin;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(EditorPlugin::default())
//!         .run();
//! }
//! ```
//!
//! ## Making Entities Editable
//!
//! Mark your entities with `SceneEntity` to make them visible and selectable in the editor:
//!
//! ```ignore
//! commands.spawn((
//!     Name::new("My Object"),
//!     SceneEntity,
//!     // ... other components
//! ));
//! ```
//!
//! ## Editor Modes
//!
//! The editor uses vim-like modal editing:
//!
//! - **View mode**: Camera navigation (WASD + mouse)
//! - **Edit mode** (`E` or `V`): Transform objects (Q=translate, W=rotate, E=scale)
//! - **Insert mode** (`I`): Add new primitives
//! - **Object Inspector** (`O`): Edit component properties
//! - **Hierarchy** (`H`): View scene hierarchy
//!
//! Press `?` for the full help menu.

pub mod commands;
pub mod constants;
pub mod editor;
pub mod gizmos;
pub mod prefabs;
pub mod scene;
pub mod selection;
pub mod ui;
pub mod utils;

// Re-export the main plugin and configuration
pub use editor::{EditorPlugin, EditorPluginConfig, PlayPlugin};

// Re-export commonly used types
pub use scene::{
    DirectionalLightMarker, GroupMarker, Locked, PrimitiveMarker, PrimitiveShape, SceneEntity,
    SceneLightMarker, SpawnPoint,
};

// Re-export selection types
pub use selection::Selected;

// Re-export editor state types
pub use editor::{AxisConstraint, EditorMode, SimulationState, TransformOperation};

// Re-export simulation events
pub use editor::{PauseEvent, PlayEvent, ResetEvent};

// Re-export scene loading
pub use editor::{SceneLoadingProgress, SceneLoadingState};

// Re-export camera types
pub use editor::{EditorCamera, GameCamera};

// Re-export serialization events
pub use scene::{LoadSceneEvent, SaveSceneEvent};

// Re-export command/history types
pub use commands::{
    DeleteSelectedEvent, DuplicateSelectedEvent, RedoEvent, SnapshotHistory, TakeSnapshotCommand,
    UndoEvent,
};
