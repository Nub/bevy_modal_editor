//! End-user session probe (USER_PROBE=1): plays a full editing session in the
//! REAL binary with REAL input — keystrokes, cursor ovement, clicks — and
//! screenshots every checkpoint to `target/user-probe/` for visual review.
//! Every stage asserts the outcome a user would expect to SEE; exits nonzero
//! on the first broken expectation.
//!
//! Journey: boot → editor → insert a sphere by click-placement → move it with
//! the x-constrained typed gesture → select all → group into a prefab →
//! open the instance → insert inside → close (propagates) → place a second
//! instance → make a variant → undo/redo storm → play → reset.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{CursorMoved, PrimaryWindow};
use editor_core::prelude::*;
use editor_prefabs::{PrefabInstance, PrefabLibrary, open_mode::OpenInstance};
use editor_scene::PrefabStamped;

pub(crate) const SHOT_DIR: &str = "target/user-probe";

#[derive(Resource, Default)]
pub(crate) struct UserProbe {
    frame: u32,
    failures: Vec<String>,
    /// Sphere translation captured before the move gesture.
    move_start_x: Option<f32>,
    /// Library generation before the open→close edit.
    generation_before_close: u64,
    /// Viewport camera distance before the wheel test.
    camera_before: Option<Vec3>,
}

pub(crate) fn key(
    world: &mut World,
    code: KeyCode,
    logical: Key,
    text: Option<&str>,
    pressed: bool,
) {
    let Some(window) = window_of(world) else {
        return;
    };
    world.write_message(KeyboardInput {
        key_code: code,
        logical_key: logical,
        state: if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        text: pressed.then(|| text.map(|t| t.into())).flatten(),
        repeat: false,
        window,
    });
}

/// One full press+release of a character key (text drives focused inputs).
pub(crate) fn tap(world: &mut World, code: KeyCode, ch: &str) {
    key(world, code, Key::Character(ch.into()), Some(ch), true);
    key(world, code, Key::Character(ch.into()), Some(ch), false);
}

pub(crate) fn tap_named(world: &mut World, code: KeyCode, logical: Key) {
    key(world, code, logical.clone(), None, true);
    key(world, code, logical, None, false);
}

pub(crate) fn window_of(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
}

pub(crate) fn move_cursor(world: &mut World, position: Vec2) {
    let Some(window) = window_of(world) else {
        return;
    };
    if let Some(mut win) = world.get_mut::<Window>(window) {
        win.set_cursor_position(Some(position));
    }
    world.write_message(CursorMoved {
        window,
        position,
        delta: None,
    });
    // Deterministic pointer state: winit only feeds PointerLocation from REAL
    // mouse events, so probe runs flapped with wherever the OS cursor sat.
    // Write the location directly — the same state real input produces.
    if let Some(target) =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Primary).normalize(Some(window))
    {
        let mut pointers = world.query::<(
            &bevy::picking::pointer::PointerId,
            &mut bevy::picking::pointer::PointerLocation,
        )>();
        for (id, mut location) in pointers.iter_mut(world) {
            if *id == bevy::picking::pointer::PointerId::Mouse {
                location.location = Some(bevy::picking::pointer::Location {
                    target: target.clone(),
                    position,
                });
            }
        }
    }
}

pub(crate) fn click(world: &mut World, pressed: bool) {
    let Some(window) = window_of(world) else {
        return;
    };
    world.write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        window,
    });
    // ALSO write the picking-side event. Real input reaches bevy_picking via
    // the WINDOW's cursor state, which synthetic input can't populate — so
    // without this no Pointer<Press> ever triggers and every chrome click
    // observer (chips, rows) stays dead in probes.
    let location = {
        let mut pointers = world.query::<(
            &bevy::picking::pointer::PointerId,
            &bevy::picking::pointer::PointerLocation,
        )>();
        pointers
            .iter(world)
            .find(|(id, _)| **id == bevy::picking::pointer::PointerId::Mouse)
            .and_then(|(_, location)| location.location.clone())
    };
    if let Some(location) = location {
        world.write_message(bevy::picking::pointer::PointerInput {
            pointer_id: bevy::picking::pointer::PointerId::Mouse,
            location,
            action: if pressed {
                bevy::picking::pointer::PointerAction::Press(
                    bevy::picking::pointer::PointerButton::Primary,
                )
            } else {
                bevy::picking::pointer::PointerAction::Release(
                    bevy::picking::pointer::PointerButton::Primary,
                )
            },
        });
    }
}

pub(crate) fn viewport_center(world: &mut World) -> Vec2 {
    let size = window_of(world)
        .and_then(|w| world.get::<Window>(w))
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    // Between the docks, low in the frame: CLEAR ground close to the camera
    // (center-screen at this camera pose is occluded by the graybox boxes).
    Vec2::new(size.x * 0.42, size.y * 0.78)
}

pub(crate) fn shot(world: &mut World, name: &str) {
    // macOS stops compositing an occluded window, so a screenshot taken while
    // the terminal is in front reads back as a uniform black frame — which is
    // indistinguishable from "the editor drew nothing" and has twice left UI
    // work verified only structurally. Raising the window for the capture is
    // the difference between a screenshot and a black rectangle.
    raise_window(world);
    let path = format!("{SHOT_DIR}/{name}.png");
    world
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// Capture an OFFSCREEN render target rather than the window.
///
/// A window screenshot is only as good as the compositor's willingness to draw
/// the window, and on macOS an occluded one reads back as a uniform black
/// frame — 46 of this repo's 48 captured frames are exactly that. A
/// render-to-texture target has no such problem: it is drawn because something
/// asked for it, not because someone is looking at it. Anything that needs to
/// be SEEN to be verified should render to an image and be captured here.
pub(crate) fn shot_image(world: &mut World, image: Handle<Image>, name: &str) {
    let path = format!("{SHOT_DIR}/{name}.png");
    world
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path));
}

/// Put the window in front so the compositor keeps drawing it. Probe-only: it
/// runs in a run the probe already owns the machine for.
pub(crate) fn raise_window(world: &mut World) {
    let windows: Vec<Entity> = world
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .collect();
    for entity in windows {
        if let Some(mut window) = world.get_mut::<Window>(entity) {
            if window.window_level != bevy::window::WindowLevel::AlwaysOnTop {
                window.window_level = bevy::window::WindowLevel::AlwaysOnTop;
            }
            window.focused = true;
        }
    }
}

pub(crate) fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("USER-PROBE PASS: {what}");
    } else {
        error!("USER-PROBE FAIL: {what}");
        world
            .resource_mut::<UserProbe>()
            .failures
            .push(what.to_string());
    }
}

fn spheres(world: &mut World) -> Vec<Entity> {
    // Sphere kind stamps a Mesh3d via the game's regenerate observer; count
    // scene entities named Sphere (kind spawns carry the kind name).
    world
        .query::<(Entity, &Name, &SceneId)>()
        .iter(world)
        .filter(|(_, n, _)| n.as_str().starts_with("Sphere"))
        .map(|(e, _, _)| e)
        .collect()
}

fn instance_roots(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, (With<PrefabInstance>, Without<PrefabStamped>)>()
        .iter(world)
        .collect()
}

/// Where the VIEWPORT camera is — the one drawing to the window, never the
/// preview rigs that render to textures.
fn viewport_camera_position(world: &mut World) -> Option<Vec3> {
    world
        .query::<(&Camera, &Transform, Option<&bevy::camera::RenderTarget>)>()
        .iter(world)
        .find(|(camera, _, target)| editor_core::camera::is_viewport_camera(camera, *target))
        .map(|(_, transform, _)| transform.translation)
}
pub(crate) fn probe_user(world: &mut World) {
    world.resource_mut::<UserProbe>().frame += 1;
    let frame = world.resource::<UserProbe>().frame;
    if frame == 1 {
        let _ = std::fs::create_dir_all(SHOT_DIR);
        // Clean slate (owner rule): probe-owned prefabs from prior runs must
        // never leak into this session's library or palettes.
        for stale in ["kit", "red"] {
            let dir = editor_prefabs::authoring::prefabs_dir();
            let _ = std::fs::remove_file(dir.join(format!("{stale}.prefab.ron")));
            let _ = std::fs::remove_file(dir.join(format!("{stale}.prefab.ron.bak")));
            let mut library = world.resource_mut::<PrefabLibrary>();
            let ids: Vec<_> = library
                .prefabs
                .iter()
                .filter(|(_, def)| def.name.eq_ignore_ascii_case(stale))
                .map(|(id, _)| *id)
                .collect();
            for id in ids {
                library.prefabs.remove(&id);
            }
        }
        info!("USER-PROBE armed — driving the session");
    }
    match frame {
        // ── Boot: menu → game → editor ─────────────────────────────────────
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => tap_named(world, KeyCode::F12, Key::F12),
        180 => {
            let active = world.resource::<EditorState>().active;
            check(world, active, "F12 switches into the editor");
            let grid_visible = world
                .query_filtered::<&Visibility, With<crate::grid::EditorGrid>>()
                .iter(world)
                .any(|v| *v == Visibility::Visible);
            check(world, grid_visible, "ground grid is visible in the editor");
            shot(world, "01-editor");
        }
        // ── Insert a sphere: i → type → Enter picks kind → ghost → click ──
        200 => tap(world, KeyCode::KeyI, "i"),
        240 => tap(world, KeyCode::KeyS, "s"),
        244 => tap(world, KeyCode::KeyP, "p"),
        248 => tap(world, KeyCode::KeyH, "h"),
        280 => shot(world, "02-insert-palette"),
        290 => tap_named(world, KeyCode::Enter, Key::Enter),
        320 => {
            let center = viewport_center(world);
            move_cursor(world, center);
        }
        360 => {
            let ghost = world
                .query_filtered::<Entity, With<editor_api::prelude::InsertPreview>>()
                .iter(world)
                .count();
            check(
                world,
                ghost > 0,
                "insert ghost previews the sphere at the cursor",
            );
            shot(world, "03-ghost");
        }
        370 => click(world, true),
        372 => click(world, false),
        420 => {
            // Diagnostics: where did placement actually land, and from which camera?
            let cursor = world.resource::<CursorGround>().0;
            let cameras: Vec<(bool, Vec3)> = world
                .query::<(&bevy::camera::Camera, &GlobalTransform)>()
                .iter(world)
                .map(|(c, t)| (c.is_active, t.translation()))
                .collect();
            let sphere_at: Vec<Vec3> = spheres(world)
                .iter()
                .filter_map(|e| world.get::<Transform>(*e).map(|t| t.translation))
                .collect();
            info!(
                "USER-PROBE diag: cursor_ground={cursor:?} spheres={sphere_at:?} cameras={cameras:?}"
            );
            let placed = spheres(world);
            check(world, placed.len() == 1, "click places exactly one sphere");
            let selected = placed
                .first()
                .is_some_and(|e| world.get::<Selected>(*e).is_some());
            check(
                world,
                selected,
                "the placed sphere is selected (user sees what they made)",
            );
            let mode = world.resource::<CurrentMode>().0.clone();
            check(
                world,
                mode == MODE_NORMAL,
                "placement returns to normal mode (single place, no shift)",
            );
            let start_x = placed
                .first()
                .and_then(|e| world.get::<Transform>(*e))
                .map(|t| t.translation.x);
            world.resource_mut::<UserProbe>().move_start_x = start_x;
        }
        // ── Move it: w (move) → x constrain → type 2 → Enter ──────────────
        440 => tap(world, KeyCode::KeyW, "w"),
        460 => tap(world, KeyCode::KeyX, "x"),
        // Drag right like a mouse hand would (typed exact amounts: spec B7
        // promise, currently unimplemented — separate finding).
        462..=496 if frame.is_multiple_of(2) => {
            let current = window_of(world)
                .and_then(|w| world.get::<Window>(w))
                .and_then(|w| w.cursor_position())
                .unwrap_or(Vec2::new(600.0, 500.0));
            move_cursor(world, current + Vec2::new(12.0, 0.0));
        }
        500 => shot(world, "04-move-gesture"),
        510 => tap_named(world, KeyCode::Enter, Key::Enter),
        550 => {
            let start = world.resource::<UserProbe>().move_start_x;
            let now = spheres(world)
                .first()
                .and_then(|e| world.get::<Transform>(*e))
                .map(|t| t.translation.x);
            let moved = match (start, now) {
                (Some(a), Some(b)) => (b - a).abs() > 0.3,
                _ => false,
            };
            check(world, moved, "x-constrained drag actually moved the sphere");
        }
        // ── zz frames the selection ────────────────────────────────────────
        552 => tap(world, KeyCode::KeyZ, "z"),
        554 => tap(world, KeyCode::KeyZ, "z"),
        562 => {
            let camera_at = world
                .query::<(&bevy::camera::Camera, &Transform)>()
                .iter(world)
                .find(|(c, _)| c.is_active && c.order >= 0)
                .map(|(_, t)| t.translation);
            let sphere_at = spheres(world)
                .first()
                .and_then(|e| world.get::<Transform>(*e))
                .map(|t| t.translation);
            let framed = match (camera_at, sphere_at) {
                (Some(cam), Some(sphere)) => cam.distance(sphere) < 12.0,
                _ => false,
            };
            check(
                world,
                framed,
                "zz frames the selection (camera moved close)",
            );
            shot(world, "04b-framed");
        }
        // …and so does `5`, the number-key habit every DCC has. Fly well away
        // first, or a camera already framed would satisfy this without the key
        // doing anything at all.
        563 => {
            let cameras: Vec<Entity> = world
                .query::<(Entity, &bevy::camera::Camera)>()
                .iter(world)
                .filter(|(_, c)| c.is_active && c.order >= 0)
                .map(|(e, _)| e)
                .collect();
            for camera in cameras {
                if let Some(mut transform) = world.get_mut::<Transform>(camera) {
                    *transform =
                        Transform::from_xyz(90.0, 60.0, 90.0).looking_at(Vec3::ZERO, Vec3::Y);
                }
            }
        }
        565 => tap(world, KeyCode::Digit5, "5"),
        580 => {
            let camera_at = world
                .query::<(&bevy::camera::Camera, &Transform)>()
                .iter(world)
                .find(|(c, _)| c.is_active && c.order >= 0)
                .map(|(_, t)| t.translation);
            let sphere_at = spheres(world)
                .first()
                .and_then(|e| world.get::<Transform>(*e))
                .map(|t| t.translation);
            let framed = match (camera_at, sphere_at) {
                (Some(cam), Some(sphere)) => cam.distance(sphere) < 12.0,
                _ => false,
            };
            check(world, framed, "5 centres the selection too");
        }
        // ── Group everything into a prefab ─────────────────────────────────
        // Hold ctrl ACROSS frames — press+release in one frame never registers
        // as a held modifier (ButtonInput processes the whole batch at once).
        566 => key(world, KeyCode::ControlLeft, Key::Control, None, true),
        570 => tap(world, KeyCode::KeyA, "a"),
        576 => key(world, KeyCode::ControlLeft, Key::Control, None, false),
        600 => tap(world, KeyCode::KeyG, "g"),
        630 => {
            let open = world
                .resource::<editor_prefabs::authoring::GroupPrompt>()
                .open;
            check(world, open, "g opens the name prompt over a selection");
            shot(world, "05-group-prompt");
        }
        640 => tap(world, KeyCode::KeyK, "k"),
        644 => tap(world, KeyCode::KeyI, "i"),
        648 => tap(world, KeyCode::KeyT, "t"),
        660 => tap_named(world, KeyCode::Enter, Key::Enter),
        720 => {
            let roots = instance_roots(world);
            check(
                world,
                roots.len() == 1,
                "grouping replaced the selection with ONE instance",
            );
            let selected = roots
                .first()
                .is_some_and(|e| world.get::<Selected>(*e).is_some());
            check(world, selected, "the new instance is selected");
            shot(world, "06-grouped");
        }
        // ── Open it, add a sphere inside, close ────────────────────────────
        740 => {
            let generation = world.resource::<PrefabLibrary>().generation;
            world.resource_mut::<UserProbe>().generation_before_close = generation;
            tap_named(world, KeyCode::Enter, Key::Enter);
        }
        780 => {
            let open = world.resource::<OpenInstance>().0.is_some();
            check(world, open, "Enter on the instance opens it in place");
            shot(world, "07-open-instance");
        }
        // Esc first: i means ADD COMPONENT while holding a selection (owner),
        // so placing inside starts with empty hands.
        792 => {
            let count = world
                .query_filtered::<(), With<Selected>>()
                .iter(world)
                .count();
            let esc_cap = world
                .resource::<editor_core::resolver::EscapeFromCapture>()
                .0;
            let capture = world.resource::<editor_core::resolver::KeyCapture>().0;
            info!("USER-PROBE diag pre-esc: selection={count} esc_cap={esc_cap} capture={capture}");
        }
        794 => tap_named(world, KeyCode::Escape, Key::Escape),
        798 => {
            let count = world
                .query_filtered::<(), With<Selected>>()
                .iter(world)
                .count();
            info!("USER-PROBE diag pre-i: selection={count}");
        }
        800 => tap(world, KeyCode::KeyI, "i"),
        830 => {
            let state = world.resource::<crate::palette::PaletteState>();
            info!(
                "USER-PROBE diag post-i: filter={:?} open={}",
                state.filter, state.open
            );
        }
        840 => tap(world, KeyCode::KeyS, "s"),
        844 => tap(world, KeyCode::KeyP, "p"),
        848 => tap(world, KeyCode::KeyH, "h"),
        880 => tap_named(world, KeyCode::Enter, Key::Enter),
        890 => {
            let center = viewport_center(world);
            move_cursor(world, center + Vec2::new(140.0, 0.0));
        }
        918 => {
            let mode = world.resource::<CurrentMode>().0.clone();
            let capture = world.resource::<editor_core::resolver::KeyCapture>().0;
            let kind = world.resource::<InsertState>().kind.clone();
            let cursor = world.resource::<CursorGround>().0;
            let over_chrome = world
                .resource::<editor_core::resolver::PointerOverChrome>()
                .0;
            info!(
                "USER-PROBE diag inside-click: mode={mode:?} capture={capture} kind={kind:?} cursor={cursor:?} over_chrome={over_chrome}"
            );
        }
        920 => click(world, true),
        922 => click(world, false),
        970 => {
            let all = spheres(world);
            let adopted = all.len() == 2 && all.iter().all(|e| world.get::<ChildOf>(*e).is_some());
            check(world, adopted, "sphere placed while open joins the group");
            let mode = world.resource::<CurrentMode>().0.clone();
            check(
                world,
                mode == MODE_NORMAL,
                "back to normal after placing inside",
            );
        }
        988 => {
            let mode = world.resource::<CurrentMode>().0.clone();
            let panel = world.resource::<PanelFocus>().0.clone();
            let capture = world.resource::<editor_core::resolver::KeyCapture>().0;
            let esc_cap = world
                .resource::<editor_core::resolver::EscapeFromCapture>()
                .0;
            info!(
                "USER-PROBE diag esc-gates: mode={mode:?} panel_focus={panel:?} capture={capture} escape_from_capture={esc_cap}"
            );
        }
        // Two escapes: first clears the placed sphere's selection, second
        // closes the open instance (one layer per press).
        990 => tap_named(world, KeyCode::Escape, Key::Escape),
        1010 => tap_named(world, KeyCode::Escape, Key::Escape),
        1050 => {
            let closed = world.resource::<OpenInstance>().0.is_none();
            check(world, closed, "Esc closes the open instance");
            let before = world.resource::<UserProbe>().generation_before_close;
            let after = world.resource::<PrefabLibrary>().generation;
            check(
                world,
                after > before,
                "closing saved the template (library generation moved)",
            );
            shot(world, "08-closed");
        }
        // ── Second instance: propagation visible ───────────────────────────
        1064 => tap_named(world, KeyCode::Escape, Key::Escape),
        1070 => tap(world, KeyCode::KeyI, "i"),
        1110 => tap(world, KeyCode::KeyK, "k"),
        1114 => tap(world, KeyCode::KeyI, "i"),
        1118 => tap(world, KeyCode::KeyT, "t"),
        1150 => tap_named(world, KeyCode::Enter, Key::Enter),
        1220 => {
            let roots = instance_roots(world);
            check(
                world,
                roots.len() == 2,
                "a second Kit instance placed from the palette",
            );
            let child_counts: Vec<usize> = roots
                .iter()
                .map(|e| world.get::<Children>(*e).map(|c| c.len()).unwrap_or(0))
                .collect();
            let matching = child_counts.len() == 2 && child_counts[0] == child_counts[1];
            check(
                world,
                matching,
                "both instances show the SAME structure (template propagated)",
            );
            shot(world, "09-two-instances");
        }
        // ── Variant via the command palette ────────────────────────────────
        1236 => key(world, KeyCode::ShiftLeft, Key::Shift, None, true),
        1240 => tap(world, KeyCode::Semicolon, ":"),
        1246 => key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        1280 => {
            for (i, (code, ch)) in [
                (KeyCode::KeyV, "v"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyR, "r"),
                (KeyCode::KeyI, "i"),
            ]
            .into_iter()
            .enumerate()
            {
                let _ = i;
                tap(world, code, ch);
            }
        }
        1320 => shot(world, "10-command-palette"),
        1330 => tap_named(world, KeyCode::Enter, Key::Enter),
        1370 => {
            let open = world
                .resource::<editor_prefabs::authoring::GroupPrompt>()
                .open;
            check(
                world,
                open,
                "Make Prefab Variant (palette) opens the variant name prompt",
            );
        }
        1380 => {
            for (code, ch) in [
                (KeyCode::KeyR, "r"),
                (KeyCode::KeyE, "e"),
                (KeyCode::KeyD, "d"),
            ] {
                tap(world, code, ch);
            }
        }
        1400 => tap_named(world, KeyCode::Enter, Key::Enter),
        1470 => {
            let has_variant = world
                .resource::<PrefabLibrary>()
                .prefabs
                .values()
                .any(|p| p.name == "red");
            check(
                world,
                has_variant,
                "variant saved to the library under the typed name",
            );
            shot(world, "11-variant");
        }
        // ── Undo/redo storm ────────────────────────────────────────────────
        1490..=1540 if frame % 6 == 2 => tap(world, KeyCode::KeyU, "u"),
        1596 => key(world, KeyCode::ControlLeft, Key::Control, None, true),
        1600..=1650 if frame % 6 == 2 => tap(world, KeyCode::KeyR, "r"),
        1656 => key(world, KeyCode::ControlLeft, Key::Control, None, false),
        1720 => {
            // Close-and-save is deliberately outside undo (asset edit, not a
            // scene transaction) so exact round-trip isn't defined — the bar
            // here is: nothing crashed, the editor is alive, instances exist.
            let roots = instance_roots(world).len();
            let alive = world.resource::<EditorState>().active;
            check(
                world,
                alive && roots >= 1,
                "undo/redo storm leaves a coherent, live editor",
            );
            shot(world, "12-after-undo-redo");
        }
        // ── Play / reset ───────────────────────────────────────────────────
        1740 => tap_named(world, KeyCode::F5, Key::F5),
        1800 => {
            let playing = !world.resource::<EditorState>().active;
            check(world, playing, "F5 hands the world to the game");
            shot(world, "13-playing");
        }
        1820 => tap_named(world, KeyCode::F7, Key::F7),
        1880 => {
            let active = world.resource::<EditorState>().active;
            check(world, active, "F7 resets back to the editor");
            shot(world, "14-reset");
        }
        // ── i with selection = ADD COMPONENT; / = component search ─────────
        1890 => key(world, KeyCode::ControlLeft, Key::Control, None, true),
        1893 => tap(world, KeyCode::KeyA, "a"),
        1897 => key(world, KeyCode::ControlLeft, Key::Control, None, false),
        1900 => tap(world, KeyCode::KeyI, "i"),
        1930 => {
            let filter_ok = world.resource::<crate::palette::PaletteState>().filter
                == crate::palette::PaletteFilter::AddComponent;
            check(
                world,
                filter_ok,
                "i with a selection opens the ADD COMPONENT palette",
            );
            for (code, ch) in [
                (KeyCode::KeyS, "s"),
                (KeyCode::KeyP, "p"),
                (KeyCode::KeyI, "i"),
                (KeyCode::KeyN, "n"),
            ] {
                tap(world, code, ch);
            }
        }
        1948 => {
            let state = world.resource::<crate::palette::PaletteState>();
            let capture = world.resource::<editor_core::resolver::KeyCapture>().0;
            let focus = world
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                .is_some();
            info!(
                "USER-PROBE diag pre-enter: palette_open={} filter={:?} query={:?} capture={capture} focus={focus}",
                state.open, state.filter, state.query
            );
        }
        1944 => shot(world, "16b-add-component-open"),
        1950 => tap_named(world, KeyCode::Enter, Key::Enter),
        1990 => {
            let flash = world
                .resource::<crate::statusbar::StatusFlash>()
                .text
                .clone();
            check(
                world,
                flash.contains("Spinner added"),
                "component added to the selection with feedback",
            );
            shot(world, "15-add-component");
        }
        2000 => tap(world, KeyCode::Slash, "/"),
        2030 => {
            let state = world.resource::<crate::palette::PaletteState>();
            let filter_ok = state.filter == crate::palette::PaletteFilter::ComponentSearch;
            check(
                world,
                filter_ok,
                "/ with a selection searches components ON it",
            );
            shot(world, "16-component-search");
        }
        2040 => tap_named(world, KeyCode::Escape, Key::Escape),
        // Hold the leader so which-key opens; screenshot it for design review.
        2050 => key(world, KeyCode::Space, Key::Space, Some(" "), true),
        2085 => shot(world, "17-which-key"),
        2090 => key(world, KeyCode::Space, Key::Space, Some(" "), false),
        2095 => tap_named(world, KeyCode::Escape, Key::Escape),
        // ── Verdict ────────────────────────────────────────────────────────
        // ── The wheel zooms (owner, testing live: "scrolling should zoom") ──
        // Nothing handled the wheel at all — getting closer to a piece meant
        // holding the right button and flying there.
        // Pointer over the VIEWPORT: the wheel belongs to whatever it is over,
        // and the previous frames left it on a panel.
        // ── The palette teaches (owner: "hard to learn from as a newb") ────
        // The first screen used to be one alphabetical bucket cut at row 50,
        // so every socket verb was off the page with nothing saying so.
        2106 => {
            world.write_message(ActionInvoked {
                action: ActionId::new_static("core.palette"),
                args: None,
                source: InvocationSource::Test,
            });
        }
        2110 => {
            let rows: Vec<String> = world
                .query::<&Text>()
                .iter(world)
                .map(|text| text.0.clone())
                .collect();
            let headers = [
                "PLACE",
                "SOCKETS & KITS",
                "SELECT & EDIT",
                "VIEW & PANELS",
                "SCENE & SESSION",
            ];
            let present = headers
                .iter()
                .filter(|header| rows.iter().any(|row| row == *header))
                .count();
            check(
                world,
                present == 5,
                &format!("the palette opens in SECTIONS ({present}/5 headers)"),
            );
            // The verbs the owner could not find: on the page, unsearched.
            let teaches_sockets = rows.iter().any(|row| row.starts_with("Sockets:"));
            check(
                world,
                teaches_sockets,
                "and a socket verb is on the first screen without typing",
            );
            // Nothing may be cut in SILENCE. Either the whole list is on the
            // page, or a row says how much was held back — the invisible cut is
            // what hid every socket verb from a newcomer.
            let actions = world.resource::<ActionCatalog>().actions.len();
            let listed = rows.len();
            let announced = rows
                .iter()
                .any(|row| row.contains("more") && row.contains("keep typing"));
            check(
                world,
                announced || listed >= actions / 2,
                &format!("nothing is cut in silence ({listed} rows, {actions} actions)"),
            );
            shot(world, "18-palette-sections");
        }
        2114 => tap_named(world, KeyCode::Escape, Key::Escape),
        2112 => {
            let center = viewport_center(world);
            move_cursor(world, center);
        }
        2116 => {
            let at = viewport_camera_position(world);
            world.resource_mut::<UserProbe>().camera_before = at;
            check(world, at.is_some(), "the viewport camera is findable");
            let window = world
                .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
                .iter(world)
                .next()
                .unwrap_or(Entity::PLACEHOLDER);
            world.write_message(bevy::input::mouse::MouseWheel {
                unit: bevy::input::mouse::MouseScrollUnit::Line,
                x: 0.0,
                y: 3.0,
                window,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
        }
        2118 => {
            let before = world.resource::<UserProbe>().camera_before;
            let after = viewport_camera_position(world);
            let moved = match (before, after) {
                (Some(before), Some(after)) => before.distance(after),
                _ => 0.0,
            };
            check(
                world,
                moved > 1.0,
                &format!("scrolling moved the viewport camera ({moved:.2}m)"),
            );
        }
        2140 => {
            let failures = world.resource::<UserProbe>().failures.clone();
            if failures.is_empty() {
                info!("USER-PROBE PASS: full session ({SHOT_DIR}/*.png for visual review)");
                world.write_message(AppExit::Success);
            } else {
                for f in &failures {
                    error!("USER-PROBE FAILED: {f}");
                }
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}

/// Probe-only: every action that fires, in order — the ground truth for "did
/// that keypress become the action I think it did".
pub(crate) fn log_actions(mut reader: MessageReader<ActionInvoked>) {
    for invoked in reader.read() {
        info!("USER-PROBE action: {}", invoked.action.as_str());
    }
}
