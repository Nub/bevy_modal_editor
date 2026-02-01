//! # Bevy Avian3D Editor
//!
//! A level editor plugin for Bevy games using Avian3D physics.
//!
//! ## Quick Start
//!
//! Add the editor to your Bevy app:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_avian3d_editor::EditorPlugin;
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
pub mod editor;
pub mod gizmos;
pub mod patterns;
pub mod prefabs;
pub mod scene;
pub mod selection;
pub mod ui;

// Re-export the main plugin and configuration
pub use editor::{EditorPlugin, EditorPluginConfig};

// Re-export commonly used types
pub use scene::{
    DirectionalLightMarker, GroupMarker, Locked, PrimitiveMarker, PrimitiveShape, SceneEntity,
    SceneLightMarker,
};

// Re-export selection types
pub use selection::Selected;

// Re-export editor state types
pub use editor::{AxisConstraint, EditorMode, TransformOperation};

// Re-export serialization events
pub use scene::{LoadSceneEvent, SaveSceneEvent};

// Re-export command/history types
pub use commands::{
    DeleteSelectedEvent, DuplicateSelectedEvent, RedoEvent, SnapshotHistory, TakeSnapshotCommand,
    UndoEvent,
};
