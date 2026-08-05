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
use crate::resolver::OverlayContext;
use crate::selection::Selected;

pub const GESTURE_MOVE_CONTEXT: ContextId = ContextId::new_static("gesture-move");

/// World-space movement for THIS frame. Fed by the screen-space conversion system
/// (v1's pixel-accurate math) in a real app; tests inject deltas directly.
#[derive(Resource, Default)]
pub struct GestureMotion(pub Option<Vec3>);

#[derive(Resource, Default)]
pub enum MoveGesture {
    #[default]
    Idle,
    Active {
        id: u64,
        axis: Option<usize>,
        /// Total world-space displacement applied so far.
        accumulated: Vec3,
        originals: Vec<(SceneId, Transform)>,
        /// Blender-style typed exact amount ("2", "-1.5") — spec M2 B7. Applies
        /// along the constrained axis; mouse motion resumes from it.
        typed: String,
    },
}

#[derive(Resource, Default)]
pub struct GestureCounter(u64);

impl GestureCounter {
    /// A fresh gesture id: transactions sharing it coalesce into ONE history entry
    /// (drags, held-key repeats, inspector slide-edits).
    pub fn begin(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

/// Typed exact amount → the gesture's displacement, live: each keystroke sets
/// translation = original + axis * value through the same coalesced transaction.
fn apply_typed(gesture: &mut MoveGesture, edits: &mut EditScope) {
    let MoveGesture::Active {
        id,
        axis,
        accumulated,
        originals,
        typed,
    } = gesture
    else {
        return;
    };
    let value: f32 = typed.parse().unwrap_or(0.0);
    let mut direction = Vec3::ZERO;
    // Unconstrained typed amounts run along X (constrain first for y/z).
    direction[axis.unwrap_or(0)] = 1.0;
    let desired = direction * value;
    *accumulated = desired;
    let mut transaction = edits.transaction("Move").gesture(*id);
    for (scene_id, original) in originals.iter() {
        let mut moved = *original;
        moved.translation = original.translation + desired;
        transaction = transaction.set(*scene_id, moved);
    }
    transaction.commit();
}

/// Consume gesture-related actions (any source — keys, palette, macros).
pub(crate) fn handle_gesture_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut gesture: ResMut<MoveGesture>,
    mut overlay: ResMut<OverlayContext>,
    mut counter: ResMut<GestureCounter>,
    mut motion: ResMut<GestureMotion>,
    mut requests: ResMut<HistoryRequests>,
    mut edits: EditScope,
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
                        accumulated: Vec3::ZERO,
                        originals,
                        typed: String::new(),
                    };
                    motion.0 = None;
                    overlay.0 = Some(GESTURE_MOVE_CONTEXT);
                }
            }
            "transform.axis-x" => set_axis(&mut gesture, 0),
            "transform.axis-y" => set_axis(&mut gesture, 1),
            "transform.axis-z" => set_axis(&mut gesture, 2),
            "transform.digit-erase" => {
                if let MoveGesture::Active { typed, .. } = &mut *gesture {
                    typed.pop();
                    apply_typed(&mut gesture, &mut edits);
                }
            }
            action if action.starts_with("transform.digit-") => {
                if let MoveGesture::Active { typed, .. } = &mut *gesture {
                    let glyph = match action.strip_prefix("transform.digit-").unwrap() {
                        "dot" => ".",
                        "minus" => {
                            // Toggle the sign (Blender idiom).
                            if typed.starts_with('-') {
                                typed.remove(0);
                            } else {
                                typed.insert(0, '-');
                            }
                            apply_typed(&mut gesture, &mut edits);
                            continue;
                        }
                        digit => digit,
                    };
                    typed.push_str(glyph);
                    apply_typed(&mut gesture, &mut edits);
                }
            }
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
        *axis = if *axis == Some(which) {
            None
        } else {
            Some(which)
        };
    }
}

/// Consume this frame's world-space motion (already pixel-accurate from the
/// conversion system) into a gesture-tagged transaction.
pub(crate) fn drive_gesture(
    mut gesture: ResMut<MoveGesture>,
    mut motion: ResMut<GestureMotion>,
    mut edits: EditScope,
) {
    let MoveGesture::Active {
        id,
        axis,
        accumulated,
        originals,
        ..
    } = &mut *gesture
    else {
        return;
    };
    let Some(mut delta) = motion.0.take() else {
        return;
    };
    if let Some(axis) = axis {
        let mut constrained = Vec3::ZERO;
        constrained[*axis] = delta[*axis];
        delta = constrained;
    }
    if delta == Vec3::ZERO {
        return;
    }
    *accumulated += delta;
    let total = *accumulated;
    let mut transaction = edits.transaction("Move").gesture(*id);
    for (scene_id, original) in originals.iter() {
        let mut moved = *original;
        moved.translation = original.translation + total;
        transaction = transaction.set(*scene_id, moved);
    }
    transaction.commit();
}

/// v1's keep-list drag math (gizmos/transform.rs:28-65): project the pivot and a
/// point one world unit along `axis_dir` to the viewport — the screen distance is
/// "pixels per world unit", giving exact 1:1 mouse tracking at any depth/FOV.
fn axis_movement(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    pivot: Vec3,
    axis_dir: Vec3,
    mouse_delta: Vec2,
) -> f32 {
    let Ok(screen_pos) = camera.world_to_viewport(camera_transform, pivot) else {
        return 0.0;
    };
    let Ok(screen_axis_pos) = camera.world_to_viewport(camera_transform, pivot + axis_dir) else {
        return 0.0;
    };
    let screen_axis = screen_axis_pos - screen_pos;
    let pixels_per_unit = screen_axis.length();
    if pixels_per_unit < 0.001 {
        // Axis points at/away from the camera.
        return -mouse_delta.y * 0.01;
    }
    mouse_delta.dot(screen_axis / pixels_per_unit) / pixels_per_unit
}

/// Real-app motion source: cursor pixel delta → world delta via the v1 math. Free
/// drag moves in the camera plane (right/up axes); an axis constraint projects onto
/// that world axis with the same pixel accuracy.
pub(crate) fn motion_from_cursor(
    gesture: Res<MoveGesture>,
    camera: Query<(
        &Camera,
        &GlobalTransform,
        Option<&bevy::camera::RenderTarget>,
    )>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut last_cursor: Local<Option<Vec2>>,
    mut motion: ResMut<GestureMotion>,
) {
    let MoveGesture::Active {
        axis,
        accumulated,
        originals,
        ..
    } = &*gesture
    else {
        *last_cursor = None;
        return;
    };
    let (Ok(window), Some((camera, camera_transform, _))) = (
        window.single(),
        camera
            .iter()
            .find(|(c, _, target)| crate::camera::is_viewport_camera(c, *target)),
    ) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some(last) = *last_cursor else {
        *last_cursor = Some(cursor);
        return;
    };
    let mouse_delta = cursor - last;
    *last_cursor = Some(cursor);
    if mouse_delta == Vec2::ZERO {
        return;
    }
    let pivot = originals
        .first()
        .map(|(_, t)| t.translation)
        .unwrap_or(Vec3::ZERO)
        + *accumulated;

    let world_delta = match axis {
        Some(index) => {
            let mut axis_dir = Vec3::ZERO;
            axis_dir[*index] = 1.0;
            axis_dir * axis_movement(camera, camera_transform, pivot, axis_dir, mouse_delta)
        }
        None => {
            let right = *camera_transform.right();
            let up = *camera_transform.up();
            right * axis_movement(camera, camera_transform, pivot, right, mouse_delta)
                + up * axis_movement(camera, camera_transform, pivot, up, mouse_delta)
        }
    };
    motion.0 = Some(world_delta);
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
    use crate::EditorCorePlugin;
    use crate::edits::History;
    use crate::resolver::EditorState;

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

    fn push_delta(app: &mut App, delta: Vec3) {
        app.world_mut().resource_mut::<GestureMotion>().0 = Some(delta);
        app.update();
    }

    // B7: drag is one coalesced entry; undo restores pre-gesture exactly.
    #[test]
    fn drag_commit_is_one_undo_entry() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::new(1.0, 0.5, 1.0));
        let depth_before = app.world().resource::<History>().undo_depth();

        invoke(&mut app, "transform.move");
        for _ in 1..=20 {
            push_delta(&mut app, Vec3::new(0.25, 0.0, 0.0));
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
        push_delta(&mut app, Vec3::new(5.0, 0.0, -2.0));
        assert_ne!(translation(&mut app, id), Vec3::new(2.0, 0.0, 3.0));

        invoke(&mut app, "transform.cancel");
        assert_eq!(
            translation(&mut app, id),
            Vec3::new(2.0, 0.0, 3.0),
            "exact restore"
        );
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before,
            "cancelled gesture leaves no history"
        );
        assert!(matches!(
            *app.world().resource::<MoveGesture>(),
            MoveGesture::Idle
        ));
        assert!(app.world().resource::<OverlayContext>().0.is_none());
    }

    // B7: axis constraint projects the delta.
    #[test]
    fn axis_constraint_projects() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.move");
        invoke(&mut app, "transform.axis-x");
        push_delta(&mut app, Vec3::new(3.0, 0.0, 9.0));
        assert_eq!(
            translation(&mut app, id),
            Vec3::new(3.0, 0.0, 0.0),
            "x-only"
        );
        invoke(&mut app, "transform.cancel");
    }
}

#[cfg(test)]
mod typed_amount_tests {
    use super::*;
    use crate::EditorCorePlugin;
    use editor_api::prelude::*;

    // Spec M2 B7: typed digits during a gesture are EXACT amounts along the
    // constrained axis, committed through the same coalesced transaction.
    #[test]
    fn typed_amount_moves_exactly() {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        struct F;
        impl EditorFeature for F {
            fn manifest(&self) -> FeatureManifest {
                FeatureManifest::new("t", "T")
            }
            fn register(&self, reg: &mut FeatureRegistry) {
                reg.component::<Transform>();
            }
        }
        app.add_editor_feature(F);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.init_resource::<bevy::input::ButtonInput<bevy::input::mouse::MouseButton>>();
        app.finish();
        app.update();
        app.world_mut()
            .resource_mut::<crate::resolver::EditorState>()
            .active = true;

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
                        Box::new(Transform::from_xyz(1.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world_mut()
            .entity_mut(entity)
            .insert(crate::selection::Selected);

        for action in [
            "transform.move",
            "transform.axis-z",
            "transform.digit-2",
            "transform.digit-dot",
            "transform.digit-5",
            "transform.commit",
        ] {
            app.world_mut().write_message(ActionInvoked {
                action: ActionId::new(action.to_string()),
                args: None,
                source: InvocationSource::Test,
            });
            app.update();
        }
        let world = app.world_mut();
        let entity = world.resource::<SceneIndex>().get(&id).unwrap();
        let translation = world.get::<Transform>(entity).unwrap().translation;
        assert_eq!(
            translation,
            Vec3::new(1.0, 0.0, 2.5),
            "w z 2.5 ⏎ = exactly +2.5 on Z"
        );
    }
}
