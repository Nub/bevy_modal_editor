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

### Core Modules

- **constants** (`src/constants.rs`) - Centralized configuration values
  - `primitive_colors` - Default colors for each primitive shape
  - `light_colors` - Default light colors and intensities
  - `preview_colors` - Colors for insert mode preview entities
  - `physics` - Physics-related constants (collider sizes)
  - `sizes` - Default dimension constants

- **utils** (`src/utils.rs`) - Shared utility functions
  - `get_half_height_along_normal()` - Calculate object height for surface placement
  - `rotation_from_normal()` - Create rotation quaternion from surface normal

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
  - `PrimitivesPlugin` - Unified entity spawning via `SpawnEntityEvent`
  - `SerializationPlugin` - RON-based save/load via `SaveSceneEvent`/`LoadSceneEvent` messages
  - `GltfSourcePlugin` - Load GLTF/GLB models as scene objects
  - `SceneSourcePlugin` - Load RON scene files as nested objects
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
  - `theme` module - Centralized styling with `colors`, dialog helpers (`draw_centered_dialog`, `draw_error_dialog`)

### Key Patterns

- Events use Bevy's `Message` derive macro with `MessageReader`/`MessageWriter`
- Serialization uses serde with RON format
- Physics provided by Avian3D (`Collider`, `RigidBody`, `SpatialQuery`)
- UI via bevy_egui (from git branch for Bevy 0.18 compatibility)

### Entity Spawning

Use `SpawnEntityEvent` to spawn any scene entity type:

```rust
// Spawn a primitive
events.spawn_entity.write(SpawnEntityEvent {
    kind: SpawnEntityKind::Primitive(PrimitiveShape::Cube),
    position: Vec3::ZERO,
    rotation: Quat::IDENTITY,
});

// Spawn a light
events.spawn_entity.write(SpawnEntityEvent {
    kind: SpawnEntityKind::PointLight,
    position: Vec3::new(0.0, 3.0, 0.0),
    rotation: Quat::IDENTITY,
});
```

`SpawnEntityKind` variants: `Primitive(PrimitiveShape)`, `Group`, `PointLight`, `DirectionalLight`

`PrimitiveShape` provides factory methods:
- `create_mesh()` - Returns the mesh for this shape
- `create_collider()` - Returns the physics collider
- `default_color()` - Returns the standard color from `constants::primitive_colors`

### Entity Markers

- `SceneEntity` - Part of the editable scene (saved/loaded)
- `Selected` - Currently selected
- `PrefabInstance` / `PrefabRoot` - Prefab system markers
- `PrimitiveMarker` - Identifies primitive shape type
- `GroupMarker` - Empty container for organizing entities
- `SceneLightMarker` / `DirectionalLightMarker` - Light configuration that persists to scene files
- `GltfLoaded` / `SceneSourceLoaded` - Marks children loaded from external files
