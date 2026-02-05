# Editor Operation Scaffolding

Use this skill when the user wants to create a new editor operation — an action that modifies scene entities and supports undo/redo. Examples: "add a flip operation", "add an align operation", "add a snap-to-grid operation".

## Architecture

This editor uses a **snapshot-based undo/redo system**, NOT a command pattern with discrete undo logic. The flow is:

1. Define a `Message` event struct for the operation
2. Write an input handler system that sends the event on keypress
3. Write a handler system that:
   - Reads the event
   - Queues `TakeSnapshotCommand` (captures scene state BEFORE mutation)
   - Performs the mutation
4. Register everything in a plugin (or add to `OperationsPlugin`)

`TakeSnapshotCommand` serializes all `SceneEntity` entities to RON. Undo restores the entire scene from the snapshot. You do NOT need to implement undo logic — it's automatic.

## File Locations

- **Operations**: `src/commands/operations.rs` — add new operations here
- **History/Snapshot**: `src/commands/history.rs` — `TakeSnapshotCommand`, `SnapshotHistory` (do not modify)
- **Plugin registration**: `src/commands/mod.rs` — `CommandsPlugin` bundles `HistoryPlugin` + `OperationsPlugin`
- **Input guards**: `src/utils.rs` — `should_process_input()` prevents input when UI has focus

## Step-by-Step

### 1. Define the Event

In `src/commands/operations.rs`, add a `Message` struct:

```rust
/// Event to [describe what the operation does]
#[derive(Message)]
pub struct MyOperationEvent {
    // Add fields if the operation needs parameters
    // pub direction: Vec3,
}
```

If the event has no fields, use a unit-like struct:

```rust
#[derive(Message)]
pub struct MyOperationEvent;
```

### 2. Write the Input Handler

Add a system that listens for keypresses and sends the event:

```rust
fn handle_my_operation_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    editor_mode: Res<State<EditorMode>>,
    mut events: MessageWriter<MyOperationEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Guard: only allow in specific modes
    if *editor_mode.get() != EditorMode::Edit {
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyF) {
        events.write(MyOperationEvent);
    }
}
```

**Key patterns:**
- Always call `should_process_input()` first
- Guard on `EditorMode` if the operation is mode-specific
- Use `just_pressed` for single-fire, `pressed` for continuous
- For Ctrl/Shift combos: `keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight)`

### 3. Write the Operation Handler

Add a system that reads the event, takes a snapshot, then mutates:

```rust
fn handle_my_operation(
    mut events: MessageReader<MyOperationEvent>,
    mut selected_query: Query<&mut Transform, (With<Selected>, With<SceneEntity>)>,
    mut commands: Commands,
) {
    for event in events.read() {
        let count = selected_query.iter().count();
        if count == 0 {
            continue;
        }

        // ALWAYS take snapshot BEFORE mutation
        commands.queue(TakeSnapshotCommand {
            description: format!("My operation on {} entities", count),
        });

        // Perform the mutation
        for mut transform in selected_query.iter_mut() {
            // ... modify transform, components, etc.
        }

        info!("Applied my operation to {} entities", count);
    }
}
```

**Critical rules:**
- Queue `TakeSnapshotCommand` BEFORE any mutations
- Use `commands.queue()` not `commands.add()`
- The snapshot description should be human-readable (shown in UI)
- Use `info!()` for user-visible logging

### 4. Register in the Plugin

In the `OperationsPlugin::build()` method in `src/commands/operations.rs`:

```rust
impl Plugin for OperationsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CopiedEntities>()
            .add_message::<DeleteSelectedEvent>()
            // ... existing messages ...
            .add_message::<MyOperationEvent>()  // <-- ADD THIS
            .add_systems(
                Update,
                (
                    // ... existing systems ...
                    handle_my_operation_input,   // <-- ADD THESE
                    handle_my_operation,
                ),
            );
    }
}
```

### 5. (Optional) Add to Command Palette

If the operation should be available from the command palette (`:`), add it in `src/ui/command_palette.rs`:

1. Add a variant to `CommandAction`:
```rust
enum CommandAction {
    // ... existing variants ...
    MyOperation,
}
```

2. Add the command entry in the commands list:
```rust
Command {
    name: "My Operation",
    shortcut: "F",
    action: CommandAction::MyOperation,
    modes: &[EditorMode::Edit],
    category: "Edit",
}
```

3. Handle the action in the execution match:
```rust
CommandAction::MyOperation => {
    events.my_operation.write(MyOperationEvent);
}
```

## Common Operation Patterns

### Operating on Selected Entities

```rust
// Read-only access to selected transforms
selected: Query<&Transform, (With<Selected>, With<SceneEntity>)>

// Mutable access to selected transforms
mut selected: Query<&mut Transform, (With<Selected>, With<SceneEntity>)>

// Access to specific marker components
selected: Query<(Entity, &Transform, Option<&PrimitiveMarker>), (With<Selected>, With<SceneEntity>)>
```

### Spawning Entities

```rust
mut spawn_events: MessageWriter<SpawnEntityEvent>,

spawn_events.write(SpawnEntityEvent {
    kind: SpawnEntityKind::Primitive(PrimitiveShape::Cube),
    position: Vec3::ZERO,
    rotation: Quat::IDENTITY,
});
```

### Despawning Entities

```rust
for entity in selected.iter() {
    commands.entity(entity).despawn();
}
```

### Adding/Removing Components

```rust
commands.entity(entity).insert(MyComponent { ... });
commands.entity(entity).remove::<MyComponent>();
```

### Deselecting Before Reselecting

```rust
// Deselect all
for entity in selected.iter() {
    commands.entity(entity).remove::<Selected>();
}
// Select specific
commands.entity(target).insert(Selected);
```

## Required Imports

```rust
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use super::TakeSnapshotCommand;
use crate::editor::{EditorMode, EditorState};
use crate::scene::{SceneEntity, SpawnEntityEvent, SpawnEntityKind};
use crate::selection::Selected;
use crate::utils::should_process_input;
```

## Checklist

- [ ] Event struct defined with `#[derive(Message)]`
- [ ] Input handler calls `should_process_input()` guard
- [ ] Input handler guards on correct `EditorMode`
- [ ] Operation handler queues `TakeSnapshotCommand` BEFORE mutation
- [ ] Snapshot description is human-readable
- [ ] Event registered with `.add_message::<MyEvent>()`
- [ ] Both systems registered in `.add_systems(Update, (...))`
- [ ] (Optional) Command palette entry added
