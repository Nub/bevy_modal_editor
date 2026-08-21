//! Selection (M2, B6): `Selected` marker on scene entities, driven by picking events
//! and actions. Selection is editor state — never serialized, never in history.

use bevy::picking::events::{Pointer, Press};
use bevy::prelude::*;
use editor_api::prelude::*;

use crate::resolver::EditorState;

/// Editor-only marker: this scene entity is selected.
#[derive(Component)]
pub struct Selected;

/// When `Some`, only these entities are selectable (an open prefab instance
/// scopes editing to its members — clicks outside are inert, never
/// clear-selects). Maintained by whoever owns the scope (editor_prefabs).
#[derive(Resource, Default)]
pub struct SelectionScope(pub Option<std::collections::HashSet<Entity>>);

/// Marks a subtree that selects AS A UNIT: a click on any descendant picks
/// this entity instead. Prefab instances wear it until opened, so authoring
/// inside one is a deliberate step in, never an accident of aim.
#[derive(Component)]
pub struct SelectionSealed;

/// An entity inside a sealed container that is nonetheless meant to be CLICKED.
///
/// A seal exists so a prefab selects as a unit and you cannot author on a member
/// of something you have not stepped into. A socket is the exception that proves
/// it: a socket is not part of the shape, it is the authoring HANDLE on the
/// shape. Without this, every verb built on "select a socket" — pivot on the
/// joint, spawn the next piece there, snap a socket to a face — was unreachable
/// with the mouse on the very pieces that could use them, because the click
/// resolved to the instance root.
///
/// Owned by whoever declares the handle (`editor_prefabs` marks sockets), so the
/// kernel stays ignorant of what a socket is.
#[derive(Component, Clone, Copy, Debug)]
pub struct SelectionHandle;

/// Broadcast whenever the selection set changes (gizmos, panels, statusbar react).
#[derive(Message, Debug)]
pub struct SelectionChanged;

/// Replace or extend the selection with `entity` (must carry `SceneId`).
pub fn select_entity(world: &mut World, entity: Entity, extend: bool) {
    if world.get::<SceneId>(entity).is_none() {
        return;
    }
    if extend {
        // Toggle membership.
        if world.get::<Selected>(entity).is_some() {
            world.entity_mut(entity).remove::<Selected>();
        } else {
            world.entity_mut(entity).insert(Selected);
        }
    } else {
        clear_selection_world(world);
        world.entity_mut(entity).insert(Selected);
    }
    world.write_message(SelectionChanged);
}

fn clear_selection_world(world: &mut World) {
    let selected: Vec<Entity> = world
        .query_filtered::<Entity, With<Selected>>()
        .iter(world)
        .collect();
    for entity in selected {
        world.entity_mut(entity).remove::<Selected>();
    }
}

/// Pixels of travel before a press stops being a click and becomes a box.
const MARQUEE_THRESHOLD: f32 = 5.0;

/// A box-select in flight. A press on empty space arms it; the release decides
/// what it was — under the threshold it was a click (which clears), over it a
/// marquee (which selects what it covers). Deferring that decision to the
/// release is why an empty press no longer clears immediately: you cannot know
/// at press time whether the user is about to drag.
#[derive(Resource, Default)]
pub struct Marquee {
    /// Where the press landed, in screen space. `None` when nothing is armed.
    pub start: Option<Vec2>,
    pub current: Vec2,
    /// Shift was held: add to the selection rather than replace it.
    pub additive: bool,
    /// What the press landed on, if anything. A press ARMS; the release decides
    /// whether it was a click on this entity or a box that happened to start
    /// over it. Selecting on press instead would make a box impossible to start
    /// anywhere the ground plane covers, which at blockout scale is everywhere.
    pub pressed: Option<Entity>,
}

impl Marquee {
    /// The covered rectangle, normalized, once the drag is past the threshold.
    /// `None` while it is still a click.
    pub fn rect(&self) -> Option<Rect> {
        let start = self.start?;
        let span = (self.current - start).abs();
        if span.x < MARQUEE_THRESHOLD && span.y < MARQUEE_THRESHOLD {
            return None;
        }
        Some(Rect::from_corners(start, self.current))
    }
}

/// Follow the pointer while a box is being dragged.
pub(crate) fn track_marquee(
    mut marquee: ResMut<Marquee>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    locations: Query<&bevy::picking::pointer::PointerLocation>,
) {
    if marquee.start.is_none() {
        return;
    }
    // Same two sources as the ground cursor: the window's field for a real
    // mouse, the pointer location for synthetic input (probes, remote control).
    let position = window
        .single()
        .ok()
        .and_then(|window| window.cursor_position())
        .or_else(|| {
            locations
                .iter()
                .find_map(|l| l.location.as_ref().map(|loc| loc.position))
        });
    if let Some(position) = position {
        marquee.current = position;
    }
}

/// Global picking observer: press on a scene entity (or a descendant of one) selects
/// it; shift extends. Skips while a gesture owns the pointer or the editor is off.
pub(crate) fn on_pointer_press(
    press: On<Pointer<Press>>,
    flying: Res<crate::camera::FlyingCamera>,
    // Alt+LMB is ORBIT — never selection.
    scope: Res<SelectionScope>,
    ids: Query<(), With<SceneId>>,
    sealed: Query<(), With<SelectionSealed>>,
    handles: Query<(), With<SelectionHandle>>,
    ui_nodes: Query<(), With<bevy::ui::ComputedNode>>,
    parents: Query<&ChildOf>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    state: Res<EditorState>,
    mode: Res<crate::modes::CurrentMode>,
    gesture: Res<crate::gesture::MoveGesture>,
    capture: Res<crate::resolver::KeyCapture>,
    mut commands: Commands,
) {
    // Propagated events re-trigger this observer for every ancestor (and finally the
    // window): handle ONLY the original hit, or the bubbled window-target invocation
    // takes the empty-click path and clears the selection made a moment earlier.
    if press.entity != press.original_event_target() {
        return;
    }
    // Clicks on ANY chrome (statusbar, panels, popups) belong to the UI — they must
    // never reach the empty-click-clears path (flow-audit: coincident pick targets).
    if ui_nodes.get(press.entity).is_ok() {
        return;
    }
    // Flow-audit gates: no selection while the game owns input, while inserting,
    // mid-gesture, or when the click is dismissing a capturing surface (palette).
    if !state.active
        || capture.0
        || flying.0
        || press.button != bevy::picking::pointer::PointerButton::Primary
        || mode.0 == crate::insert::MODE_INSERT
        || !matches!(*gesture, crate::gesture::MoveGesture::Idle)
    {
        return;
    }
    // Walk up to the nearest SceneId ancestor (mesh child hits resolve to the entity).
    let mut current = press.entity;
    let target = loop {
        if ids.get(current).is_ok() {
            break Some(current);
        }
        match parents.get(current) {
            Ok(parent) => current = parent.parent(),
            Err(_) => break None,
        }
    };
    // A SEALED container selects as a unit: clicking any part of it picks the
    // container, not the part. Prefab instances seal themselves until opened,
    // so you cannot accidentally author on a member of a prefab you have not
    // stepped into. The OUTERMOST seal wins, which handles nesting.
    let target = target.map(|entity| {
        click_target(
            entity,
            |e| handles.contains(e),
            |e| sealed.contains(e),
            |e| parents.get(e).ok().map(|parent| parent.parent()),
        )
    });
    // Alt+click is ORBIT (camera), never selection.
    if keys
        .as_ref()
        .map(|k| k.pressed(KeyCode::AltLeft) || k.pressed(KeyCode::AltRight))
        .unwrap_or(false)
    {
        return;
    }
    let extend = keys
        .map(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight))
        .unwrap_or(false);
    // Scoped editing (open prefab): presses outside the scope are inert.
    if let Some(target) = target
        && let Some(scope) = &scope.0
        && !scope.contains(&target)
    {
        return;
    }
    // ARM. Nothing is selected yet — the release decides whether this was a
    // click on `target` or a box that started over it.
    let at = press.pointer_location.position;
    commands.queue(move |world: &mut World| {
        let mut marquee = world.resource_mut::<Marquee>();
        marquee.start = Some(at);
        marquee.current = at;
        marquee.additive = extend;
        marquee.pressed = target;
    });
}

/// Release ends a box-select. Under the threshold it was a click on empty space,
/// which clears; over it, everything the box covered becomes the selection.
pub(crate) fn on_pointer_release(
    release: On<Pointer<Release>>,
    mut marquee: ResMut<Marquee>,
    mut commands: Commands,
) {
    if release.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let Some(_) = marquee.start else { return };
    let rect = marquee.rect();
    let additive = marquee.additive;
    let pressed = marquee.pressed;
    marquee.start = None;
    marquee.pressed = None;
    match (rect, pressed) {
        // A drag is a box, wherever it started.
        (Some(rect), _) => commands.queue(move |world: &mut World| {
            select_within(world, rect, additive);
        }),
        // A click on something selects it.
        (None, Some(target)) => {
            commands.queue(move |world: &mut World| select_entity(world, target, additive));
        }
        // A click on nothing clears, unless extending.
        (None, None) if !additive => commands.queue(|world: &mut World| {
            let had_selection = world
                .query_filtered::<(), With<Selected>>()
                .iter(world)
                .count()
                > 0;
            if had_selection {
                clear_selection_world(world);
                world.write_message(SelectionChanged);
            }
        }),
        (None, None) => {}
    }
}

/// Select every scene entity whose origin projects inside `rect`.
///
/// Origins rather than bounds: a bounds test needs every entity's world AABB,
/// which derived gltf subtrees do not all have, and "the thing is in the box"
/// reads the same to a designer either way at blockout scale.
pub(crate) fn select_within(world: &mut World, rect: Rect, additive: bool) {
    let Some((camera, camera_transform)) = world
        .query::<(
            &Camera,
            &GlobalTransform,
            Option<&bevy::camera::RenderTarget>,
        )>()
        .iter(world)
        .find(|(camera, _, target)| crate::camera::is_viewport_camera(camera, *target))
        .map(|(camera, transform, _)| (camera.clone(), *transform))
    else {
        return;
    };
    select_projected(world, rect, additive, |at| {
        camera.world_to_viewport(&camera_transform, at).ok()
    });
}

/// The decision itself, with the projection handed in: which scene entities the
/// box covers, resolved through seals and scope, and what that does to the
/// selection. Split out because the camera half needs a real render target —
/// which a headless test has no way to provide — and the DECISION half is the
/// part worth testing.
pub(crate) fn select_projected(
    world: &mut World,
    rect: Rect,
    additive: bool,
    project: impl Fn(Vec3) -> Option<Vec2>,
) {
    // Sealed containers select as a unit here too: a box over half a prefab
    // picks the prefab, exactly as a click on one of its parts does.
    let candidates: Vec<(Entity, Vec3)> = world
        .query_filtered::<(Entity, &GlobalTransform), With<SceneId>>()
        .iter(world)
        .map(|(entity, transform)| (entity, transform.translation()))
        .collect();
    let scope = world.resource::<SelectionScope>().0.clone();
    // Cloned, not borrowed: the loop below needs &World for the seal walk.
    let hidden = world.resource::<crate::hide::Hidden>().clone();
    let mut covered: Vec<Entity> = Vec::new();
    for (entity, at) in candidates {
        let Some(screen) = project(at) else {
            continue; // behind the camera or off-screen
        };
        if !rect.contains(screen) {
            continue;
        }
        let resolved = outermost_seal(world, entity);
        // A hidden object is out of the conversation. Without this,
        // `space h` then a box-drag then `d` deletes geometry nobody can see.
        // The test is on the RESOLVED root: a stamped member is not itself in
        // the hidden set, it hangs under something that is.
        if crate::hide::is_hidden_world(world, resolved, &hidden) {
            continue;
        }
        if let Some(scope) = &scope
            && !scope.contains(&resolved)
        {
            continue;
        }
        if !covered.contains(&resolved) {
            covered.push(resolved);
        }
    }
    if covered.is_empty() && !additive {
        let had_selection = world
            .query_filtered::<(), With<Selected>>()
            .iter(world)
            .count()
            > 0;
        if had_selection {
            clear_selection_world(world);
            world.write_message(SelectionChanged);
        }
        return;
    }
    if covered.is_empty() {
        return;
    }
    if !additive {
        clear_selection_world(world);
    }
    for entity in covered {
        world.entity_mut(entity).insert(Selected);
    }
    world.write_message(SelectionChanged);
}

/// What a click on `entity` actually selects: the entity itself if it is a
/// HANDLE, otherwise its outermost sealed ancestor, otherwise itself.
///
/// THE rule, as a function, because it has two callers — the picking observer
/// (which has queries) and the world-side helpers — and two copies of a
/// selection rule is how a click starts meaning different things in different
/// places.
pub(crate) fn click_target(
    entity: Entity,
    is_handle: impl Fn(Entity) -> bool,
    is_sealed: impl Fn(Entity) -> bool,
    parent_of: impl Fn(Entity) -> Option<Entity>,
) -> Entity {
    if is_handle(entity) {
        return entity;
    }
    let mut resolved = entity;
    let mut current = entity;
    loop {
        if is_sealed(current) {
            resolved = current;
        }
        match parent_of(current) {
            Some(parent) => current = parent,
            None => break,
        }
    }
    resolved
}

/// The outermost sealed ancestor of `entity`, or the entity itself.
pub(crate) fn outermost_seal(world: &World, entity: Entity) -> Entity {
    click_target(
        entity,
        |e| world.get::<SelectionHandle>(e).is_some(),
        |e| world.get::<SelectionSealed>(e).is_some(),
        |e| world.get::<ChildOf>(e).map(|parent| parent.parent()),
    )
}

/// How far a fold walks before deciding the hierarchy is malformed. Bailing
/// returns "not carried", i.e. the entity keeps its own op — the pre-fold
/// behaviour, which is the safe direction to fail in.
const MAX_ANCESTOR_WALK: usize = 256;

/// Is `entity` already being moved by something else this verb is moving?
///
/// A verb that writes a DELTA, or a recomputed pose, onto an existing
/// `Transform` has to ask this. Bevy already carries a child when its ancestor's
/// transform changes, so a child that ALSO writes the delta itself moves twice —
/// and `ctrl+a` then `w` is all it takes to reach that, because select-all takes
/// children too.
///
/// `is_carrier` is deliberately narrower than "is selected": it has to mean
/// "this entity's own edit will actually LAND". A locked operand's op is refused
/// at the queue, so it moves nothing and carries nothing — its selected
/// descendants must still move themselves, or "moving ten objects with two
/// locked moves the eight" (spec §9) quietly stops being true.
///
/// A predicate rather than a set, so a query-side caller and a `&World` caller
/// share one implementation without either owning a collection. It walks
/// `ChildOf` to the root unconditionally: the question is "will propagation move
/// this", and propagation follows `ChildOf` alone — an ancestor the scene cannot
/// name is simply never a carrier, and the walk passes through it.
pub fn carried_by(
    entity: Entity,
    is_carrier: impl Fn(Entity) -> bool,
    parent_of: impl Fn(Entity) -> Option<Entity>,
) -> bool {
    let mut current = entity;
    for _ in 0..MAX_ANCESTOR_WALK {
        match parent_of(current) {
            Some(parent) if is_carrier(parent) => return true,
            Some(parent) => current = parent,
            None => return false,
        }
    }
    false
}

/// Select this entity once its spawn transaction has applied (placement,
/// grouping, paste — anything that creates and should hand the user the result).
#[derive(Resource, Default)]
pub struct PendingSelect(pub Option<SceneId>);

pub(crate) fn select_pending(
    mut pending: ResMut<PendingSelect>,
    index: Res<SceneIndex>,
    previous: Query<Entity, With<Selected>>,
    mut changed: MessageWriter<SelectionChanged>,
    mut commands: Commands,
) {
    let Some(id) = pending.0 else { return };
    let Some(entity) = index.get(&id) else { return };
    pending.0 = None;
    for entity in &previous {
        commands.entity(entity).remove::<Selected>();
    }
    commands.entity(entity).insert(Selected);
    changed.write(SelectionChanged);
}

/// `core.escape-home` (and explicit `select.clear`) empties the selection;
/// `select.all` selects every scene entity.
pub(crate) fn handle_selection_actions(
    mut reader: MessageReader<ActionInvoked>,
    selected: Query<Entity, With<Selected>>,
    scene_entities: Query<Entity, With<SceneId>>,
    scene_ids: Query<&SceneId>,
    parents: Query<&ChildOf>,
    hidden: Res<crate::hide::Hidden>,
    scope: Res<SelectionScope>,
    escape_from_capture: Res<crate::resolver::EscapeFromCapture>,
    confirm: Res<crate::clipboard::DeleteConfirm>,
    mut commands: Commands,
    mut changed: MessageWriter<SelectionChanged>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "core.escape-home" if escape_from_capture.0 => {}
            // A delete question owns Escape while it is up. Answering "no" and
            // losing the selection anyway would make the safe answer nearly as
            // expensive as the destructive one.
            "core.escape-home" if confirm.pending.is_some() || confirm.just_cancelled => {}
            "core.escape-home" | "select.clear" => {
                for entity in &selected {
                    commands.entity(entity).remove::<Selected>();
                }
                changed.write(SelectionChanged);
            }
            "select.all" => {
                for entity in &scene_entities {
                    // Ctrl+A does not reach what you hid. `space h` then
                    // ctrl+a then `d` is the silent-destruction path the
                    // lock work exists to prevent. The ancestor walk covers
                    // this sweep's seal-blindness: a stamped member is not
                    // itself hidden, its instance root is.
                    if crate::hide::is_hidden(entity, &hidden, &scene_ids, &parents) {
                        continue;
                    }
                    if scope.0.as_ref().is_none_or(|s| s.contains(&entity)) {
                        commands.entity(entity).insert(Selected);
                    }
                }
                changed.write(SelectionChanged);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prefab selects as a UNIT — except for its handles. Sockets sit inside
    /// that seal, so every verb built on "select a socket" (pivot on the joint,
    /// spawn the next piece there, snap a socket to a face) was unreachable
    /// with the mouse on exactly the pieces that could use them: the click
    /// resolved to the instance root, every time.
    #[test]
    fn a_handle_is_clicked_as_itself_inside_a_seal() {
        let root = Entity::from_raw_u32(1).unwrap();
        let member = Entity::from_raw_u32(2).unwrap();
        let handle = Entity::from_raw_u32(3).unwrap();
        let parent_of = |entity: Entity| (entity != root).then_some(root);
        let sealed = |entity: Entity| entity == root;

        assert_eq!(
            click_target(member, |_| false, sealed, parent_of),
            root,
            "an ordinary member still selects the whole prefab"
        );
        assert_eq!(
            click_target(handle, |e| e == handle, sealed, parent_of),
            handle,
            "a handle selects as itself"
        );
        assert_eq!(
            click_target(root, |_| false, sealed, parent_of),
            root,
            "and the container still selects itself"
        );
    }

    /// Nesting still resolves outward for ordinary members.
    #[test]
    fn the_outermost_seal_wins() {
        let outer = Entity::from_raw_u32(10).unwrap();
        let inner = Entity::from_raw_u32(11).unwrap();
        let leaf = Entity::from_raw_u32(12).unwrap();
        let parent_of = |entity: Entity| match entity {
            e if e == leaf => Some(inner),
            e if e == inner => Some(outer),
            _ => None,
        };
        assert_eq!(
            click_target(leaf, |_| false, |e| e == inner || e == outer, parent_of),
            outer
        );
    }
    use crate::EditorCorePlugin;

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        // A viewport camera looking down -Z from +Z, so world x/y map to screen
        // predictably.
        app.world_mut().spawn((
            Camera3d::default(),
            // An explicit viewport: headless there is no window to take a size
            // from, and projection needs one.
            Camera {
                viewport: Some(bevy::camera::Viewport {
                    physical_position: UVec2::ZERO,
                    physical_size: UVec2::new(1280, 720),
                    ..default()
                }),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection::default()),
            Transform::from_xyz(0.0, 0.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        app.update();
        app
    }

    fn spawn_at(app: &mut App, at: Vec3) -> Entity {
        app.world_mut()
            .spawn((
                SceneId::random(),
                Transform::from_translation(at),
                GlobalTransform::from_translation(at),
            ))
            .id()
    }

    /// A projection with nothing to go wrong in it: world x/y ARE screen x/y.
    /// The real one needs a render target, which a headless app cannot provide;
    /// what these tests are about is which entities a box takes.
    fn flat(at: Vec3) -> Option<Vec2> {
        Some(Vec2::new(at.x, at.y))
    }
    // A box under the threshold is a CLICK, not a marquee: a five-pixel wobble
    // while clicking empty space must not turn into a selection gesture.
    #[test]
    fn a_short_drag_is_still_a_click() {
        let marquee = Marquee {
            start: Some(Vec2::new(100.0, 100.0)),
            current: Vec2::new(103.0, 102.0),
            additive: false,
            pressed: None,
        };
        assert!(marquee.rect().is_none(), "under the threshold");
        let dragged = Marquee {
            current: Vec2::new(140.0, 160.0),
            ..marquee
        };
        assert!(dragged.rect().is_some(), "past it, it is a box");
    }

    // Dragging in ANY direction gives the same box — a marquee started at the
    // bottom-right has to work exactly like one started at the top-left.
    #[test]
    fn a_box_normalizes_whichever_way_it_is_dragged() {
        let forward = Marquee {
            start: Some(Vec2::new(10.0, 10.0)),
            current: Vec2::new(90.0, 70.0),
            additive: false,
            pressed: None,
        };
        let backward = Marquee {
            start: Some(Vec2::new(90.0, 70.0)),
            current: Vec2::new(10.0, 10.0),
            additive: false,
            pressed: None,
        };
        assert_eq!(forward.rect(), backward.rect());
    }

    // The verb itself: everything the box covers becomes the selection, and
    // everything outside it does not.
    #[test]
    fn a_box_selects_what_it_covers() {
        let mut app = test_app();
        let inside_a = spawn_at(&mut app, Vec3::new(-1.0, 0.0, 0.0));
        let inside_b = spawn_at(&mut app, Vec3::new(1.0, 0.0, 0.0));
        let outside = spawn_at(&mut app, Vec3::new(9.0, 0.0, 0.0));
        app.update();

        let rect = Rect::from_corners(Vec2::new(-5.0, -5.0), Vec2::new(5.0, 5.0));
        let world = app.world_mut();
        select_projected(world, rect, false, flat);
        app.update();

        assert!(app.world().get::<Selected>(inside_a).is_some(), "covered");
        assert!(app.world().get::<Selected>(inside_b).is_some(), "covered");
        assert!(
            app.world().get::<Selected>(outside).is_none(),
            "outside the box, untouched"
        );
    }

    // Shift ADDS: laying a second box must not throw away the first.
    #[test]
    fn an_additive_box_keeps_what_was_selected() {
        let mut app = test_app();
        let first = spawn_at(&mut app, Vec3::new(-1.0, 0.0, 0.0));
        let second = spawn_at(&mut app, Vec3::new(1.0, 0.0, 0.0));
        app.update();

        let a = Vec2::new(-1.0, 0.0);
        let b = Vec2::new(1.0, 0.0);
        select_projected(
            app.world_mut(),
            Rect::from_corners(a - Vec2::splat(0.5), a + Vec2::splat(0.5)),
            false,
            flat,
        );
        app.update();
        assert!(app.world().get::<Selected>(first).is_some());

        select_projected(
            app.world_mut(),
            Rect::from_corners(b - Vec2::splat(0.5), b + Vec2::splat(0.5)),
            true,
            flat,
        );
        app.update();
        assert!(
            app.world().get::<Selected>(first).is_some(),
            "the first survived an additive box"
        );
        assert!(
            app.world().get::<Selected>(second).is_some(),
            "and the second joined"
        );
    }

    // A box over nothing clears, exactly as a click on empty space does.
    #[test]
    fn an_empty_box_clears_the_selection() {
        let mut app = test_app();
        let entity = spawn_at(&mut app, Vec3::ZERO);
        app.update();
        app.world_mut().entity_mut(entity).insert(Selected);

        let far = Rect::from_corners(Vec2::new(2000.0, 2000.0), Vec2::new(2200.0, 2200.0));
        select_projected(app.world_mut(), far, false, flat);
        app.update();
        assert!(
            app.world().get::<Selected>(entity).is_none(),
            "a box over nothing clears"
        );
    }
}
