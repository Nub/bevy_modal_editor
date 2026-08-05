//! Flow check (PREFAB_PROBE=1): drive the REAL prefab-insert flow with
//! injected key events — menu → editor → `i` → type prefab name → Enter — then
//! VERDICT on what the user would actually see (placed, selected, meshed,
//! visible, near the root, back in normal mode). Exits nonzero on failure, so
//! this runs as a UX smoke check, not just a diagnostic.

use bevy::prelude::*;
use editor_core::prelude::*;
use editor_prefabs::PrefabInstance;
use editor_scene::PrefabStamped;

fn key(
    key_events: &mut MessageWriter<bevy::input::keyboard::KeyboardInput>,
    window: Entity,
    key_code: KeyCode,
    logical: bevy::input::keyboard::Key,
    ch: Option<&str>,
    pressed: bool,
) {
    key_events.write(bevy::input::keyboard::KeyboardInput {
        key_code,
        logical_key: logical,
        state: if pressed {
            bevy::input::ButtonState::Pressed
        } else {
            bevy::input::ButtonState::Released
        },
        text: (pressed)
            .then(|| ch.unwrap_or_default().into())
            .filter(|_| ch.is_some()),
        repeat: false,
        window,
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn probe_prefab(
    mut frames: Local<u32>,
    mut key_events: MessageWriter<bevy::input::keyboard::KeyboardInput>,
    mut writer: MessageWriter<ActionInvoked>,
    window: Query<Entity, With<bevy::window::PrimaryWindow>>,
    state: Res<EditorState>,
    mode: Res<CurrentMode>,
    palette: Res<crate::palette::PaletteState>,
    roots: Query<
        (
            Entity,
            Option<&Name>,
            &Transform,
            Option<&Children>,
            Has<Selected>,
        ),
        With<PrefabInstance>,
    >,
    stamped: Query<
        (
            Entity,
            Option<&Name>,
            Has<Mesh3d>,
            &GlobalTransform,
            Option<&ViewVisibility>,
            Option<&InheritedVisibility>,
        ),
        With<PrefabStamped>,
    >,
    preview_subject: Res<crate::palette_preview::PreviewSubject>,
    preview_content: Query<
        (Has<Mesh3d>, Option<&bevy::camera::visibility::RenderLayers>),
        With<crate::palette_preview::PreviewContent>,
    >,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(window) = window.single() else { return };
    *frames += 1;
    use bevy::input::keyboard::Key;
    match *frames {
        // Leave the main menu.
        60 => key(
            &mut key_events,
            window,
            KeyCode::Enter,
            Key::Enter,
            None,
            true,
        ),
        62 => key(
            &mut key_events,
            window,
            KeyCode::Enter,
            Key::Enter,
            None,
            false,
        ),
        90 => {
            writer.write(ActionInvoked {
                action: ActionId::new_static("core.toggle-editor"),
                args: None,
                source: InvocationSource::Test,
            });
        }
        120 => {
            info!("PROBE editor active={} mode={:?}", state.active, mode.0);
            key(
                &mut key_events,
                window,
                KeyCode::KeyI,
                Key::Character("i".into()),
                Some("i"),
                true,
            );
        }
        122 => key(
            &mut key_events,
            window,
            KeyCode::KeyI,
            Key::Character("i".into()),
            Some("i"),
            false,
        ),
        150 => {
            info!(
                "PROBE after i: palette open={} mode={:?}",
                palette.open, mode.0
            );
            key(
                &mut key_events,
                window,
                KeyCode::KeyC,
                Key::Character("c".into()),
                Some("c"),
                true,
            );
        }
        152 => key(
            &mut key_events,
            window,
            KeyCode::KeyC,
            Key::Character("c".into()),
            Some("c"),
            false,
        ),
        154 => key(
            &mut key_events,
            window,
            KeyCode::KeyU,
            Key::Character("u".into()),
            Some("u"),
            true,
        ),
        156 => key(
            &mut key_events,
            window,
            KeyCode::KeyU,
            Key::Character("u".into()),
            Some("u"),
            false,
        ),
        158 => key(
            &mut key_events,
            window,
            KeyCode::KeyB,
            Key::Character("b".into()),
            Some("b"),
            true,
        ),
        160 => key(
            &mut key_events,
            window,
            KeyCode::KeyB,
            Key::Character("b".into()),
            Some("b"),
            false,
        ),
        185 => {
            // Preview must be LIVE while a placeable row is highlighted.
            let has_subject = preview_subject.0.is_some();
            let meshed = preview_content.iter().filter(|(m, _)| *m).count();
            let layered = preview_content.iter().filter(|(_, l)| l.is_some()).count();
            info!(
                "PROBE preview: subject={} content={} meshed={} layered={}",
                has_subject,
                preview_content.iter().count(),
                meshed,
                layered
            );
            if !has_subject || meshed == 0 {
                error!("PROBE FAIL: palette preview not live for highlighted item");
                exit.write(AppExit::error());
            }
        }
        190 => {
            info!("PROBE query typed, palette open={}", palette.open);
            key(
                &mut key_events,
                window,
                KeyCode::Enter,
                Key::Enter,
                None,
                true,
            );
        }
        192 => key(
            &mut key_events,
            window,
            KeyCode::Enter,
            Key::Enter,
            None,
            false,
        ),
        260 => {
            let mut failures: Vec<String> = Vec::new();
            let mut check = |ok: bool, what: &str| {
                if !ok {
                    failures.push(what.to_string());
                }
            };
            check(!palette.open, "palette closed after Enter");
            check(mode.0 == MODE_NORMAL, "back in normal mode after placement");
            check(
                roots.iter().count() == 1,
                "exactly one instance root placed",
            );
            let mut root_at = Vec3::ZERO;
            for (entity, name, transform, children, selected) in roots.iter() {
                root_at = transform.translation;
                info!(
                    "PROBE root {entity:?} name={:?} at {:?} children={} selected={selected}",
                    name.map(|n| n.as_str()),
                    transform.translation,
                    children.map(|c| c.len()).unwrap_or(0)
                );
                check(selected, "placed instance is selected");
                check(
                    name.is_some_and(|n| n.as_str() != "Prefab Instance"),
                    "root named after the prefab",
                );
                check(
                    children.is_some_and(|c| !c.is_empty()),
                    "template stamped under the root",
                );
            }
            for (entity, name, has_mesh, global, view, inherited) in stamped.iter() {
                info!(
                    "PROBE stamped {entity:?} name={:?} mesh={} world={:?} view_vis={:?} inherited_vis={:?}",
                    name.map(|n| n.as_str()),
                    has_mesh,
                    global.translation(),
                    view.map(|v| v.get()),
                    inherited.map(|v| v.get()),
                );
                check(has_mesh, "stamped entity has a mesh (regenerate fired)");
                check(
                    view.is_some_and(|v| v.get()),
                    "stamped entity passes visibility",
                );
                check(
                    global.translation().distance(root_at) < 5.0,
                    "stamped entity stays near its root (template centered)",
                );
            }
            if failures.is_empty() {
                info!("PROBE PASS: prefab insert flow");
                exit.write(AppExit::Success);
            } else {
                for f in &failures {
                    error!("PROBE FAIL: {f}");
                }
                exit.write(AppExit::error());
            }
        }
        _ => {}
    }
}
