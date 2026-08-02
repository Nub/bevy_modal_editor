//! Selection (M2, B6): `Selected` marker on scene entities, driven by picking events
//! and actions. Selection is editor state — never serialized, never in history.

use bevy::picking::events::{Pointer, Press};
use bevy::prelude::*;
use editor_api::prelude::*;

use crate::resolver::EditorState;

/// Editor-only marker: this scene entity is selected.
#[derive(Component)]
pub struct Selected;

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
    let selected: Vec<Entity> =
        world.query_filtered::<Entity, With<Selected>>().iter(world).collect();
    for entity in selected {
        world.entity_mut(entity).remove::<Selected>();
    }
}

/// Global picking observer: press on a scene entity (or a descendant of one) selects
/// it; shift extends. Skips while a gesture owns the pointer or the editor is off.
pub(crate) fn on_pointer_press(
    press: On<Pointer<Press>>,
    flying: Res<crate::camera::FlyingCamera>,
    ids: Query<(), With<SceneId>>,
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
    let extend = keys
        .map(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight))
        .unwrap_or(false);
    match target {
        Some(target) => {
            commands.queue(move |world: &mut World| select_entity(world, target, extend));
        }
        // Click on empty space (ground, sky) clears the selection — unless extending.
        None if !extend => {
            commands.queue(|world: &mut World| {
                let had_selection =
                    world.query_filtered::<(), With<Selected>>().iter(world).count() > 0;
                if had_selection {
                    clear_selection_world(world);
                    world.write_message(SelectionChanged);
                }
            });
        }
        None => {}
    }
}

/// `core.escape-home` (and explicit `select.clear`) empties the selection;
/// `select.all` selects every scene entity.
pub(crate) fn handle_selection_actions(
    mut reader: MessageReader<ActionInvoked>,
    selected: Query<Entity, With<Selected>>,
    scene_entities: Query<Entity, With<SceneId>>,
    escape_from_capture: Res<crate::resolver::EscapeFromCapture>,
    mut commands: Commands,
    mut changed: MessageWriter<SelectionChanged>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "core.escape-home" if escape_from_capture.0 => {}
            "core.escape-home" | "select.clear" => {
                for entity in &selected {
                    commands.entity(entity).remove::<Selected>();
                }
                changed.write(SelectionChanged);
            }
            "select.all" => {
                for entity in &scene_entities {
                    commands.entity(entity).insert(Selected);
                }
                changed.write(SelectionChanged);
            }
            _ => {}
        }
    }
}

/// Viewport feedback: accent AABB wireframe around every selected entity
/// (the M2 outline; the JFA outliner port replaces this later).
pub(crate) fn draw_selection_gizmos(
    mut gizmos: Gizmos,
    state: Res<EditorState>,
    selected: Query<(&GlobalTransform, Option<&bevy::camera::primitives::Aabb>), With<Selected>>,
) {
    if !state.active {
        return;
    }
    let count = selected.iter().count();
    if count > 0 {
        info_once!("OUTLINE DRAW: running with {count} selected");
    }
    let color = Color::srgb(0.35, 0.62, 1.0);
    for (transform, aabb) in &selected {
        let (center, half) = match aabb {
            Some(aabb) => (Vec3::from(aabb.center), Vec3::from(aabb.half_extents)),
            None => (Vec3::ZERO, Vec3::splat(0.5)),
        };
        gizmos.aabb_3d(
            bevy::math::bounding::Aabb3d::new(center, half),
            *transform,
            color,
        );
    }
}
