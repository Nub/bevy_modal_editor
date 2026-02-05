# New Entity Type Scaffolding

Use this skill when the user wants to add a new spawnable entity type to the editor. Examples: "add a water volume", "add a trigger zone", "add a particle emitter", "add a new blockout shape".

This is the most file-heavy pattern in the codebase. A new entity type touches **9+ files** and missing any one integration point will cause broken behavior (entities that can't be duplicated, copied, saved, undo'd, etc).

## Files to Modify (Ordered)

| # | File | What to add |
|---|------|-------------|
| 1 | `src/scene/primitives.rs` | Marker component, `SpawnEntityKind` variant, spawn function, handler match arm |
| 2 | `src/scene/mod.rs` | `.register_type::<MyMarker>()` |
| 3 | `src/commands/operations.rs` | Duplicate query + match arm, Copy query + match arm |
| 4 | `src/scene/mod.rs` | `.allow_component::<MyMarker>()` in `build_editor_scene`, regeneration in `regenerate_runtime_components` |
| 6 | `src/editor/state.rs` | `InsertObjectType` variant |
| 7 | `src/editor/insert.rs` | Preview spawning, click-to-place handler |
| 8 | `src/ui/command_palette.rs` | `CommandAction` variant, command entry, insert mode handler, normal mode handler |
| 9 | `src/ui/inspector.rs` | Data struct, data fetching, UI drawing, change application, component filter |

For **blockout/parametric shapes**, also modify:
| 10 | `src/scene/blockout/mod.rs` | Marker definition, spawn function, mesh generation, `Changed<T>` system, type registration |

## Step-by-Step

### Step 1: Define Marker Component

In `src/scene/primitives.rs` (or `src/scene/blockout/mod.rs` for blockout shapes), add the marker:

```rust
/// Marker component for [entity type] entities
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct MyEntityMarker {
    // Serializable fields that define this entity's configuration
    pub some_setting: f32,
}

impl Default for MyEntityMarker {
    fn default() -> Self {
        Self {
            some_setting: 1.0,
        }
    }
}
```

**Required derives:** `Component`, `Serialize`, `Deserialize`, `Clone`, `Reflect`
**Required attribute:** `#[reflect(Component)]`

For simple markers with no data, use a unit struct:
```rust
#[derive(Component, Serialize, Deserialize, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct MyEntityMarker;
```

### Step 2: Add SpawnEntityKind Variant

In `src/scene/primitives.rs`:

```rust
pub enum SpawnEntityKind {
    // ... existing variants ...
    /// [Description]
    MyEntity,
}

impl SpawnEntityKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            // ... existing arms ...
            SpawnEntityKind::MyEntity => "My Entity",
        }
    }
}
```

### Step 3: Write Spawn Function

In `src/scene/primitives.rs`:

```rust
/// Spawn a [entity type] entity
pub fn spawn_my_entity(
    commands: &mut Commands,
    position: Vec3,
    rotation: Quat,
    name: &str,
) -> Entity {
    let marker = MyEntityMarker::default();
    commands
        .spawn((
            SceneEntity,                    // REQUIRED: marks as editable
            Name::new(name.to_string()),    // REQUIRED: display name
            marker.clone(),                 // REQUIRED: type marker (serialized)
            // Runtime components (NOT serialized — recreated by regeneration):
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
            Collider::cuboid(0.5, 0.5, 0.5), // REQUIRED: for raycast selection
        ))
        .id()
}
```

If your entity needs mesh assets, add `meshes: &mut ResMut<Assets<Mesh>>` and `materials` params (see `spawn_primitive` for the pattern).

**Key rules:**
- Always include `SceneEntity` — without it the entity won't be saved/undo'd
- Always include a `Collider` — without it the entity can't be selected via raycast
- The marker component is what gets serialized; runtime components are regenerated

### Step 4: Add Handler Match Arm

In `src/scene/primitives.rs`, add to `handle_spawn_entity`:

```rust
let new_entity = match &event.kind {
    // ... existing arms ...
    SpawnEntityKind::MyEntity => spawn_my_entity(
        &mut commands, event.position, event.rotation, &name,
    ),
};
```

If your spawn function needs `meshes`/`materials`, add those to the system params too.

### Step 5: Register Type for Reflection

In `src/scene/mod.rs`, inside `ScenePlugin::build()`:

```rust
.register_type::<MyEntityMarker>()
```

This is required for serialization/deserialization to work.

### Step 6: Add to Duplicate Logic

In `src/commands/operations.rs`, update `handle_duplicate_selected`:

**A) Add to query tuple:**
```rust
fn handle_duplicate_selected(
    selected_query: Query<
        (
            Entity,
            &Transform,
            Option<&PrimitiveMarker>,
            Option<&GroupMarker>,
            Option<&SceneLightMarker>,
            Option<&DirectionalLightMarker>,
            Option<&SplineMarker>,
            Option<&FogVolumeMarker>,
            Option<&StairsMarker>,
            Option<&RampMarker>,
            Option<&ArchMarker>,
            Option<&LShapeMarker>,
            Option<&MyEntityMarker>,      // <-- ADD
        ),
        With<Selected>,
    >,
```

**B) Add to destructure:**
```rust
for (
    _entity, transform, primitive, group, point_light, dir_light,
    spline, fog, stairs, ramp, arch, lshape,
    my_entity,                             // <-- ADD
) in selected
```

**C) Add match arm (BEFORE the `None` fallback):**
```rust
} else if my_entity.is_some() {
    Some(SpawnEntityKind::MyEntity)
}
```

### Step 7: Add to Copy Logic

In `src/commands/operations.rs`, update `handle_copy_selected` with the **exact same pattern** as Step 6 (query tuple, destructure, match arm). The copy function mirrors duplicate.

### Step 8: Add to Serialization Allow-List and Regeneration

In `src/scene/mod.rs`, add to the `build_editor_scene` allow-list:

```rust
pub fn build_editor_scene(world: &World, entities: impl Iterator<Item = Entity>) -> DynamicScene {
    DynamicSceneBuilder::from_world(world)
        .deny_all()
        // ... existing allows ...
        .allow_component::<MyEntityMarker>()   // <-- ADD
        .extract_entities(entities)
        .build()
}
```

This is the **single source of truth** for both undo/redo snapshots (`history.rs`) and scene file saves (`serialization.rs`).

**If your entity has runtime components** that need to be recreated from the marker, add regeneration logic in `regenerate_runtime_components` (also in `src/scene/mod.rs`):

```rust
pub fn regenerate_runtime_components(world: &mut World) {
    // ... existing regeneration ...

    // Handle my entities
    let mut my_entities_to_update: Vec<(Entity, MyEntityMarker)> = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &MyEntityMarker), Without<MyRuntimeComponent>>();
        for (entity, marker) in query.iter(world) {
            my_entities_to_update.push((entity, marker.clone()));
        }
    }

    for (entity, marker) in my_entities_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                // Recreate runtime components from marker data
                Visibility::default(),
                Collider::cuboid(0.5, 0.5, 0.5),
            ));
        }
    }
}
```

**Note:** Blockout shapes skip this because they use `Changed<T>` systems that auto-regenerate meshes.

### Step 9: Add InsertObjectType Variant

In `src/editor/state.rs`:

```rust
pub enum InsertObjectType {
    // ... existing variants ...
    /// [Description]
    MyEntity,
}
```

### Step 10: Add Insert Mode Preview

In `src/editor/insert.rs`, add preview entity spawning in `spawn_preview_entity`:

```rust
InsertObjectType::MyEntity => {
    commands
        .spawn((
            InsertPreview,
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: preview_colors::GENERIC,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
            Transform::from_translation(Vec3::ZERO),
        ))
        .id()
}
```

Add click-to-place conversion (in the insert confirmation handler):

```rust
InsertObjectType::MyEntity => {
    spawn_events.write(SpawnEntityEvent {
        kind: SpawnEntityKind::MyEntity,
        position: final_position,
        rotation: final_rotation,
    });
}
```

### Step 11: Add Command Palette Integration

In `src/ui/command_palette.rs`:

**A) Add CommandAction variant:**
```rust
pub enum CommandAction {
    // ... existing variants ...
    /// Spawn a [entity type]
    SpawnMyEntity,
}
```

**B) Add command entry** (in the commands list):
```rust
Command {
    name: "My Entity",
    shortcut: "",
    action: CommandAction::SpawnMyEntity,
    modes: &[EditorMode::View, EditorMode::Edit],
    category: "Insert",
    enabled: true,
},
```

**C) Add insert mode handler** (in the `if in_insert_mode` block):
```rust
CommandAction::SpawnMyEntity => {
    events.start_insert.write(StartInsertEvent {
        object_type: InsertObjectType::MyEntity,
    });
}
```

**D) Add normal mode handler** (in the `else` block):
```rust
CommandAction::SpawnMyEntity => {
    events.spawn_entity.write(SpawnEntityEvent {
        kind: SpawnEntityKind::MyEntity,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    });
}
```

### Step 12: Add Inspector Support

In `src/ui/inspector.rs`:

**A) Define data struct** (copies of marker fields for egui editing):
```rust
#[derive(Clone)]
struct MyEntityData {
    pub some_setting: f32,
}

impl From<&MyEntityMarker> for MyEntityData {
    fn from(marker: &MyEntityMarker) -> Self {
        Self {
            some_setting: marker.some_setting,
        }
    }
}
```

**B) Fetch data** (in the data-gathering section):
```rust
let mut my_entity_data = single_entity.and_then(|e| {
    world.get::<MyEntityMarker>(e).map(|m| MyEntityData::from(m))
});
let mut my_entity_changed = false;
```

**C) Draw UI** (in the drawing section):
```rust
if let Some(ref mut data) = my_entity_data {
    ui.separator();
    egui::CollapsingHeader::new(
        egui::RichText::new("My Entity").strong().color(colors::TEXT_PRIMARY),
    )
    .default_open(true)
    .show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Setting").color(colors::TEXT_SECONDARY));
            if ui.add(egui::DragValue::new(&mut data.some_setting).speed(0.1)).changed() {
                my_entity_changed = true;
            }
        });
    });
}
```

**D) Apply changes** (in the change-application section):
```rust
if my_entity_changed {
    if let (Some(entity), Some(data)) = (single_entity, my_entity_data) {
        if let Some(mut marker) = world.get_mut::<MyEntityMarker>(entity) {
            marker.some_setting = data.some_setting;
        }
    }
}
```

**E) Add to component filter** (prevents duplicate display via reflection):
```rust
if name == "Transform"
    || name == "SceneLightMarker"
    // ... existing filters ...
    || name == "MyEntityMarker"        // <-- ADD
{
    continue;
}
```

### Step 13: Update Imports

Update `use` statements in every modified file. Key imports to add:

```rust
// In operations.rs:
use crate::scene::MyEntityMarker;

// In command_palette.rs:
// (usually already imports SpawnEntityKind and InsertObjectType)

// In inspector.rs:
use crate::scene::MyEntityMarker;
```

## Entity Type Categories

### Simple entity (like FogVolume)
- Marker with data fields
- Runtime component created at spawn time
- Regeneration needed after undo/load (query marker `Without<RuntimeComponent>`)

### Unit marker entity (like Group, Spline)
- Marker with no data (`struct MyMarker;`)
- No regeneration needed (or minimal — just collider)

### Parametric/blockout entity (like Stairs, Ramp)
- Marker with dimension/parameter fields
- Mesh generated from parameters
- Uses `Changed<T>` system for automatic mesh regeneration on parameter change
- Define in `src/scene/blockout/` instead of `primitives.rs`
- Register type in `BlockoutPlugin` instead of `ScenePlugin`

## Checklist

- [ ] Marker component defined with correct derives
- [ ] `SpawnEntityKind` variant added
- [ ] `display_name()` match arm added
- [ ] Spawn function implemented
- [ ] `handle_spawn_entity` match arm added
- [ ] `.register_type::<MyMarker>()` in ScenePlugin or BlockoutPlugin
- [ ] Duplicate query + destructure + match arm updated (operations.rs)
- [ ] Copy query + destructure + match arm updated (operations.rs)
- [ ] `.allow_component::<MyMarker>()` in `build_editor_scene` (scene/mod.rs)
- [ ] Regeneration logic in `regenerate_runtime_components` (scene/mod.rs) if needed
- [ ] `InsertObjectType` variant added (state.rs)
- [ ] Preview spawning in `spawn_preview_entity` (insert.rs)
- [ ] Click-to-place handler in insert mode (insert.rs)
- [ ] `CommandAction` variant added (command_palette.rs)
- [ ] Command entry in commands list (command_palette.rs)
- [ ] Insert mode handler match arm (command_palette.rs)
- [ ] Normal mode handler match arm (command_palette.rs)
- [ ] Inspector data struct + From impl (inspector.rs)
- [ ] Inspector data fetching (inspector.rs)
- [ ] Inspector UI drawing (inspector.rs)
- [ ] Inspector change application (inspector.rs)
- [ ] Inspector component filter updated (inspector.rs)
- [ ] All imports updated
