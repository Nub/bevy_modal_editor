//! Insert mode (M2, B8/B9): pick a kind (palette or synthesized action), ghost
//! preview follows the cursor on the ground plane (grid-snapped when enabled), click
//! places via a single transaction; Shift-click places and continues (M1-proven v1
//! behavior). Kind actions are DERIVED from the registry — no hand-maintained lists.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use editor_api::prelude::*;

use crate::modes::{set_mode, CurrentMode, ModeChanged, MODE_NORMAL};
use crate::resolver::EditorState;

pub const MODE_INSERT: ModeId = ModeId::new_static("insert");

/// All registered kinds, keyed for lookup (from `ValidatedFeatures.kinds`).
#[derive(Resource, Default)]
pub struct KindCatalog {
    pub kinds: Vec<EntityKindDef>,
}

impl KindCatalog {
    pub fn get(&self, id: &EntityKindId) -> Option<&EntityKindDef> {
        self.kinds.iter().find(|k| &k.id == id)
    }
}

/// The kind currently being placed (persists across insert sessions).
#[derive(Resource, Default)]
pub struct InsertState {
    pub kind: Option<EntityKindId>,
}

/// Grid snap (B9): quantizes preview + placement when enabled.
#[derive(Resource)]
pub struct GridSnap {
    pub enabled: bool,
    pub size: f32,
}

impl Default for GridSnap {
    fn default() -> Self {
        Self { enabled: false, size: 1.0 }
    }
}

/// Cursor projected onto the ground plane (y = 0), editor-active frames only.
/// Tests inject this directly; the camera system fills it in a real app.
#[derive(Resource, Default)]
pub struct CursorGround(pub Option<Vec3>);

/// Set for one frame when a kind-pick action fires — distinguishes "entered insert
/// by picking a kind" (palette must NOT re-open over the fresh ghost) from "entered
/// insert to browse" (palette auto-opens).
#[derive(Resource, Default)]
pub struct KindJustPicked(pub bool);

#[derive(Component)]
pub(crate) struct PreviewEntity {
    /// Which kind this ghost represents — a mismatch with `InsertState.kind`
    /// rebuilds the ghost (switching Cube→Sphere must switch the preview).
    kind: EntityKindId,
    /// Kind placement offset (e.g. +Y half-height): ghost translation = target +
    /// offset, so preview and final placement are always identical.
    offset: Vec3,
}

pub(crate) fn cursor_ground(
    state: Res<EditorState>,
    camera: Query<(&Camera, &GlobalTransform)>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cursor: ResMut<CursorGround>,
) {
    if !state.active {
        cursor.0 = None;
        return;
    }
    let (Ok(window), Some((camera, camera_transform))) =
        (window.single(), camera.iter().find(|(c, _)| c.is_active))
    else {
        return;
    };
    let Some(position) = window.cursor_position() else {
        cursor.0 = None;
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, position) else {
        cursor.0 = None;
        return;
    };
    cursor.0 = ray
        .intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))
        .map(|distance| ray.get_point(distance));
}

pub fn snapped(position: Vec3, snap: &GridSnap) -> Vec3 {
    if !snap.enabled {
        return position;
    }
    (position / snap.size).round() * snap.size
}

/// Kind-pick actions (synthesized `insert.kind.<id>`) select the kind and enter
/// insert mode; `core.toggle-grid-snap` flips B9's quantization.
pub(crate) fn handle_insert_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut insert: ResMut<InsertState>,
    mut grid: ResMut<GridSnap>,
    mut mode: ResMut<CurrentMode>,
    mut just_picked: ResMut<KindJustPicked>,
    mut mode_changed: MessageWriter<ModeChanged>,
) {
    for invoked in reader.read() {
        if let Some(kind) = invoked.action.as_str().strip_prefix("insert.kind.") {
            insert.kind = Some(EntityKindId::new(kind.to_string()));
            just_picked.0 = true;
            set_mode(MODE_INSERT, &mut mode, &mut mode_changed);
        }
        if invoked.action.as_str() == "core.toggle-grid-snap" {
            grid.enabled = !grid.enabled;
        }
    }
}

/// Keep exactly one ghost preview alive while insert mode has a kind; track cursor.
pub(crate) fn sync_preview(world: &mut World) {
    let state_active = world.resource::<EditorState>().active;
    let in_insert = world.resource::<CurrentMode>().0 == MODE_INSERT;
    let kind_id = world.resource::<InsertState>().kind.clone();
    let cursor = world.resource::<CursorGround>().0;
    let grid_target = {
        let grid = world.resource::<GridSnap>();
        cursor.map(|c| snapped(c, &grid))
    };

    let existing: Vec<Entity> =
        world.query_filtered::<Entity, With<PreviewEntity>>().iter(world).collect();

    let want_preview = state_active && in_insert && kind_id.is_some() && grid_target.is_some();
    if !want_preview {
        for entity in existing {
            world.entity_mut(entity).despawn();
        }
        return;
    }
    let target = grid_target.unwrap();

    if let Some(&entity) = existing.first() {
        let preview_kind = world.get::<PreviewEntity>(entity).map(|p| p.kind.clone());
        if preview_kind.as_ref() == kind_id.as_ref() {
            let offset =
                world.get::<PreviewEntity>(entity).map(|p| p.offset).unwrap_or(Vec3::ZERO);
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.translation = target + offset;
            }
            return;
        }
        // Kind switched: rebuild the ghost below.
        for entity in existing {
            world.entity_mut(entity).despawn();
        }
    }

    // Spawn the ghost: semantic components (via reflection) + preview markers.
    let Some(kind) = world
        .resource::<KindCatalog>()
        .get(kind_id.as_ref().unwrap())
        .cloned()
    else {
        return;
    };
    let components = (kind.components)(target);
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let entity = world
        .spawn((PreviewEntity { kind: kind.id.clone(), offset: Vec3::ZERO }, InsertPreview))
        .id();
    for value in components {
        let Some(info) = value.get_represented_type_info() else { continue };
        let Some(registration) = registry.get(info.type_id()) else { continue };
        let Some(reflect_component) =
            registration.data::<bevy::ecs::reflect::ReflectComponent>()
        else {
            continue;
        };
        let Ok(mut entity_mut) = world.get_entity_mut(entity) else { continue };
        reflect_component.apply_or_insert_mapped(
            &mut entity_mut,
            value.as_ref(),
            &registry,
            &mut (),
            bevy::ecs::relationship::RelationshipHookMode::Run,
        );
    }
    // Record the kind's placement offset so cursor updates keep preview == placement.
    let offset = world
        .get::<Transform>(entity)
        .map(|t| t.translation - target)
        .unwrap_or(Vec3::ZERO);
    if let Some(mut preview) = world.get_mut::<PreviewEntity>(entity) {
        preview.offset = offset;
    }
}

/// Click places (one transaction); Shift-click places and stays in insert mode.
pub(crate) fn place_on_click(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    state: Res<EditorState>,
    mut mode: ResMut<CurrentMode>,
    insert: Res<InsertState>,
    catalog: Res<KindCatalog>,
    grid: Res<GridSnap>,
    cursor: Res<CursorGround>,
    mut edits: EditScope,
    mut mode_changed: MessageWriter<ModeChanged>,
) {
    if !state.active || mode.0 != MODE_INSERT {
        return;
    }
    let (Some(mouse), Some(target)) = (mouse, cursor.0) else { return };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(kind) = insert.kind.as_ref().and_then(|id| catalog.get(id)) else { return };

    let position = snapped(target, &grid);
    let id = SceneId::random();
    edits
        .transaction(format!("Place {}", kind.display_name))
        .spawn(id, (kind.components)(position))
        .commit();

    let stay = keys
        .map(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight))
        .unwrap_or(false);
    if !stay {
        set_mode(MODE_NORMAL, &mut mode, &mut mode_changed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edits::History;
    use crate::EditorCorePlugin;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Marker {
        tag: u32,
    }

    fn cube_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
        vec![
            Box::new(Transform::from_translation(position + Vec3::Y * 0.5))
                .into_partial_reflect(),
            Box::new(Marker { tag: 7 }).into_partial_reflect(),
        ]
    }

    fn sphere_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
        vec![
            Box::new(Transform::from_translation(position + Vec3::Y * 0.25))
                .into_partial_reflect(),
            Box::new(Marker { tag: 9 }).into_partial_reflect(),
        ]
    }

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test", "Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Transform>()
                .component::<Marker>()
                .entity_kind(EntityKindDef {
                    id: EntityKindId::new_static("test-cube"),
                    display_name: "Test Cube",
                    components: cube_components,
                })
                .entity_kind(EntityKindDef {
                    id: EntityKindId::new_static("test-sphere"),
                    display_name: "Test Sphere",
                    components: sphere_components,
                });
        }
    }

    // Regression (owner bug): switching kinds must rebuild the ghost.
    #[test]
    fn switching_kind_rebuilds_ghost() {
        let mut app = test_app();
        invoke(&mut app, "insert.kind.test-cube");
        app.world_mut().resource_mut::<CursorGround>().0 = Some(Vec3::new(1.0, 0.0, 1.0));
        app.update();
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Marker, With<InsertPreview>>();
            assert_eq!(q.single(world).unwrap().tag, 7, "cube ghost");
        }
        invoke(&mut app, "insert.kind.test-sphere");
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Marker, &Transform), With<InsertPreview>>();
        let (marker, transform) = q.single(world).expect("exactly one ghost");
        assert_eq!(marker.tag, 9, "sphere ghost after switch");
        assert_eq!(transform.translation.y, 0.25, "new kind's offset applies");
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
    }

    // B8: kind action is synthesized from the registry and drives insert mode.
    #[test]
    fn kind_action_synthesized_and_enters_insert() {
        let mut app = test_app();
        let catalog = app.world().resource::<crate::resolver::ActionCatalog>();
        assert!(
            catalog.get(&ActionId::new_static("insert.kind.test-cube")).is_some(),
            "registry-derived insert action exists"
        );
        invoke(&mut app, "insert.kind.test-cube");
        assert_eq!(app.world().resource::<CurrentMode>().0, MODE_INSERT);
        assert_eq!(
            app.world().resource::<InsertState>().kind,
            Some(EntityKindId::new_static("test-cube"))
        );
    }

    // B8 + B9: preview tracks (snapped) cursor; click places one snapped entity as
    // one undoable transaction; shift-click stays in insert mode.
    #[test]
    fn preview_place_and_grid_snap() {
        let mut app = test_app();
        invoke(&mut app, "insert.kind.test-cube");
        invoke(&mut app, "core.toggle-grid-snap");
        assert!(app.world().resource::<GridSnap>().enabled);

        app.world_mut().resource_mut::<CursorGround>().0 = Some(Vec3::new(2.3, 0.0, -1.7));
        app.update();

        // Ghost exists at the snapped position with semantic components.
        let world = app.world_mut();
        let (preview_transform, marker) = {
            let mut q = world.query_filtered::<(&Transform, Option<&Marker>), With<InsertPreview>>();
            let (t, m) = q.single(world).expect("one ghost preview");
            (*t, m.cloned())
        };
        assert_eq!(preview_transform.translation, Vec3::new(2.0, 0.5, -2.0), "snapped+offset");
        assert_eq!(marker, Some(Marker { tag: 7 }));

        // Regression (owner bug): after the cursor MOVES, the ghost must keep the
        // kind's Y offset — it was clipping into the ground while placement didn't.
        app.world_mut().resource_mut::<CursorGround>().0 = Some(Vec3::new(5.4, 0.0, 3.2));
        app.update();
        let world = app.world_mut();
        let moved = {
            let mut q = world.query_filtered::<&Transform, With<InsertPreview>>();
            *q.single(world).unwrap()
        };
        assert_eq!(moved.translation, Vec3::new(5.0, 0.5, 3.0), "preview == placement");
        app.world_mut().resource_mut::<CursorGround>().0 = Some(Vec3::new(2.3, 0.0, -1.7));
        app.update();

        // Shift-click: place and remain in insert mode.
        let depth_before = app.world().resource::<History>().undo_depth();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ShiftLeft);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(KeyCode::ShiftLeft);
        }
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.clear();
            mouse.release(MouseButton::Left);
        }
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();

        assert_eq!(app.world().resource::<CurrentMode>().0, MODE_INSERT, "shift keeps mode");
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before + 1,
            "placement is one transaction"
        );
        let world = app.world_mut();
        let placed: Vec<Vec3> = world
            .query_filtered::<&Transform, (With<SceneId>, With<Marker>)>()
            .iter(world)
            .map(|t| t.translation)
            .collect();
        assert_eq!(placed, vec![Vec3::new(2.0, 0.5, -2.0)], "snapped placement w/ offset");

        // Plain click: places then returns to normal.
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<CurrentMode>().0, crate::modes::MODE_NORMAL);
    }
}
