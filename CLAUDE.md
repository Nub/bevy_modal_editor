# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Workspace Structure

This is a Cargo workspace containing:

```
bevy_modal_editor/
├── Cargo.toml          (workspace root + editor package)
├── src/                (editor code)
├── crates/
│   ├── bevy_outliner/  (JFA-based object outlining)
│   └── bevy_spline_3d/ (3D spline editing with gizmos)
```

**Member crates:**

- **bevy_outliner** (`crates/bevy_outliner`) - Jump Flood Algorithm based object outlining for mesh selection visualization

- **bevy_spline_3d** (`crates/bevy_spline_3d`) - Spline curve library providing:
  - Spline types: Cubic Bezier, Catmull-Rom, B-Spline
  - Control point editing with gizmos
  - Road mesh generation, distribution along splines, path following
  - Surface projection onto terrain

The `SplineEditPlugin` bridges `bevy_spline_3d` with the modal editor:
- Disables library hotkeys (uses modal-aware input instead)
- Syncs `EditorSettings` based on editor mode
- Control points only editable in Edit mode with spline selected
- X-ray rendering enabled only during spline editing

## Build Commands

**Important:** All commands must be run inside the Nix development shell. Run `nix develop` first, or ensure you are already in the shell before building or running anything.

```bash
# Enter development environment (REQUIRED before any build/run commands)
nix develop

# Build and run the editor
cargo run

# Build all workspace members
cargo build --workspace

# Check all crates for errors
cargo check --workspace

# Run a specific crate's example
cargo run -p bevy_outliner --example basic
cargo run -p bevy_spline_3d --example editor

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
  - `should_process_input()` - Check if editor input should be handled (guards against disabled editor or UI focus)
  - `get_half_height_along_normal()` - Calculate object height for surface placement
  - `rotation_from_normal()` - Create rotation quaternion from surface normal

### Plugin Structure

The `EditorPlugin` composes these sub-plugins:

- **editor/** - Core editor functionality
  - `EditorStatePlugin` - Modal state machine (`EditorMode`) and `TransformOperation` resource
  - `EditorInputPlugin` - Keyboard handling for mode/operation switching
  - `EditorCameraPlugin` - Orbit camera controls
  - `SplineEditPlugin` - Spline control point editing (bridges bevy_spline_3d)

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

---

## Reusable Patterns and Components

### Input Handling

**`should_process_input()`** (`src/utils.rs:28`)

Guards against processing keyboard input when editor is disabled or egui wants focus:

```rust
if !should_process_input(&editor_state, &mut contexts) {
    return;
}
```

### Theme and UI Styling (`src/ui/theme.rs`)

**Color Palette** - Semantic color constants for consistent theming:
```rust
use crate::ui::theme::colors;

colors::TEXT_PRIMARY      // Main text
colors::TEXT_SECONDARY    // Secondary text
colors::TEXT_MUTED        // Disabled/hint text
colors::ACCENT_BLUE       // Links, selections
colors::ACCENT_GREEN      // Success, add actions
colors::ACCENT_ORANGE     // Warnings, highlights
colors::AXIS_X/Y/Z        // Transform gizmo colors
colors::BG_DARK           // Window backgrounds
colors::SELECTION_BG      // Selected item background
```

**Window Frame Helpers**:
```rust
use crate::ui::theme::{window_frame, popup_frame};

// Standard window styling
egui::Window::new("Title")
    .frame(window_frame(&ctx.style()))
    // ...

// Popup/tooltip styling
egui::Frame::popup(&ctx.style())
    .fill(popup_frame(&ctx.style()).fill)
```

**Dialog Helpers**:
```rust
use crate::ui::theme::{draw_centered_dialog, draw_error_dialog, DialogResult};

// Generic centered modal dialog
let result = draw_centered_dialog(ctx, "Title", [400.0, 200.0], |ui| {
    ui.label("Content here");
    if ui.button("OK").clicked() {
        return DialogResult::Confirmed;
    }
    DialogResult::None
});

// Simple error dialog
if draw_error_dialog(ctx, "Error", "Something went wrong") {
    // Dialog closed
}
```

### Fuzzy Search Palette (`src/ui/fuzzy_palette.rs`)

Reusable fuzzy search widget for searchable lists:

**1. Implement `PaletteItem` trait for your items:**
```rust
use crate::ui::fuzzy_palette::PaletteItem;

struct MyItem {
    name: String,
    category: String,
}

impl PaletteItem for MyItem {
    fn label(&self) -> &str { &self.name }
    fn category(&self) -> Option<&str> { Some(&self.category) }
    fn is_enabled(&self) -> bool { true }
    fn suffix(&self) -> Option<&str> { None }
    fn keywords(&self) -> &[String] { &[] }
}
```

**2. Use `draw_fuzzy_palette()`:**
```rust
use crate::ui::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteResult, PaletteState};

let mut palette_state = PaletteState::default();
let config = PaletteConfig {
    title: "SEARCH",
    title_color: colors::ACCENT_BLUE,
    subtitle: "Find items",
    hint_text: "Type to search...",
    action_label: "select",
    size: [400.0, 300.0],
    show_categories: true,
};

match draw_fuzzy_palette(ctx, &mut palette_state, &items, &config) {
    PaletteResult::Selected(index) => { /* handle selection */ }
    PaletteResult::Closed => { /* handle close */ }
    PaletteResult::Open => { /* still open */ }
}
```

**3. Or use `fuzzy_filter()` directly:**
```rust
use crate::ui::fuzzy_palette::fuzzy_filter;

let filtered = fuzzy_filter(&items, &query);
for item in filtered {
    println!("{} (score: {})", item.item.label(), item.score);
}
```

**Pre-built item types:**
- `SimpleItem` - Just a label
- `CategorizedItem` - Label + category + enabled + suffix
- `KeywordItem` - Label + keywords + optional category

### State Initialization Pattern

UI state structs follow a consistent reset pattern:

```rust
// CommandPaletteState has helper methods
state.open_commands();           // Opens in command mode
state.open_insert();             // Opens in insert mode
state.open_component_search();   // Opens for component search
state.open_add_component(entity); // Opens for adding components

// Or use the standalone helper
open_add_component_palette(&mut state, entity);

// PaletteState has reset()
palette_state.reset();  // Clears query, resets index, sets just_opened

// ComponentBrowserState
browser_state.open_for_entity(entity);
```

### Inspector Property Helpers (`src/ui/inspector.rs`)

Reusable property row drawing:
```rust
// These are private but show the pattern for custom inspectors
draw_color_row(ui, &mut color);           // Color picker row
draw_checkbox_row(ui, "Label", &mut val); // Checkbox row
draw_drag_row(ui, "Label", &mut val, speed, range); // Drag value row
```

### Surface Placement Utilities (`src/utils.rs`)

For placing objects on surfaces:
```rust
use crate::utils::{get_half_height_along_normal, rotation_from_normal};

// Get height offset for surface placement
let offset = get_half_height_along_normal(collider.as_ref(), surface_normal);
let position = hit_point + surface_normal * offset;

// Align object rotation to surface
let rotation = rotation_from_normal(surface_normal);
```

### Constants (`src/constants.rs`)

Centralized configuration values:

```rust
use crate::constants::{primitive_colors, light_colors, preview_colors, physics, sizes};

// Primitive colors
let color = primitive_colors::for_shape(PrimitiveShape::Cube);

// Light defaults
let intensity = light_colors::POINT_DEFAULT_INTENSITY;

// Preview colors for insert mode
let preview = preview_colors::GENERIC;

// Physics constants
let radius = physics::LIGHT_COLLIDER_RADIUS;

// Size defaults
let distance = sizes::INSERT_DEFAULT_DISTANCE;
```

---

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

`SpawnEntityKind` variants: `Primitive(PrimitiveShape)`, `Group`, `PointLight`, `DirectionalLight`, `Spline(SplineType)`

`PrimitiveShape` provides factory methods:
- `create_mesh()` - Returns the mesh for this shape
- `create_material()` - Returns a StandardMaterial with the shape's default color
- `create_collider()` - Returns the physics collider
- `default_color()` - Returns the standard color from `constants::primitive_colors`

### Scene Snapshots

Use `build_editor_scene()` for consistent scene building (single source of truth for serializable components):

```rust
let scene = build_editor_scene(world, entity_ids.into_iter());
```

### Entity Markers

- `SceneEntity` - Part of the editable scene (saved/loaded)
- `Selected` - Currently selected
- `PrefabInstance` / `PrefabRoot` - Prefab system markers
- `PrimitiveMarker` - Identifies primitive shape type
- `GroupMarker` - Empty container for organizing entities
- `SceneLightMarker` / `DirectionalLightMarker` - Light configuration that persists to scene files
- `SplineMarker` - Identifies spline entities (from bevy_spline_3d integration)
- `GltfLoaded` / `SceneSourceLoaded` - Marks children loaded from external files
