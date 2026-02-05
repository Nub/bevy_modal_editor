# Editor Plugin Scaffolding

Use this skill when the user wants to add a new self-contained feature module to the editor. Examples: "add a terrain system", "add a debug visualization plugin", "add a measurement tool", "add a snapping system".

This is for general editor plugins — new feature modules that bundle their own state, systems, and events. For operations with undo/redo specifically, use `editor-operation`. For UI panels, use `ui-panel`.

## Architecture

Every editor feature is a Bevy `Plugin` that gets registered in `EditorPlugin::build()`. Plugins are organized by domain:

- `src/editor/` — Core editor features (camera, input, modes, insert)
- `src/scene/` — Scene data and entity management
- `src/commands/` — Undo/redo and operations
- `src/gizmos/` — Visual overlays
- `src/selection/` — Entity selection
- `src/materials/` — Material system
- `src/ui/` — UI panels (use `ui-panel` skill instead)

Choose the directory that best matches your feature's domain.

## Files to Modify

| # | File | What to add |
|---|------|-------------|
| 1 | `src/{domain}/my_feature.rs` | New module with Plugin + State + Systems |
| 2 | `src/{domain}/mod.rs` | `mod my_feature;` + `pub use my_feature::*;` |
| 3 | Parent plugin file | Register in the domain's parent plugin |

The parent plugin depends on the domain:
- `src/editor/` → registered in `EditorPlugin::build()` (`src/editor/plugin.rs`)
- `src/scene/` → registered in `ScenePlugin::build()` (`src/scene/mod.rs`)
- `src/commands/` → registered in `CommandsPlugin::build()` (`src/commands/mod.rs`)
- `src/gizmos/` → registered in `EditorGizmosPlugin::build()` (`src/gizmos/mod.rs`)

## Step-by-Step

### Step 1: Create the Module File

Create `src/{domain}/my_feature.rs`:

```rust
use bevy::prelude::*;

use crate::editor::EditorState;

/// Plugin for [describe feature]
pub struct MyFeaturePlugin;

impl Plugin for MyFeaturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MyFeatureState>()
            .add_message::<MyFeatureEvent>()
            .add_systems(Update, (
                handle_my_feature_input,
                handle_my_feature,
            ));
    }
}

/// Persistent state for the feature
#[derive(Resource, Default)]
pub struct MyFeatureState {
    pub enabled: bool,
    // feature-specific state
}

/// Event to trigger the feature
#[derive(Message)]
pub struct MyFeatureEvent;
```

### Step 2: Add Input Handler (If Needed)

If the feature responds to keyboard input:

```rust
use bevy_egui::EguiContexts;
use crate::editor::{EditorMode, EditorState};
use crate::utils::should_process_input;

fn handle_my_feature_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    editor_mode: Res<State<EditorMode>>,
    mut events: MessageWriter<MyFeatureEvent>,
    mut contexts: EguiContexts,
) {
    // REQUIRED: skip when editor disabled or egui has focus
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Optional: restrict to specific modes
    // if *editor_mode.get() != EditorMode::Edit { return; }

    // Modifier keys
    let ctrl = keyboard.pressed(KeyCode::ControlLeft)
            || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft)
             || keyboard.pressed(KeyCode::ShiftRight);

    if ctrl && keyboard.just_pressed(KeyCode::KeyM) {
        events.write(MyFeatureEvent);
    }
}
```

**Key patterns:**
- Always call `should_process_input()` first — guards against disabled editor and egui focus
- Use `just_pressed` for single-fire actions, `pressed` for held keys
- Check both Left and Right variants for modifier keys
- Separate input detection from logic (testability)

### Step 3: Add Feature Logic

```rust
use crate::scene::SceneEntity;
use crate::selection::Selected;

fn handle_my_feature(
    mut events: MessageReader<MyFeatureEvent>,
    mut state: ResMut<MyFeatureState>,
    selected: Query<(Entity, &Transform), (With<Selected>, With<SceneEntity>)>,
) {
    for _event in events.read() {
        // Feature logic here
        for (entity, transform) in selected.iter() {
            info!("Processing entity {:?} at {:?}", entity, transform.translation);
        }
    }
}
```

**If the feature modifies scene entities**, take a snapshot for undo support:

```rust
use crate::commands::TakeSnapshotCommand;

fn handle_my_feature(
    mut events: MessageReader<MyFeatureEvent>,
    mut commands: Commands,
    mut selected: Query<&mut Transform, (With<Selected>, With<SceneEntity>)>,
) {
    for _event in events.read() {
        // ALWAYS snapshot BEFORE mutation
        commands.queue(TakeSnapshotCommand {
            description: "My feature operation".to_string(),
        });

        for mut transform in selected.iter_mut() {
            // modify transform...
        }
    }
}
```

### Step 4: Register in Parent Module

**A) Declare in `mod.rs`:**

```rust
// src/{domain}/mod.rs
mod my_feature;
pub use my_feature::*;
```

**B) Register in parent plugin:**

For `src/editor/` features, add to `src/editor/plugin.rs`:
```rust
use super::my_feature::MyFeaturePlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        // ... existing plugins ...
        .add_plugins(MyFeaturePlugin)
    }
}
```

For `src/scene/` features, add to `ScenePlugin` in `src/scene/mod.rs`.
For `src/gizmos/` features, add to `EditorGizmosPlugin` in `src/gizmos/mod.rs`.

## Common Patterns

### Reactive Systems (Run When State Changes)

```rust
app.add_systems(Update, my_system.run_if(resource_changed::<MyState>));
```

### Mode-Gated Systems

```rust
app.add_systems(Update, my_system.run_if(in_state(EditorMode::Edit)));
```

### Exclusive World Access

For systems that need to read/write arbitrary components:

```rust
fn my_exclusive_system(world: &mut World) {
    let state = world.resource::<MyFeatureState>().clone();

    // Query entities
    let entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).collect()
    };

    // Modify entities
    for entity in entities {
        if let Some(mut transform) = world.get_mut::<Transform>(entity) {
            // ...
        }
    }
}
```

### Events (Message Pattern)

This project uses `#[derive(Message)]` with `MessageReader`/`MessageWriter`, NOT `#[derive(Event)]`:

```rust
#[derive(Message)]
pub struct MyEvent {
    pub data: Vec3,
}

// Register
app.add_message::<MyEvent>();

// Send
fn sender(mut events: MessageWriter<MyEvent>) {
    events.write(MyEvent { data: Vec3::ZERO });
}

// Receive
fn receiver(mut events: MessageReader<MyEvent>) {
    for event in events.read() {
        // handle event.data
    }
}
```

### Grouped Event Writers (SystemParam)

When a system needs multiple event writers:

```rust
#[derive(SystemParam)]
pub struct MyFeatureEvents<'w> {
    pub spawn: MessageWriter<'w, SpawnEntityEvent>,
    pub delete: MessageWriter<'w, DeleteSelectedEvent>,
    pub my_event: MessageWriter<'w, MyFeatureEvent>,
}
```

## Checklist

- [ ] Module file created in correct domain directory
- [ ] Plugin struct with `impl Plugin` and `build()` method
- [ ] State resource defined and initialized with `init_resource`
- [ ] Events defined with `#[derive(Message)]` and registered with `add_message`
- [ ] Input handler uses `should_process_input()` guard
- [ ] Scene mutations preceded by `TakeSnapshotCommand`
- [ ] Module declared in `mod.rs` (`mod my_feature;`)
- [ ] Public items re-exported (`pub use my_feature::*;`)
- [ ] Plugin registered in parent plugin's `build()` method
