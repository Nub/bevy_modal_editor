# bevy_outliner

A silhouette-based object outlining plugin for [Bevy](https://bevyengine.org/) 0.18.

Add outlines to any mesh by simply adding the `MeshOutline` component.

## Features

- Per-object outlining - only meshes with `MeshOutline` get outlined
- Configurable outline color and width
- Smooth corners using JFA-style distance field sampling
- Compatible with HDR rendering
- Works with bevy_egui

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
bevy_outliner = { git = "https://github.com/your-username/bevy_outliner" }
```

## Quick Start

```rust
use bevy::prelude::*;
use bevy_outliner::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, OutlinePlugin))
        .add_systems(Startup, setup)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Spawn a cube with an orange outline
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::default())),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.2, 0.2))),
        MeshOutline::default(),
    ));

    // Camera must have OutlineSettings to render outlines
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        OutlineSettings::default(),
    ));
}
```

## Components

### `MeshOutline`

Add to any entity with a mesh to give it an outline.

```rust
MeshOutline {
    color: LinearRgba::new(1.0, 0.5, 0.0, 1.0), // Orange
    width: 5.0, // Pixels
}
```

Builder methods:
- `MeshOutline::default()` - Orange outline, 5px width
- `MeshOutline::new(color, width)` - Custom color and width
- `MeshOutline::with_color(color)` - Custom color, default width
- `MeshOutline::with_width(width)` - Default color, custom width

### `OutlineSettings`

Add to cameras that should render outlines.

```rust
OutlineSettings {
    max_width: 64,  // Maximum supported outline width
    enabled: true,  // Toggle outlines on/off
}
```

## Examples

```bash
# Basic example
cargo run --example basic

# With egui controls
cargo run --example with_egui
```

## How It Works

1. Objects with `MeshOutline` are rendered to a separate silhouette texture using a white unlit material
2. A post-processing shader computes the distance from each pixel to the nearest silhouette edge
3. Pixels within the outline width are colored with the outline color
4. The result is composited over the main scene

## Bevy Compatibility

| bevy_outliner | Bevy |
|---------------|------|
| 0.1           | 0.18 |

## License

MIT OR Apache-2.0
