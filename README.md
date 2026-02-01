# Bevy Avian3D Editor

A level editor for Bevy games using the Avian3D physics engine. Features vim-like modal editing for efficient scene construction with physics-enabled primitives and prefabs.

## Features

- **Modal Editing** - Vim-inspired View/Edit modes for focused workflows
- **Command Palette** - Fuzzy searchable commands (press `C`)
- **Object Search** - Find objects in scene by name (press `F`)
- **Transform Tools** - Translate, rotate, and scale with axis constraints
- **Grid & Rotation Snapping** - Configurable snap increments
- **Camera Marks** - Save and recall camera positions (1-9 keys)
- **Primitive Shapes** - Cube, Sphere, Cylinder, Capsule, Plane
- **Prefab System** - Reusable entity templates
- **Pattern Layouts** - Linear and circular array duplication
- **Scene Serialization** - Save/load scenes in RON format
- **Undo/Redo** - Command history for all operations

## Requirements

- [Nix](https://nixos.org/) (for dependency management)
- Rust 2024 edition

## Building

```bash
# Enter development environment
nix develop

# Run the editor
cargo run

# Build release
cargo build --release
```

## Controls

### General
| Key | Action |
|-----|--------|
| `C` | Open command palette |
| `F` | Find object in scene |
| `V` | Toggle View/Edit mode |
| `Esc` | Return to View mode / Cancel |

### View Mode - Camera
| Key | Action |
|-----|--------|
| `W/A/S/D` | Move camera |
| `Space/Ctrl` | Move up/down |
| `Shift` | Move faster |
| `Right Mouse` | Look around |
| `1-9` | Jump to camera mark |
| `Shift+1-9` | Set camera mark |
| `` ` `` | Jump to last position |

### View Mode - Selection
| Key | Action |
|-----|--------|
| `Left Click` | Select object |
| `Delete` | Delete selected |

### Edit Mode - Transform
| Key | Action |
|-----|--------|
| `Q` | Translate tool |
| `W` | Rotate tool |
| `E` | Scale tool |
| `A` | Constrain to X axis |
| `S` | Constrain to Y axis |
| `D` | Constrain to Z axis |
| `J/K` | Step transform -/+ |

## Project Structure

```
src/
├── main.rs              # Application entry point
├── lib.rs               # Library root
├── editor/              # Core editor functionality
│   ├── plugin.rs        # Main EditorPlugin
│   ├── state.rs         # EditorMode, TransformOperation
│   ├── input.rs         # Keyboard input handling
│   ├── camera.rs        # Fly camera and presets
│   └── marks.rs         # Camera mark system
├── selection/           # Entity selection via raycasting
├── commands/            # Undo/redo command system
├── scene/               # Scene management
│   ├── primitives.rs    # Primitive shape spawning
│   └── serialization.rs # RON save/load
├── prefabs/             # Prefab asset system
├── gizmos/              # Visual editor overlays
├── patterns/            # Pattern-based duplication
└── ui/                  # egui interface
    ├── panels.rs        # Status bar
    ├── hierarchy.rs     # Entity tree view
    ├── inspector.rs     # Component editor
    ├── command_palette.rs
    ├── find_object.rs
    └── view_gizmo.rs    # 3D orientation widget
```

## License

MIT OR Apache-2.0
