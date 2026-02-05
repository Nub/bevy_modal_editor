# Scene Component Registration

Use this skill when the user wants to make a component serializable so it persists in scene files and undo/redo snapshots. Examples: "make this component save with the scene", "register this for serialization", "add this to the scene snapshot".

This is different from adding a full entity type (use `new-entity-type` for that). This skill covers just making an existing or new component participate in the save/load and undo/redo systems.

## How It Works

The editor uses a snapshot-based system. `build_editor_scene()` in `src/scene/mod.rs` is the **single source of truth** for what gets serialized — both for scene file saves AND undo/redo snapshots. If a component isn't in the allow-list, it silently disappears on save/load/undo.

Runtime components (Mesh3d, MeshMaterial3d, Collider, PointLight) are NOT serialized. Instead, serializable marker components store configuration, and `regenerate_runtime_components()` recreates the runtime components after restore.

## Files to Modify

| # | File | What to add |
|---|------|-------------|
| 1 | Component definition file | Component struct with correct derives |
| 2 | `src/scene/mod.rs` | `.register_type::<MyComponent>()` in `ScenePlugin::build()` |
| 3 | `src/scene/mod.rs` | `.allow_component::<MyComponent>()` in `build_editor_scene()` |
| 4 | `src/scene/mod.rs` | Regeneration logic in `regenerate_runtime_components()` (if needed) |

## Step-by-Step

### Step 1: Define the Component

The component needs specific derives to work with Bevy's scene serialization:

```rust
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// [Description of what this component represents]
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct MyComponent {
    pub some_field: f32,
    pub another_field: bool,
}

impl Default for MyComponent {
    fn default() -> Self {
        Self {
            some_field: 1.0,
            another_field: true,
        }
    }
}
```

**Required derives:** `Component`, `Serialize`, `Deserialize`, `Clone`, `Reflect`
**Required attribute:** `#[reflect(Component)]`

For unit markers with no data:
```rust
#[derive(Component, Serialize, Deserialize, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct MyMarker;
```

**Enum fields** also need `Serialize, Deserialize, Clone, Reflect`:
```rust
#[derive(Serialize, Deserialize, Clone, Default, Reflect)]
pub enum MyEnum {
    #[default]
    VariantA,
    VariantB(f32),
}
```

### Step 2: Register Type for Reflection

In `src/scene/mod.rs`, inside `ScenePlugin::build()` (around line 511+):

```rust
// Register types for scene serialization
// ... existing registrations ...
.register_type::<MyComponent>()
```

If the component contains custom enums or nested structs, register those too:
```rust
.register_type::<MyComponent>()
.register_type::<MyEnum>()  // if MyComponent contains MyEnum
```

### Step 3: Add to Scene Allow-List

In `src/scene/mod.rs`, inside `build_editor_scene()` (around line 49+):

```rust
pub fn build_editor_scene(world: &World, entities: impl Iterator<Item = Entity>) -> DynamicScene {
    let builder = DynamicSceneBuilder::from_world(world)
        .deny_all()
        // ... existing allows ...
        .allow_component::<MyComponent>()   // <-- ADD
```

**This is the critical step.** Without this, the component will:
- Not be saved to scene files
- Not be captured in undo/redo snapshots
- Silently disappear after undo/redo or scene load

### Step 4: Add Regeneration Logic (If Needed)

Only needed if your serializable component implies runtime components that must be recreated. For example, `PrimitiveMarker` stores the shape type, and regeneration creates the `Mesh3d` + `Collider`.

In `src/scene/mod.rs`, inside `regenerate_runtime_components()` (around line 102+):

```rust
// Handle my components
let mut to_update: Vec<(Entity, MyComponent)> = Vec::new();
{
    // Query entities that have the marker but are MISSING the runtime component
    let mut query = world.query_filtered::<(Entity, &MyComponent), Without<MyRuntimeComponent>>();
    for (entity, comp) in query.iter(world) {
        to_update.push((entity, comp.clone()));
    }
}

for (entity, comp) in to_update {
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.insert((
            // Recreate runtime components from serialized data
            MyRuntimeComponent::from(&comp),
            Visibility::default(),
        ));
    }
}
```

**Pattern:** Query `With<Marker>, Without<RuntimeComponent>` to find entities needing regeneration. This naturally triggers after scene load or undo restore, since runtime components aren't serialized.

**Skip this step** if:
- The component is purely data (no runtime components to recreate)
- The component is a simple marker (like `GroupMarker`)
- A `Changed<T>` system already handles recreation (like blockout shapes)

### Step 5: Update Imports

Add the import in `src/scene/mod.rs` if the component is defined elsewhere:

```rust
use crate::my_module::MyComponent;
```

If defined in a sub-module of `src/scene/`, the `pub use` in `src/scene/mod.rs` handles it.

## For Game-Registered Components

Game crates can register components without modifying editor code using the `SceneComponentRegistry`:

```rust
use bevy_editor_game::RegisterSceneComponentExt;

impl Plugin for MyGamePlugin {
    fn build(&self, app: &mut App) {
        app.register_scene_component::<MyGameComponent>();
    }
}
```

This automatically adds the component to `build_editor_scene()` via the registry check at line ~93.

## Checklist

- [ ] Component has derives: `Component, Serialize, Deserialize, Clone, Reflect`
- [ ] Component has attribute: `#[reflect(Component)]`
- [ ] Any nested enums/structs also derive `Serialize, Deserialize, Clone, Reflect`
- [ ] `.register_type::<MyComponent>()` added in `ScenePlugin::build()`
- [ ] `.allow_component::<MyComponent>()` added in `build_editor_scene()`
- [ ] Regeneration logic added (if component implies runtime components)
- [ ] Imports updated in all modified files
