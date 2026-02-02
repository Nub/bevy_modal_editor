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
3. Use the command pallette unpause physics and watch the objects interact

### Basic Editing Workflow

**Adding objects:**
1. Press `I` to enter Insert mode—the command palette opens automatically
2. Type the object you want (e.g., "cube", "sphere", "light")
3. Press Enter to select, then move your view to position the preview
4. Click to place the object, or press `Escape` to cancel

**Transforming objects:**
1. Click an object to select it (or use `F` to search by name)
2. Press `E` to enter Edit mode
3. Press `Q` for translate, `W` for rotate, or `E` for scale
4. Press `A`, `S`, or `D` to constrain to X, Y, or Z axis
5. Move the mouse to transform, or use `J`/`K` for precise step adjustments
6. Click to confirm or press `Escape` to cancel

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

### Saving and Loading

- Press `C` and type "save" to save your scene
- Press `C` and type "load" to load a previously saved scene
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

- **Modal Editing** - Vim-inspired modes for focused, efficient workflows
- **Command Palette** - Fuzzy searchable commands (`C`)
- **Object Search** - Find objects by name (`F`)
- **Transform Tools** - Translate, rotate, scale with axis constraints
- **Grid & Rotation Snapping** - Configurable snap increments
- **Camera Marks** - Save and recall camera positions (1-9 keys)
- **Primitive Shapes** - Cube, Sphere, Cylinder, Capsule, Plane
- **Point & Directional Lights** - Full lighting control
- **Entity Groups** - Organize objects hierarchically
- **Pattern Layouts** - Linear and circular array duplication
- **Scene Serialization** - Save/load scenes in RON format
- **Undo/Redo** - Full command history (`U` to undo, `Ctrl+R` to redo)
- **Physics Integration** - All objects are physics-enabled with Avian3D

## Tools

- [Nix](https://nixos.org/) (for dependency management)
- Rust 2024 edition


## License

MIT OR Apache-2.0
