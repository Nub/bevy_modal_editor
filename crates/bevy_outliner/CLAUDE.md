# CLAUDE.md - bevy_outliner

## Build & Run Commands

```bash
# Enter development environment (required for Bevy dependencies)
nix develop

# Build the library
cargo build

# Run the basic example
cargo run --example basic

# Run the egui example (interactive controls)
cargo run --example with_egui --features bevy_egui

# Run tests
cargo test

# Check for errors without building
cargo check
```

## Architecture Overview

bevy_outliner is a silhouette-based object outlining plugin for Bevy 0.18. It uses a Jump Flood Algorithm (JFA) style distance field approximation to render smooth outlines around specific objects.

### How It Works

1. **Silhouette Rendering**: Objects with `MeshOutline` component are copied to render layer 31 with a white unlit material
2. **Separate Camera**: A silhouette camera renders only layer 31 objects to a dedicated texture
3. **Post-Processing**: A custom render node (`OutlineNode`) runs after tonemapping, sampling both the scene and silhouette textures
4. **Distance Field**: The shader computes distance from each pixel to the nearest silhouette edge using JFA-style multi-pass sampling
5. **Compositing**: Outline color is blended over the scene based on distance from silhouette edges

### Key Files

- `src/lib.rs` - Plugin entry point, embeds shader and registers systems
- `src/components.rs` - `MeshOutline` (per-object) and `OutlineSettings` (per-camera) components
- `src/jfa_material.rs` - Core rendering: custom ViewNode, pipeline setup, extraction systems
- `src/shaders/jfa_composite.wgsl` - WGSL shader with JFA distance field computation

### Component Usage

```rust
// Add outline to any mesh
commands.spawn((
    Mesh3d(mesh_handle),
    MeshMaterial3d(material_handle),
    MeshOutline::default(),  // Orange outline, 5px width
));

// Camera must have OutlineSettings to render outlines
commands.spawn((
    Camera3d::default(),
    OutlineSettings::default(),
));
```

### Render Pipeline

```
Main Camera Render -> Tonemapping -> OutlineNode -> EndMainPassPostProcessing
                                         |
                    Silhouette Camera ---+
                    (renders to texture)
```

## Code Style

- Uses Bevy 0.18 idioms (Mesh3d, MeshMaterial3d, Camera3d components)
- Render world extraction via `Extract` system and `RenderEntity` lookup
- Custom `ViewNode` implementation for post-processing
- Shader uses `#import` for Bevy's fullscreen vertex shader
