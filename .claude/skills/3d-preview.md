# 3D Preview Viewport

Use this skill when the user wants to add a new render-to-texture 3D preview for the editor UI. Examples: "add a preview for prefabs", "add a texture preview sphere", "add a 3D thumbnail for the asset browser", "show a rotating preview of the selected entity".

## Architecture

3D previews render a small scene to an offscreen texture, then display that texture in an egui panel. All previews share common infrastructure from `src/ui/preview_common.rs`:

- **`PreviewTexture`** — embeddable struct holding `render_texture: Handle<Image>` + `egui_texture_id: Option<TextureId>`
- **`PreviewSceneConfig`** — parameterizes camera, lights, background, ground plane
- **`spawn_preview_scene()`** — creates camera, directional light, optional point light fill, optional ground plane on a dedicated `RenderLayer`
- **`register_preview_egui_texture()`** — one-shot registration with bevy_egui
- **`apply_preview_rotation()`** — Y-axis rotation preserving fit-to-frame scale/translation
- **`fit_transform_from_mesh()`** / **`fit_transform_from_extents()`** — AABB-based centering and uniform scaling

Two preset configurations:
- `PreviewSceneConfig::object_preview(layer, order)` — transparent background, 2000 lux + point light fill, no ground (for insert palette, GLTF browser)
- `PreviewSceneConfig::material_studio(layer, order)` — dark gray background, 3000 lux, ground plane (for material editor)

## File Locations

- **Shared infrastructure**: `src/ui/preview_common.rs`
- **Existing previews**: `src/ui/insert_preview.rs`, `src/ui/gltf_preview.rs`, `src/ui/material_preview.rs`
- **Registration**: `src/ui/mod.rs` — add `pub mod` and plugin to `UiPlugin`

## Render Layer Allocation

Each preview needs a unique render layer to avoid cross-contamination:

| Layer | Used by |
|-------|---------|
| 27 | GLTF preview |
| 28 | Insert preview |
| 29 | Preset palette preview |
| 30 | Material editor preview |
| 31 | Outliner (reserved) |

New previews should use **layer 26 or lower** (decrement from 27).

## Step-by-Step

### 1. Create the Preview Module

Create `src/ui/my_preview.rs`:

```rust
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_egui::EguiUserTextures;

use crate::ui::preview_common::{
    apply_preview_rotation, fit_transform_from_mesh, register_preview_egui_texture,
    spawn_preview_scene, PreviewSceneConfig, PreviewTexture, PREVIEW_ROTATION_SPEED,
};

const MY_PREVIEW_RENDER_LAYER: usize = 26; // Pick an unused layer

#[derive(Component)]
struct MyPreviewCamera;

#[derive(Component)]
struct MyPreviewMesh;

#[derive(Component)]
struct MyPreviewLight;

#[derive(Resource)]
pub struct MyPreviewState {
    pub texture: PreviewTexture,
    rotation_angle: f32,
    fit_scale: Vec3,
    fit_translation: Vec3,
    // Add change-detection fields as needed
}

pub struct MyPreviewPlugin;

impl Plugin for MyPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, setup_my_preview)
            .add_systems(Update, (
                register_my_preview_texture,
                sync_my_preview,     // Your unique update logic
                rotate_my_preview,
            ));
    }
}
```

### 2. Setup System

```rust
fn setup_my_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Choose a config preset or customize
    let config = PreviewSceneConfig::object_preview(MY_PREVIEW_RENDER_LAYER, -6);
    let layer = RenderLayers::layer(MY_PREVIEW_RENDER_LAYER);
    let handles = spawn_preview_scene(
        &mut commands, &mut images, &mut meshes, &mut materials, &config,
    );

    // Tag spawned entities with local markers
    commands.entity(handles.camera).insert(MyPreviewCamera);
    for &light in &handles.lights {
        commands.entity(light).insert(MyPreviewLight);
    }

    // Spawn your preview object (mesh, sphere, etc.)
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        ..default()
    });
    commands.spawn((
        MyPreviewMesh,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        layer,
    ));

    commands.insert_resource(MyPreviewState {
        texture: PreviewTexture {
            render_texture: handles.render_texture,
            egui_texture_id: None,
        },
        rotation_angle: 0.0,
        fit_scale: Vec3::ONE,
        fit_translation: Vec3::ZERO,
    });
}
```

### 3. Registration System (Boilerplate)

```rust
fn register_my_preview_texture(
    mut state: ResMut<MyPreviewState>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    register_preview_egui_texture(&mut state.texture, &mut user_textures);
}
```

### 4. Sync System (Your Unique Logic)

This is the only part that varies significantly between previews:

```rust
fn sync_my_preview(
    mut state: ResMut<MyPreviewState>,
    mesh_entity: Query<Entity, With<MyPreviewMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    // Add whatever queries/resources drive your preview
) {
    // Change detection — skip if nothing changed
    // ...

    let Ok(entity) = mesh_entity.single() else { return };

    // Build new mesh + material based on your state
    let mesh: Mesh = Sphere::new(0.5).into();

    // Compute fit-to-frame transform
    let fit = fit_transform_from_mesh(&mesh);
    state.fit_scale = fit.scale;
    state.fit_translation = fit.translation;
    state.rotation_angle = 0.0;

    commands.entity(entity).insert((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial::default())),
        fit,
    ));
}
```

### 5. Rotation System (Boilerplate)

```rust
fn rotate_my_preview(
    mut state: ResMut<MyPreviewState>,
    time: Res<Time>,
    mut mesh: Query<&mut Transform, With<MyPreviewMesh>>,
    // Add a gate condition (e.g., palette open check)
) {
    // Optional: guard — only rotate when visible
    // if !palette_state.open { return; }

    state.rotation_angle += PREVIEW_ROTATION_SPEED * time.delta_secs();
    if let Ok(mut transform) = mesh.single_mut() {
        apply_preview_rotation(
            state.rotation_angle,
            &mut transform,
            state.fit_scale,
            state.fit_translation,
        );
    }
}
```

### 6. Display in egui

From any UI system that has access to `MyPreviewState`:

```rust
if let Some(tex_id) = my_preview_state.texture.egui_texture_id {
    let size = ui.available_width().min(220.0);
    ui.image(egui::load::SizedTexture::new(tex_id, [size, size]));
}
```

### 7. Register in `src/ui/mod.rs`

```rust
pub mod my_preview;

// In UiPlugin::build():
.add_plugins(my_preview::MyPreviewPlugin)
```

## Existing Preview Reference

| Preview | File | Layer | Order | Config | Unique Logic |
|---------|------|-------|-------|--------|-------------|
| Insert | `insert_preview.rs` | 28 | -4 | `object_preview` | Swaps mesh per `InsertPreviewKind`, uses `fit_transform_from_mesh` |
| GLTF | `gltf_preview.rs` | 27 | -5 | `object_preview` | Loads GLTF scenes as children, propagates render layers, uses `fit_transform_from_extents` with world-space AABBs |
| Material | `material_preview.rs` | 30 | -2 | `material_studio` | Syncs sphere material from selected entity's `MaterialRef`, deferred via `Commands::queue` |
| Preset | `material_preview.rs` | 29 | -3 | `material_studio` | Syncs sphere material from palette selection's `MaterialDefinition` |

## Key Patterns

- **Setup runs in `PreStartup`** so preview textures exist before any UI system needs them
- **Registration runs every frame in `Update`** until successful (one-shot pattern)
- **State resources are `pub`** so UI panels can read `texture.egui_texture_id` and write change-detection fields
- **Marker components are private** — only the preview module queries its own entities
- **Material previews don't use fit/rotation helpers** for scale/translation since the sphere is always the same size — they just rotate `transform.rotation`
- **GLTF preview has async loading** — waits frames for scene instantiation, propagates render layers to children, collects world-space AABBs from descendants

## Checklist

- [ ] Unique render layer chosen (check table above for conflicts)
- [ ] Camera order is negative and unique (check existing: -2, -3, -4, -5)
- [ ] Module created in `src/ui/`
- [ ] `pub mod` added to `src/ui/mod.rs`
- [ ] Plugin registered in `UiPlugin::build()`
- [ ] Setup system in `PreStartup`
- [ ] Registration + sync + rotation systems in `Update`
- [ ] State resource has `pub texture: PreviewTexture`
- [ ] Preview mesh/object spawned on the correct `RenderLayers`
- [ ] Consumer UI reads `state.texture.egui_texture_id`
