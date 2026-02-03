# Bevy Modal Editor

A keyboard-first level editor for Bevy games using the Avian3D physics engine. Inspired by vim's modal editing philosophy, this editor lets you build 3D scenes efficiently without ever touching the mouse.



[![Bevy Modal Editor Screenshot](bevy_modal_editor.png)](https://github.com/Nub/bevy_modal_editor/raw/main/bevy_modal_editor.mp4)

## Why Keyboard-First?

Traditional 3D editors require constant switching between keyboard and mouse—selecting tools from toolbars, clicking through menus, dragging widgets. This context switching breaks your flow and slows you down.

Modal editing solves this by organizing commands into modes, each with its own focused set of single-key commands:

- **View mode**: Navigate the scene and select objects
- **Edit mode**: Transform selected objects with precision
- **Insert mode**: Add new objects to the scene
- **Hierarchy mode**: Navigate and organize scene structure
- **Inspector mode**: Edit component properties
- **Blockout mode**: Rapid tile-based level prototyping

Every action has a keyboard shortcut. The command palette (`C`) gives you fuzzy-searchable access to all commands. Press `?` at any time to see all available hotkeys for your current mode.

## Quick Start

```bash
# Enter development environment
nix develop

# Run the editor
cargo run
```

### Loading a Demo Scene

1. Press `C` to open the command palette
2. Type "demo" and press Enter to spawn a demo scene with primitives and physics objects
3. Use the command palette to unpause physics and watch the objects interact

### Basic Editing Workflow

**Adding objects:**
1. Press `I` to enter Insert mode—the command palette opens automatically
2. Type the object you want (e.g., "cube", "stairs", "light", "spline")
3. Press Enter to select, then move your view to position the preview
4. Click to place the object, or Shift+Click to place multiple
5. Press `Escape` to cancel

**Transforming objects:**
1. Click an object to select it (or use `F` to search by name)
2. Press `E` to enter Edit mode
3. Press `Q` for translate, `W` for rotate, or `E` for scale
4. Press `A`, `S`, or `D` to constrain to X, Y, or Z axis
5. Move the mouse to transform, or use `J`/`K` for precise step adjustments
6. Hold `Alt` while dragging to snap edges to nearby objects
7. Click to confirm or press `Escape` to cancel

**Duplicating and nudging:**
1. Select one or more objects
2. Press `Ctrl+D` to duplicate in-place
3. Use arrow keys to nudge selected objects on the XZ plane

**Organizing the scene:**
1. Press `H` to enter Hierarchy mode
2. Use `F` to filter entities by name
3. Press `/` to search and jump to any object
4. Press `L` to look at the selected object
5. Drag entities to reparent them, or use `G` to group selected objects

**Inspecting components:**
1. Select an object and press `O` to enter Inspector mode
2. Press `/` to search for a component to edit
3. Press `I` to add new components to the selected entity
4. Press `X` to remove a component

**Blockout mode (rapid prototyping):**
1. Place an initial object, select it
2. Press `B` to enter Blockout mode
3. Press `1-5` to select shape (Cube, Stairs, Ramp, Arch, L-Shape)
4. Use `W/A/S/D/Q/E` to select which face to snap to
5. Press `R` to rotate the preview 90 degrees
6. Press `Enter` to place—the new tile becomes the anchor for chaining
7. Continue placing or press `Escape` to exit

### Saving and Loading

- Press `C` to open the command palette, then type "save" or "load"
- Scenes are saved in RON format and can be version controlled

## Using as a Plugin

Add the editor to your Bevy game:

```rust
use bevy::prelude::*;
use bevy_modal_editor::EditorPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EditorPlugin::default())
        .run();
}
```

The editor automatically detects if you already have EguiPlugin or Avian3D physics set up and won't duplicate them.

Press `F10` to toggle the editor UI on/off during gameplay.

## Features

### Core
- **Modal Editing** - Vim-inspired modes for focused, efficient workflows
- **Command Palette** - Fuzzy searchable commands (`C`)
- **Object Search** - Find objects by name (`F`)
- **Undo/Redo** - Full command history (`U` to undo, `Ctrl+R` to redo)
- **Scene Serialization** - Save/load scenes in RON format

### Objects
- **Primitive Shapes** - Cube, Sphere, Cylinder, Capsule, Plane
- **Blockout Shapes** - Stairs, Ramp, Arch, L-Shape with editable parameters
- **Point & Directional Lights** - Full lighting control with shadows
- **Fog Volumes** - Volumetric fog for atmosphere
- **GLTF/GLB Models** - Import 3D models
- **Nested Scenes** - Import RON scene files as sub-scenes
- **Entity Groups** - Organize objects hierarchically

### Splines
- **Spline Types** - Cubic Bezier, Catmull-Rom, B-Spline
- **Control Point Editing** - Edit points directly in Edit mode
- **Distributions** - Clone objects along splines with configurable spacing

### Transform Tools
- **Translate/Rotate/Scale** - Standard transform operations
- **Axis Constraints** - Lock to X, Y, or Z axis
- **Grid & Rotation Snapping** - Configurable snap increments
- **Edge Snapping** - Align edges to nearby objects (Alt+Drag)
- **Place Mode** - Raycast-based surface placement
- **Snap to Object** - Align to surface normal, center, or vertex

### Camera
- **Fly Camera** - WASD + mouse navigation
- **Camera Marks** - Save and recall positions (1-9, Shift+1-9)
- **Look At** - Focus on selected object (`L`)
- **Last Position** - Return to previous view (backtick)

### Workflow
- **Quick Duplicate** - Clone selected objects (`Ctrl+D`)
- **Arrow Key Nudge** - Move selected objects by grid step
- **Preview Mode** - Hide all gizmos and debug rendering (`P`)
- **Physics Simulation** - Toggle physics on/off via command palette

## Keyboard Reference

### Mode Switching
| Key | Action |
|-----|--------|
| `E` | Edit mode |
| `I` | Insert mode |
| `O` | Object Inspector mode |
| `H` | Hierarchy mode |
| `B` | Blockout mode |
| `Shift+key` | Switch from any mode |
| `Escape` | Return to View mode |

### View Mode
| Key | Action |
|-----|--------|
| `W/A/S/D` | Move camera |
| `Space/Ctrl` | Up/down (relative) |
| `Shift` | Move faster |
| `Right Mouse` | Look around |
| `L` | Look at selected |
| `1-9` | Jump to mark |
| `Shift+1-9` | Set mark |
| `` ` `` | Last position |
| `M` | Toggle measurements |

### Edit Mode
| Key | Action |
|-----|--------|
| `Q` | Translate |
| `W` | Rotate |
| `E` | Scale |
| `R` | Place (raycast) |
| `T` | Snap to object |
| `A/S/D` | Constrain X/Y/Z |
| `J/K` | Step -/+ |
| `Alt+Drag` | Edge snap |

### Selection & Edit
| Key | Action |
|-----|--------|
| `Click` | Select object |
| `Shift+Click` | Multi-select |
| `Ctrl+D` | Duplicate |
| `Arrow Keys` | Nudge selected |
| `G` | Group selected |
| `Delete` or `X` | Delete selected |

### Commands
| Key | Action |
|-----|--------|
| `C` | Command palette |
| `F` | Find object |
| `U` | Undo |
| `Ctrl+R` | Redo |
| `P` | Preview mode |
| `?` | Help |

## Tools

- [Nix](https://nixos.org/) (for dependency management)
- Rust 2024 edition


## License

MIT OR Apache-2.0
