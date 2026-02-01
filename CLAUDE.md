# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
# Enter development environment (required for dependencies)
nix develop

# Build and run
cargo run

# Check for errors without building
cargo check

# Build release
cargo build --release
```

The project uses Nix for dependency management. The `flake.nix` provides all necessary system dependencies (Vulkan, X11, Wayland, audio, etc.).

## Architecture Overview

This is a level editor for Bevy games using Avian3D physics. The main plugin (`EditorPlugin` in `src/editor/plugin.rs`) bundles all functionality.

### Modal Editing System

The editor uses vim-like modal editing (`src/editor/state.rs`):
- **View mode**: Camera navigation and selection only
- **Edit mode**: Transform manipulation (G=translate, R=rotate, S=scale)
- Toggle with Tab, Escape returns to View mode

### Plugin Structure

The `EditorPlugin` composes these sub-plugins:

- **editor/** - Core editor functionality
  - `EditorStatePlugin` - Modal state machine (`EditorMode`) and `TransformOperation` resource
  - `EditorInputPlugin` - Keyboard handling for mode/operation switching
  - `EditorCameraPlugin` - Orbit camera controls

- **selection/** - Entity selection via physics raycasting against `SceneEntity` components. `Selected` marker component indicates selection state.

- **commands/** - Undo/redo system
  - `HistoryPlugin` - `CommandHistory` resource with undo/redo stacks
  - `OperationsPlugin` - Concrete command implementations
  - Commands implement `EditorCommand` trait (execute/undo/description)

- **scene/** - Scene management
  - `PrimitivesPlugin` - Spawning primitive shapes (Cube, Sphere, Cylinder, Capsule, Plane)
  - `SerializationPlugin` - RON-based save/load via `SaveSceneEvent`/`LoadSceneEvent` messages
  - `SceneEntity` marker component identifies editable entities

- **prefabs/** - Reusable entity templates
  - `Prefab` asset type with hierarchical `PrefabEntity` structures
  - `PrefabInstance` and `PrefabRoot` marker components

- **gizmos/** - Visual editor overlays
  - `TransformGizmoPlugin` - Transform manipulation gizmos
  - Grid drawing system

- **patterns/** - Pattern-based entity duplication
  - `LinearPatternPlugin` - Linear array spawning
  - `CircularPatternPlugin` - Circular array spawning

- **ui/** - egui-based interface
  - `PanelsPlugin` - Main panel layout
  - `HierarchyPlugin` - Entity tree view
  - `InspectorPlugin` - Component property editor
  - `ToolbarPlugin` - Tool buttons
  - `ViewGizmoPlugin` - Viewport orientation indicator

### Key Patterns

- Events use Bevy's `Message` derive macro with `MessageReader`/`MessageWriter`
- Serialization uses serde with RON format
- Physics provided by Avian3D (`Collider`, `RigidBody`, `SpatialQuery`)
- UI via bevy_egui (from git branch for Bevy 0.18 compatibility)

### Entity Markers

- `SceneEntity` - Part of the editable scene (saved/loaded)
- `Selected` - Currently selected
- `PrefabInstance` / `PrefabRoot` - Prefab system markers
- `PrimitiveMarker` - Identifies primitive shape type
