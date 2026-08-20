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

/// Per-frame pointer motion for the active gesture: the world delta a MOVE
/// consumes, and the raw screen delta a ROTATE turns into an angle.
#[derive(Resource, Default)]
pub struct GestureMotion {
    pub world: Option<Vec3>,
    pub screen: Option<Vec2>,
}

/// What the active gesture does with its input. Both share the axis
/// constraints, typed amounts, coalescing and commit/cancel grammar — only the
/// transform they compute differs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GestureKind {
    #[default]
    Move,
    Rotate,
    Scale,
}

impl GestureKind {
    /// The ONE name for this gesture — the transaction label in history and the
    /// mode readout in the statusbar, so the two can never drift apart.
    pub fn label(self) -> &'static str {
        match self {
            GestureKind::Move => "Move",
            GestureKind::Rotate => "Rotate",
            GestureKind::Scale => "Scale",
        }
    }
}

#[derive(Resource, Default)]
pub enum MoveGesture {
    #[default]
    Idle,
    Active {
        id: u64,
        kind: GestureKind,
        axis: Option<usize>,
        /// Move: total world displacement. Rotate: `x` carries the angle in
        /// DEGREES. Scale: `x` carries the FACTOR, seeded to 1. One accumulator
        /// either way, so typed amounts and drags interchange.
        accumulated: Vec3,
        /// Rotate about THIS point when set; the selection's centroid otherwise.
        pivot: Option<Vec3>,
        originals: Vec<(SceneId, Transform)>,
        /// Blender-style typed exact amount ("2", "-1.5") — spec M2 B7. Applies
        /// along the constrained axis; mouse motion resumes from it.
        typed: String,
    },
}

/// An explicit pivot for the next ROTATE, and what it should turn. Set by
/// whoever owns the concept (editor_prefabs pins it to a selected socket), so
/// the kernel stays ignorant of sockets while still supporting "rotate this
/// piece about that point".
#[derive(Resource, Default)]
pub struct GesturePivot {
    pub subject: Option<SceneId>,
    pub pivot: Option<Vec3>,
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
/// THE gesture application (typed amounts and drags both land here, so they can
/// never disagree). `amount` is a world delta for a move, and degrees about the
/// axis for a rotate.
fn apply_gesture(
    kind: GestureKind,
    id: u64,
    axis: Option<usize>,
    amount: Vec3,
    explicit_pivot: Option<Vec3>,
    originals: &[(SceneId, Transform)],
    // What a DRAG lands on when snapping is enabled: a grid step for a move, an
    // angle step for a rotate. Typed amounts pass `None` — "w x 2.5" is an
    // exact instruction, and quantizing its result would contradict what the
    // user just spelled out.
    snap_step: Option<f32>,
    edits: &mut EditScope,
) {
    let mut transaction = edits.transaction(kind.label()).gesture(id);
    match kind {
        GestureKind::Move => {
            for (scene_id, original) in originals {
                let mut moved = *original;
                moved.translation = original.translation + amount;
                if let Some(step) = snap_step {
                    moved.translation = quantize(moved.translation, axis, step);
                }
                transaction = transaction.set(*scene_id, moved);
            }
        }
        GestureKind::Rotate => {
            // Unconstrained rotation is YAW — the one that needs no camera to
            // be predictable, and the one level layout wants nine times in ten.
            let mut direction = Vec3::ZERO;
            direction[axis.unwrap_or(1)] = 1.0;
            // Snapping the ANGLE, not the resulting orientation: quantizing the
            // final quaternion would fight any rotation the piece already had.
            let degrees = match snap_step {
                Some(step) if step > 0.0 => (amount.x / step).round() * step,
                _ => amount.x,
            };
            let spin = Quat::from_axis_angle(direction, degrees.to_radians());
            let pivot = pivot_of(explicit_pivot, originals);
            for (scene_id, original) in originals {
                let mut turned = *original;
                turned.rotation = spin * original.rotation;
                turned.translation = pivot + spin * (original.translation - pivot);
                transaction = transaction.set(*scene_id, turned);
            }
        }
        GestureKind::Scale => {
            // Unconstrained scale is uniform; a constraint makes it ONE axis,
            // which is how a cube becomes a wall without opening the inspector.
            let factor = amount.x.max(MIN_SCALE);
            let mut ratio = Vec3::splat(factor);
            if let Some(axis) = axis {
                ratio = Vec3::ONE;
                ratio[axis] = factor;
            }
            let pivot = pivot_of(explicit_pivot, originals);
            for (scene_id, original) in originals {
                let mut scaled = *original;
                scaled.scale = original.scale * ratio;
                // Offsets grow with the group, so a multi-selection scales
                // APART rather than every piece swelling in place.
                scaled.translation = pivot + (original.translation - pivot) * ratio;
                transaction = transaction.set(*scene_id, scaled);
            }
        }
    }
    transaction.commit();
}

/// What a rotate or a scale pivots about: an explicit pivot (a pinned socket)
/// wins; otherwise a multi-selection uses its shared centroid, and a single one
/// uses its own origin — so it turns or grows in place.
fn pivot_of(explicit: Option<Vec3>, originals: &[(SceneId, Transform)]) -> Vec3 {
    explicit.unwrap_or_else(|| {
        originals
            .iter()
            .map(|(_, t)| t.translation)
            .fold(Vec3::ZERO, |acc, t| acc + t)
            / originals.len().max(1) as f32
    })
}

/// Land a dragged position on the grid, quantizing only the axes the move
/// actually touches: a constrained move lands on the grid along its own axis,
/// and an unconstrained one lands on the GROUND grid while keeping its height —
/// dragging a piece across a half-metre platform must not drop it to the floor.
fn quantize(translation: Vec3, axis: Option<usize>, step: f32) -> Vec3 {
    if step <= 0.0 {
        return translation;
    }
    let mut out = translation;
    let mut snap = |index: usize| out[index] = (translation[index] / step).round() * step;
    match axis {
        Some(axis) => snap(axis),
        None => {
            snap(0);
            snap(2);
        }
    }
    out
}

/// Scale never reaches zero and never flips negative: a zero scale makes
/// degenerate colliders and NaN normals, and a mirror is not something a drag
/// should be able to produce by accident.
const MIN_SCALE: f32 = 0.001;

fn apply_typed(gesture: &mut MoveGesture, edits: &mut EditScope) {
    let MoveGesture::Active {
        id,
        kind,
        axis,
        accumulated,
        pivot,
        originals,
        typed,
    } = gesture
    else {
        return;
    };
    let parsed = typed.parse::<f32>().ok();
    let desired = match kind {
        // Unconstrained typed amounts run along X (constrain first for y/z).
        GestureKind::Move => {
            let mut direction = Vec3::ZERO;
            direction[axis.unwrap_or(0)] = 1.0;
            direction * parsed.unwrap_or(0.0)
        }
        // Degrees, whatever the axis.
        GestureKind::Rotate => Vec3::X * parsed.unwrap_or(0.0),
        // The FACTOR itself, so "r 2 ⏎" is exactly twice as big. A half-typed
        // buffer has to mean 1× rather than 0×: an empty one after a backspace,
        // a lone "-", and the "0" on the way to "0.5" are all states the user
        // passes THROUGH, and collapsing the selection at each of them (the
        // floor takes 0 to near-nothing) makes the object flicker out of
        // existence mid-keystroke.
        GestureKind::Scale => Vec3::X * parsed.filter(|value| *value > 0.0).unwrap_or(1.0),
    };
    *accumulated = desired;
    apply_gesture(*kind, *id, *axis, desired, *pivot, originals, None, edits);
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
    scene: Query<(&SceneId, &Transform)>,
    pin: Res<GesturePivot>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            action @ ("transform.move" | "transform.rotate" | "transform.scale") => {
                if matches!(*gesture, MoveGesture::Idle) {
                    let rotating = action == "transform.rotate";
                    // PIVOT ON SOCKET: with a socket pinned, a rotate turns the
                    // piece that owns it about that point, so the mated joint
                    // stays put and the far end swings — corners and curves.
                    let pinned = rotating
                        .then(|| {
                            let pin = &*pin;
                            pin.subject.zip(pin.pivot)
                        })
                        .flatten()
                        .and_then(|(subject, at)| {
                            let transform = scene
                                .iter()
                                .find(|(id, _)| **id == subject)
                                .map(|(_, t)| *t)?;
                            Some((vec![(subject, transform)], at))
                        });
                    let (originals, pivot) = match pinned {
                        Some((originals, at)) => (originals, Some(at)),
                        None => (
                            selected.iter().map(|(id, t)| (*id, *t)).collect::<Vec<_>>(),
                            None,
                        ),
                    };
                    if originals.is_empty() {
                        continue;
                    }
                    counter.0 += 1;
                    let kind = match action {
                        "transform.rotate" => GestureKind::Rotate,
                        "transform.scale" => GestureKind::Scale,
                        _ => GestureKind::Move,
                    };
                    *gesture = MoveGesture::Active {
                        id: counter.0,
                        kind,
                        axis: None,
                        // Scale's accumulator IS the factor, so it starts at 1×
                        // — zero would collapse the selection on frame one.
                        accumulated: match kind {
                            GestureKind::Scale => Vec3::X,
                            _ => Vec3::ZERO,
                        },
                        pivot,
                        originals,
                        typed: String::new(),
                    };
                    motion.world = None;
                    motion.screen = None;
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

/// PUSH AND PULL: the wheel moves the dragged object along the view axis.
///
/// A free drag moves in the CAMERA PLANE — right and up relative to the view —
/// which gives two of the three dimensions and no way to say "further away" or
/// "closer to me". That is what makes moving feel wrong: you can slide a crate
/// across the screen all day and never get it onto the far side of the room
/// without flying the camera there first.
///
/// The push feeds the SAME motion channel the cursor does, so it inherits grid
/// snapping, the axis constraints, the coalesced transaction and the undo entry
/// — pushing and dragging are one gesture, not two.
pub(crate) fn push_pull_gesture(
    gesture: Res<MoveGesture>,
    settings: Res<crate::settings::EditorSettings>,
    camera: Query<(
        &Camera,
        &GlobalTransform,
        Option<&bevy::camera::RenderTarget>,
    )>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut motion: ResMut<GestureMotion>,
) {
    if !matches!(
        *gesture,
        MoveGesture::Active {
            kind: GestureKind::Move,
            ..
        }
    ) {
        // Not ours: the camera's zoom will take it.
        return;
    }
    let notches: f32 = wheel
        .read()
        .map(|event| match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => event.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => event.y / 50.0,
        })
        .sum();
    if notches == 0.0 {
        return;
    }
    let Some((_, camera_transform, _)) = camera
        .iter()
        .find(|(camera, _, target)| crate::camera::is_viewport_camera(camera, *target))
    else {
        return;
    };
    let forward = camera_transform.forward();
    *motion.world.get_or_insert(Vec3::ZERO) += forward * notches * settings.camera.zoom_step;
}

/// Consume this frame's world-space motion (already pixel-accurate from the
/// conversion system) into a gesture-tagged transaction.
pub(crate) fn drive_gesture(
    mut gesture: ResMut<MoveGesture>,
    mut motion: ResMut<GestureMotion>,
    grid: Res<crate::insert::GridSnap>,
    angle: Res<crate::insert::AngleSnap>,
    settings: Res<crate::settings::EditorSettings>,
    mut edits: EditScope,
) {
    let MoveGesture::Active {
        id,
        kind,
        axis,
        accumulated,
        pivot,
        originals,
        ..
    } = &mut *gesture
    else {
        return;
    };
    let delta = match kind {
        GestureKind::Move => {
            let Some(mut delta) = motion.world.take() else {
                return;
            };
            if let Some(axis) = axis {
                let mut constrained = Vec3::ZERO;
                constrained[*axis] = delta[*axis];
                delta = constrained;
            }
            delta
        }
        // Horizontal drag turns; the vertical component would fight the axis
        // constraint for no gain.
        GestureKind::Rotate => {
            let Some(screen) = motion.screen.take() else {
                return;
            };
            Vec3::X * screen.x * ROTATE_DEGREES_PER_PIXEL
        }
        // Drag right to grow, left to shrink — the same horizontal grammar.
        GestureKind::Scale => {
            let Some(screen) = motion.screen.take() else {
                return;
            };
            Vec3::X * screen.x * SCALE_PER_PIXEL
        }
    };
    if delta == Vec3::ZERO {
        return;
    }
    match kind {
        // Scale accumulates MULTIPLICATIVELY: the drag is an exponent, so equal
        // travel each way are exact inverses (200px doubles, 200px back halves)
        // and the factor approaches zero asymptotically instead of crossing it.
        // Adding pixels to a ratio instead would give shrink a hard 200px of
        // range and then wind up off the floor, leaving a dead zone the user
        // has to drag back out of before anything moves.
        GestureKind::Scale => accumulated.x = (accumulated.x * delta.x.exp()).max(MIN_SCALE),
        _ => *accumulated += delta,
    }
    // B9's quantization was reachable from placement only: the toggle says it
    // snaps "placement and movement", and nothing in the gesture path had ever
    // read it, so a drag landed wherever the mouse stopped.
    let snap_step = match kind {
        GestureKind::Move => grid.enabled.then_some(settings.viewport.grid_step),
        GestureKind::Rotate => angle.enabled.then_some(settings.viewport.angle_step),
        // Scale has no natural quantum — a ratio is not a distance.
        GestureKind::Scale => None,
    }
    .filter(|step| *step > 0.0);
    apply_gesture(
        *kind,
        *id,
        *axis,
        *accumulated,
        *pivot,
        originals,
        snap_step,
        &mut edits,
    );
}

/// Drag sensitivity: a full 360° needs a deliberate sweep, not a flick.
const ROTATE_DEGREES_PER_PIXEL: f32 = 0.5;

/// Drag sensitivity: the drag is an EXPONENT (see `drive_gesture`), so ~200px
/// doubles the selection and ~200px the other way halves it.
const SCALE_PER_PIXEL: f32 = 0.003_466;

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
    motion.world = Some(world_delta);
    motion.screen = Some(mouse_delta);
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
        app.world_mut().resource_mut::<GestureMotion>().world = Some(delta);
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
    // Owner ask: rotate shares the move grammar — typed amounts are DEGREES,
    // axis constraints apply, and the whole gesture is ONE undo entry.
    #[test]
    fn rotate_typed_degrees_about_an_axis() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        let depth_before = app.world().resource::<History>().undo_depth();

        invoke(&mut app, "transform.rotate");
        invoke(&mut app, "transform.axis-y");
        for action in ["transform.digit-9", "transform.digit-0"] {
            invoke(&mut app, action);
        }
        invoke(&mut app, "transform.commit");
        app.update();

        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        let rotation = app.world().get::<Transform>(entity).unwrap().rotation;
        // 90° of yaw takes +Z to +X.
        assert!(
            (rotation * Vec3::Z).abs_diff_eq(Vec3::X, 1e-4),
            "r y 90 ⏎ yaws a quarter turn: {:?}",
            rotation * Vec3::Z
        );
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before + 1,
            "the whole rotate is ONE history entry"
        );
    }

    fn scale_of(app: &mut App, id: SceneId) -> Vec3 {
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world().get::<Transform>(entity).unwrap().scale
    }

    // Spec §9 blockout: scale shares the whole move/rotate grammar, and a typed
    // amount is the FACTOR — "s 2 ⏎" is exactly twice as big, in one undo step.
    #[test]
    fn scale_typed_factor_resizes_exactly() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        let depth_before = app.world().resource::<History>().undo_depth();

        invoke(&mut app, "transform.scale");
        invoke(&mut app, "transform.digit-2");
        invoke(&mut app, "transform.commit");
        app.update();

        assert_eq!(scale_of(&mut app, id), Vec3::splat(2.0), "s 2 ⏎ doubles");
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before + 1,
            "the whole scale is ONE history entry"
        );
    }

    // THE blockout move: constrain an axis and a cube becomes a wall, without
    // ever opening the inspector.
    #[test]
    fn scale_axis_constraint_is_non_uniform() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.scale");
        invoke(&mut app, "transform.axis-x");
        invoke(&mut app, "transform.digit-4");
        invoke(&mut app, "transform.commit");
        app.update();
        assert_eq!(
            scale_of(&mut app, id),
            Vec3::new(4.0, 1.0, 1.0),
            "x-only: a cube becomes a wall"
        );
    }

    // A scale must not displace a single selection — it grows in place.
    #[test]
    fn scale_keeps_a_single_selection_in_place() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::new(3.0, 1.0, -2.0));
        invoke(&mut app, "transform.scale");
        invoke(&mut app, "transform.digit-3");
        invoke(&mut app, "transform.commit");
        app.update();
        assert_eq!(
            translation(&mut app, id),
            Vec3::new(3.0, 1.0, -2.0),
            "scale grows in place"
        );
    }

    // Half-typed input is a state you pass THROUGH: the empty buffer after a
    // backspace, a lone "-", and the "0" on the way to "0.5". Each has to leave
    // the selection at 1x — treating them as a factor of zero made the object
    // flicker out of existence mid-keystroke, and committing there persisted it.
    #[test]
    fn a_half_typed_scale_holds_at_identity() {
        for keystrokes in [
            vec!["transform.digit-0"],
            vec!["transform.digit-minus"],
            vec!["transform.digit-dot"],
            vec!["transform.digit-5", "transform.digit-erase"],
            vec!["transform.digit-erase"],
        ] {
            let mut app = test_app();
            let id = spawn_selected(&mut app, Vec3::ZERO);
            invoke(&mut app, "transform.scale");
            for action in &keystrokes {
                invoke(&mut app, action);
            }
            app.update();
            assert_eq!(
                scale_of(&mut app, id),
                Vec3::ONE,
                "{keystrokes:?} holds the selection at 1x"
            );
            invoke(&mut app, "transform.cancel");
        }
    }

    // Typing the fraction all the way through still lands exactly.
    #[test]
    fn a_fractional_typed_scale_lands_exactly() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.scale");
        for action in [
            "transform.digit-0",
            "transform.digit-dot",
            "transform.digit-5",
        ] {
            invoke(&mut app, action);
        }
        invoke(&mut app, "transform.commit");
        app.update();
        assert_eq!(scale_of(&mut app, id), Vec3::splat(0.5), "r 0.5 ⏎ halves");
    }

    // Esc restores the original size exactly and leaves no history — the same
    // contract move and rotate already hold to.
    #[test]
    fn cancelled_scale_restores_and_leaves_no_history() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        let depth_before = app.world().resource::<History>().undo_depth();
        invoke(&mut app, "transform.scale");
        invoke(&mut app, "transform.digit-5");
        invoke(&mut app, "transform.cancel");
        app.update();
        assert_eq!(scale_of(&mut app, id), Vec3::ONE, "exact restore");
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before,
            "cancelled gesture leaves no history"
        );
    }

    // A rotate must not displace a single selection — it turns in place.
    #[test]
    fn rotate_keeps_a_single_selection_in_place() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::new(3.0, 1.0, -2.0));
        invoke(&mut app, "transform.rotate");
        invoke(&mut app, "transform.digit-4");
        invoke(&mut app, "transform.digit-5");
        invoke(&mut app, "transform.commit");
        app.update();
        assert_eq!(
            translation(&mut app, id),
            Vec3::new(3.0, 1.0, -2.0),
            "rotation turns in place"
        );
    }

    // Owner: 1/2/3 front/left/top, shift for the opposite face — axis-aligned
    // and orthographic, framing whatever you are looking at.
    #[test]
    fn axis_views_face_their_axis_orthographically() {
        let mut app = test_app();
        let _ = spawn_selected(&mut app, Vec3::new(2.0, 1.0, -3.0));
        // A viewport camera: active, rendering to a window (no explicit target).
        app.world_mut().spawn((
            Camera3d::default(),
            Camera::default(),
            Projection::Perspective(PerspectiveProjection::default()),
            Transform::default(),
        ));
        app.update();

        for (action, expect_along) in [
            ("view.front", Vec3::NEG_Z),
            ("view.back", Vec3::Z),
            ("view.left", Vec3::X),
            ("view.right", Vec3::NEG_X),
            ("view.top", Vec3::NEG_Y),
            ("view.bottom", Vec3::Y),
        ] {
            invoke(&mut app, action);
            app.update();
            let world = app.world_mut();
            let (transform, projection) = world
                .query::<(&Transform, &Projection, &Camera)>()
                .iter(world)
                .map(|(t, p, _)| (*t, matches!(p, Projection::Orthographic(_))))
                .next()
                .expect("a viewport camera");
            assert!(
                transform
                    .forward()
                    .as_vec3()
                    .abs_diff_eq(expect_along, 1e-4),
                "{action} looks along {expect_along:?}, got {:?}",
                transform.forward()
            );
            assert!(projection, "{action} is orthographic");
        }

        // Flying leaves the canonical view: holding RMB hands perspective back.
        {
            let world = app.world_mut();
            let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
            mouse.press(MouseButton::Right);
        }
        app.update();
        let world = app.world_mut();
        let perspective = world
            .query::<(&Projection, &Camera)>()
            .iter(world)
            .any(|(p, _)| matches!(p, Projection::Perspective(_)));
        assert!(perspective, "flying restores perspective");
    }

    fn push_screen(app: &mut App, dx: f32) {
        app.world_mut().resource_mut::<GestureMotion>().screen = Some(Vec2::new(dx, 0.0));
        app.update();
    }

    // B9's toggle promises to quantize "placement and movement"; nothing in the
    // gesture path had ever read it, so a drag landed wherever the mouse
    // stopped. A DRAG lands on the grid...
    #[test]
    fn a_dragged_move_lands_on_the_grid() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        app.world_mut()
            .resource_mut::<crate::insert::GridSnap>()
            .enabled = true;
        app.world_mut()
            .resource_mut::<crate::settings::EditorSettings>()
            .viewport
            .grid_step = 0.5;
        invoke(&mut app, "transform.move");
        push_delta(&mut app, Vec3::new(0.31, 0.0, 0.79));
        let landed = translation(&mut app, id);
        assert_eq!(
            landed,
            Vec3::new(0.5, 0.0, 1.0),
            "a drag quantizes to the grid: {landed:?}"
        );
        invoke(&mut app, "transform.cancel");
    }

    // ...and with the toggle off it lands exactly where the mouse left it.
    #[test]
    fn grid_snap_off_leaves_a_drag_untouched() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.move");
        push_delta(&mut app, Vec3::new(0.31, 0.0, 0.79));
        assert_eq!(
            translation(&mut app, id),
            Vec3::new(0.31, 0.0, 0.79),
            "no snapping without the toggle"
        );
        invoke(&mut app, "transform.cancel");
    }

    // Height survives an unconstrained drag: a piece standing on a half-metre
    // platform must not drop to the floor because the grid is whole metres.
    #[test]
    fn grid_snap_keeps_height_on_an_unconstrained_drag() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::new(0.0, 0.5, 0.0));
        app.world_mut()
            .resource_mut::<crate::insert::GridSnap>()
            .enabled = true;
        app.world_mut()
            .resource_mut::<crate::settings::EditorSettings>()
            .viewport
            .grid_step = 1.0;
        invoke(&mut app, "transform.move");
        push_delta(&mut app, Vec3::new(0.4, 0.0, 0.4));
        let landed = translation(&mut app, id);
        assert_eq!(landed.y, 0.5, "the platform height survives: {landed:?}");
        invoke(&mut app, "transform.cancel");
    }

    // Constraining an axis quantizes THAT axis — including Y, when that is
    // what the user asked for.
    #[test]
    fn grid_snap_quantizes_the_constrained_axis() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::new(0.2, 0.0, 0.2));
        app.world_mut()
            .resource_mut::<crate::insert::GridSnap>()
            .enabled = true;
        app.world_mut()
            .resource_mut::<crate::settings::EditorSettings>()
            .viewport
            .grid_step = 1.0;
        invoke(&mut app, "transform.move");
        invoke(&mut app, "transform.axis-y");
        push_delta(&mut app, Vec3::new(0.0, 2.4, 0.0));
        let landed = translation(&mut app, id);
        assert_eq!(
            landed,
            Vec3::new(0.2, 2.0, 0.2),
            "y quantizes, x and z are left exactly where they were: {landed:?}"
        );
        invoke(&mut app, "transform.cancel");
    }

    // A typed amount is an exact instruction and must NOT be quantized.
    #[test]
    fn a_typed_move_ignores_the_grid() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        app.world_mut()
            .resource_mut::<crate::insert::GridSnap>()
            .enabled = true;
        app.world_mut()
            .resource_mut::<crate::settings::EditorSettings>()
            .viewport
            .grid_step = 1.0;
        invoke(&mut app, "transform.move");
        invoke(&mut app, "transform.axis-x");
        for action in [
            "transform.digit-2",
            "transform.digit-dot",
            "transform.digit-5",
        ] {
            invoke(&mut app, action);
        }
        invoke(&mut app, "transform.commit");
        app.update();
        assert_eq!(
            translation(&mut app, id),
            Vec3::new(2.5, 0.0, 0.0),
            "w x 2.5 means 2.5, grid or no grid"
        );
    }

    // Spec §9 lists grid AND angle toggles. A drag lands on the angle step, so
    // a wall meets a wall without anyone typing an exact number.
    #[test]
    fn a_dragged_rotate_lands_on_the_angle_step() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        app.world_mut()
            .resource_mut::<crate::insert::AngleSnap>()
            .enabled = true;
        invoke(&mut app, "transform.rotate");
        // 40px of drag is 20°, which quantizes to 15°.
        push_screen(&mut app, 40.0);
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        let rotation = app.world().get::<Transform>(entity).unwrap().rotation;
        let (_, yaw, _) = rotation.to_euler(EulerRot::XYZ);
        assert!(
            (yaw.to_degrees() - 15.0).abs() < 0.01,
            "20° of drag snaps to 15°: {}",
            yaw.to_degrees()
        );
        invoke(&mut app, "transform.cancel");
    }

    // Off by default, and off means exactly what the mouse did.
    #[test]
    fn angle_snap_off_leaves_a_rotate_untouched() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.rotate");
        push_screen(&mut app, 40.0);
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        let rotation = app.world().get::<Transform>(entity).unwrap().rotation;
        let (_, yaw, _) = rotation.to_euler(EulerRot::XYZ);
        assert!(
            (yaw.to_degrees() - 20.0).abs() < 0.01,
            "no snapping without the toggle: {}",
            yaw.to_degrees()
        );
        invoke(&mut app, "transform.cancel");
    }

    // A typed angle is exact: "e 37 ⏎" means 37, snap or no snap.
    #[test]
    fn a_typed_rotate_ignores_the_angle_step() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        app.world_mut()
            .resource_mut::<crate::insert::AngleSnap>()
            .enabled = true;
        invoke(&mut app, "transform.rotate");
        for action in ["transform.digit-3", "transform.digit-7"] {
            invoke(&mut app, action);
        }
        invoke(&mut app, "transform.commit");
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        let rotation = app.world().get::<Transform>(entity).unwrap().rotation;
        let (_, yaw, _) = rotation.to_euler(EulerRot::XYZ);
        assert!(
            (yaw.to_degrees() - 37.0).abs() < 0.01,
            "typed angles are exact: {}",
            yaw.to_degrees()
        );
    }

    // The toggle is an ACTION like everything else, and it is its own toggle —
    // laying a run on the grid while dialling a free angle has to be possible.
    #[test]
    fn the_angle_toggle_is_independent_of_the_grid_toggle() {
        let mut app = test_app();
        invoke(&mut app, "core.toggle-angle-snap");
        assert!(app.world().resource::<crate::insert::AngleSnap>().enabled);
        assert!(
            !app.world().resource::<crate::insert::GridSnap>().enabled,
            "the grid toggle is untouched"
        );
        invoke(&mut app, "core.toggle-angle-snap");
        assert!(!app.world().resource::<crate::insert::AngleSnap>().enabled);
    }
    // Scale is a RATIO, so the drag has to compose multiplicatively: equal
    // travel each way must cancel exactly. Accumulating pixels into the factor
    // instead gave shrink 200px of range and then wound up below the floor,
    // leaving a dead zone you had to drag back out of before anything moved.
    #[test]
    fn scale_drag_is_symmetric_and_never_winds_up() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.scale");
        for _ in 0..100 {
            push_screen(&mut app, -10.0); // 1000px left: deep into shrink
        }
        let shrunk = scale_of(&mut app, id).x;
        assert!(shrunk > 0.0, "never reaches zero: {shrunk}");
        // The very next rightward pixel has to MOVE it — no dead zone.
        push_screen(&mut app, 10.0);
        assert!(
            scale_of(&mut app, id).x > shrunk,
            "a rightward drag responds immediately: {} then {}",
            shrunk,
            scale_of(&mut app, id).x
        );
        for _ in 0..99 {
            push_screen(&mut app, 10.0); // retrace the remaining 990px
        }
        let back = scale_of(&mut app, id).x;
        assert!(
            (back - 1.0).abs() < 1e-3,
            "equal travel each way cancels exactly: {back}"
        );
        invoke(&mut app, "transform.cancel");
    }

    // ~200px doubles, and the same distance the other way halves.
    #[test]
    fn scale_drag_sensitivity_is_a_doubling_per_200px() {
        let mut app = test_app();
        let id = spawn_selected(&mut app, Vec3::ZERO);
        invoke(&mut app, "transform.scale");
        for _ in 0..20 {
            push_screen(&mut app, 10.0);
        }
        let grown = scale_of(&mut app, id).x;
        assert!((grown - 2.0).abs() < 0.01, "200px doubles: {grown}");
        for _ in 0..40 {
            push_screen(&mut app, -10.0);
        }
        let halved = scale_of(&mut app, id).x;
        assert!((halved - 0.5).abs() < 0.01, "400px back halves: {halved}");
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
