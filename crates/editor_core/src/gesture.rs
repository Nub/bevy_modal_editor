//! The move-gesture state machine (M2, B7 — spec §4):
//! `Idle → Active { originals } → Commit | Cancel`. Every frame of the drag is a
//! gesture-tagged transaction through the `EditQueue` (macros and undo see the same
//! stream); coalescing makes the whole drag ONE history entry, Esc-cancel pops it and
//! restores originals exactly.
//!
//! Pointer input is abstracted behind `GesturePointer` (a world-space target): the
//! camera system fills it from the cursor ray in a real app; headless tests set it
//! directly. Axis constraints (`x`/`y`/`z`) and commit/cancel arrive as ACTIONS from
//! the `gesture-move` overlay context — no raw keys here.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use editor_api::prelude::*;

use crate::edits::HistoryRequests;
use crate::resolver::{EditorState, OverlayContext};
use crate::selection::Selected;

pub const GESTURE_MOVE_CONTEXT: ContextId = ContextId::new_static("gesture-move");

/// World-space point the gesture steers toward. Fed by the camera/cursor system (or a
/// test). `None` = no valid target this frame.
#[derive(Resource, Default)]
pub struct GesturePointer(pub Option<Vec3>);

#[derive(Resource, Default)]
pub enum MoveGesture {
    #[default]
    Idle,
    Active {
        id: u64,
        axis: Option<usize>,
        /// Pointer position when the gesture began (deltas are relative to this).
        anchor: Option<Vec3>,
        originals: Vec<(SceneId, Transform)>,
    },
}

#[derive(Resource, Default)]
pub(crate) struct GestureCounter(u64);

/// Consume gesture-related actions (any source — keys, palette, macros).
pub(crate) fn handle_gesture_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut gesture: ResMut<MoveGesture>,
    mut overlay: ResMut<OverlayContext>,
    mut counter: ResMut<GestureCounter>,
    mut pointer: ResMut<GesturePointer>,
    mut requests: ResMut<HistoryRequests>,
    selected: Query<(&SceneId, &Transform), With<Selected>>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "transform.move" => {
                if matches!(*gesture, MoveGesture::Idle) {
                    let originals: Vec<(SceneId, Transform)> =
                        selected.iter().map(|(id, t)| (*id, *t)).collect();
                    if originals.is_empty() {
                        continue;
                    }
                    counter.0 += 1;
                    *gesture = MoveGesture::Active {
                        id: counter.0,
                        axis: None,
                        anchor: None,
                        originals,
                    };
                    pointer.0 = None;
                    overlay.0 = Some(GESTURE_MOVE_CONTEXT);
                }
            }
            "transform.axis-x" => set_axis(&mut gesture, 0),
            "transform.axis-y" => set_axis(&mut gesture, 1),
            "transform.axis-z" => set_axis(&mut gesture, 2),
            "transform.commit" => {
                if !matches!(*gesture, MoveGesture::Idle) {
                    *gesture = MoveGesture::Idle;
                    overlay.0 = None;
                }
            }
            "transform.cancel" => {
                if let MoveGesture::Active { id, .. } = *gesture {
                    requests.cancel_gesture = Some(id);
                    *gesture = MoveGesture::Idle;
                    overlay.0 = None;
                }
            }
            _ => {}
        }
    }
}

fn set_axis(gesture: &mut MoveGesture, which: usize) {
    if let MoveGesture::Active { axis, .. } = gesture {
        // Same axis again clears the constraint (Blender idiom).
        *axis = if *axis == Some(which) { None } else { Some(which) };
    }
}

/// Steer the gesture: translate the pointer delta (optionally axis-projected) into a
/// gesture-tagged transaction each frame it changes.
pub(crate) fn drive_gesture(
    mut gesture: ResMut<MoveGesture>,
    pointer: Res<GesturePointer>,
    mut edits: EditScope,
) {
    let MoveGesture::Active { id, axis, anchor, originals } = &mut *gesture else {
        return;
    };
    let Some(target) = pointer.0 else { return };
    let anchor_point = *anchor.get_or_insert(target);
    let mut delta = target - anchor_point;
    if let Some(axis) = axis {
        let mut constrained = Vec3::ZERO;
        constrained[*axis] = delta[*axis];
        delta = constrained;
    }
    if delta == Vec3::ZERO {
        return;
    }
    let mut transaction = edits.transaction("Move").gesture(*id);
    for (scene_id, original) in originals.iter() {
        let mut moved = *original;
        moved.translation = original.translation + delta;
        transaction = transaction.set(*scene_id, moved);
    }
    transaction.commit();
}

/// Real-app pointer source: cursor ray intersected with the ground-parallel plane
/// through the gesture's first original (good graybox behavior; snap solvers refine
/// this in the insert pass).
pub(crate) fn pointer_from_cursor(
    gesture: Res<MoveGesture>,
    camera: Query<(&Camera, &GlobalTransform)>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut pointer: ResMut<GesturePointer>,
) {
    let MoveGesture::Active { originals, .. } = &*gesture else {
        return;
    };
    let plane_height = originals.first().map(|(_, t)| t.translation.y).unwrap_or(0.0);
    let (Ok(window), Some((camera, camera_transform))) =
        (window.single(), camera.iter().find(|(c, _)| c.is_active))
    else {
        return;
    };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else { return };
    let Some(distance) = ray.intersect_plane(
        Vec3::new(0.0, plane_height, 0.0),
        InfinitePlane3d::new(Vec3::Y),
    ) else {
        return;
    };
    pointer.0 = Some(ray.get_point(distance));
}

/// Mouse-click commit (the pick-arbitration guard: selection skips while active).
pub(crate) fn commit_on_click(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut gesture: ResMut<MoveGesture>,
    mut overlay: ResMut<OverlayContext>,
) {
    if matches!(*gesture, MoveGesture::Idle) {
        return;
    }
    let Some(mouse) = mouse else { return };
    if mouse.just_pressed(MouseButton::Left) {
        *gesture = MoveGesture::Idle;
        overlay.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edits::History;
    use crate::resolver::EditorState;
    use crate::EditorCorePlugin;

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test", "Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Transform>();
        }
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

    fn spawn_selected(app: &mut App, at: Vec3) -> SceneId {
        let id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Transform::from_translation(at)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        id
    }

    fn translation(app: &mut App, id: SceneId) -> Vec3 {
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world().get::<Transform>(entity).unwrap().translation
    }

    fn point_at(app: &mut App, target: Vec3) {
        app.world_mut().resource_mut::<GesturePointer>().0 = Some(target);
        app.update();
    }

    // B7: drag is one coalesced entry; undo restores pre-gesture exactly.
    #[test]
    fn drag_commit_is_one_undo_entry() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::new(1.0, 0.5, 1.0));
        let depth_before = app.world().resource::<History>().undo_depth();

        invoke(&mut app, "transform.move");
        point_at(&mut app, Vec3::new(0.0, 0.5, 0.0)); // anchor
        for i in 1..=20 {
            point_at(&mut app, Vec3::new(i as f32 * 0.25, 0.5, 0.0));
        }
        assert_eq!(translation(&mut app, id), Vec3::new(6.0, 0.5, 1.0));
        invoke(&mut app, "transform.commit");

        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before + 1,
            "whole drag = one history entry"
        );
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        assert_eq!(translation(&mut app, id), Vec3::new(1.0, 0.5, 1.0));
    }

    // B7: Esc mid-drag restores originals exactly and leaves history untouched.
    #[test]
    fn cancel_restores_exactly() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::new(2.0, 0.0, 3.0));
        let depth_before = app.world().resource::<History>().undo_depth();

        invoke(&mut app, "transform.move");
        point_at(&mut app, Vec3::ZERO);
        point_at(&mut app, Vec3::new(5.0, 0.0, -2.0));
        assert_ne!(translation(&mut app, id), Vec3::new(2.0, 0.0, 3.0));

        invoke(&mut app, "transform.cancel");
        assert_eq!(translation(&mut app, id), Vec3::new(2.0, 0.0, 3.0), "exact restore");
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before,
            "cancelled gesture leaves no history"
        );
        assert!(matches!(*app.world().resource::<MoveGesture>(), MoveGesture::Idle));
        assert!(app.world().resource::<OverlayContext>().0.is_none());
    }

    // B7: axis constraint projects the delta.
    #[test]
    fn axis_constraint_projects() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.move");
        point_at(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.axis-x");
        point_at(&mut app, Vec3::new(3.0, 0.0, 9.0));
        assert_eq!(translation(&mut app, id), Vec3::new(3.0, 0.0, 0.0), "x-only");
        invoke(&mut app, "transform.cancel");
    }
}
