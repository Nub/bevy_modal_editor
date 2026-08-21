//! The single input resolver (spec §4, RFC §4): the ONLY reader of raw keyboard input
//! in the entire editor. Tracks multi-chord sequences, resolves through the active
//! context layers (mode first, then global), and emits `ActionInvoked`.
//! Which-key data (A8) is derived from the same table — no hand-maintained hints.

use crate::keymap_data::ResolvedKeymapData;
use crate::modes::{CurrentMode, MODE_NORMAL, ModeChanged, Modes, set_mode};
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use editor_api::feature::ValidatedFeatures;
use editor_api::keymap::{Binding, Chord, Modifiers};
use editor_api::prelude::*;
use std::collections::HashMap;

/// Re-exported dispatch table type (built in `keymap_data`).
pub type ResolvedKeymap = ResolvedKeymapData;

/// Whether the editor overlay is active. When inactive, only the global context
/// resolves (so the toggle binding still works) and the game owns input.
#[derive(Resource, Default)]
pub struct EditorState {
    pub active: bool,
}

/// The in-flight chord sequence (multi-key bindings like `g g`).
#[derive(Resource, Default)]
pub struct PendingKeys(pub Vec<Chord>);

/// When true, a text field (palette, rename box) owns the keyboard and the resolver
/// stands down entirely. UI shell code sets/clears this.
#[derive(Resource, Default)]
pub struct KeyCapture(pub bool);

/// True while the pointer is over ANY editor chrome (docks, statusbar, palette) —
/// written by `editor_ui` each frame, read by viewport tools so ghosts, placement,
/// and cursor rays never fire through a panel (flow-audit: input falling through
/// overlapping surfaces).
#[derive(Resource, Default)]
pub struct PointerOverChrome(pub bool);

/// An active overlay keymap layer (gesture, focused panel) — highest priority when set.
#[derive(Resource, Default)]
pub struct OverlayContext {
    pub context: Option<ContextId>,
    /// Exclusive layers GRAB the keyboard: a gesture rebinds everything while it
    /// runs, and a stray `u` mid-drag must not undo. A LAYERED one wins the keys
    /// it declares and lets the rest fall through to the mode — which is what a
    /// working layer like socket mode wants, so that arming a socket does not
    /// also take away move, undo and the palette.
    pub exclusive: bool,
}

impl OverlayContext {
    pub fn set_exclusive(&mut self, context: ContextId) {
        self.context = Some(context);
        self.exclusive = true;
    }
    pub fn set_layer(&mut self, context: ContextId) {
        self.context = Some(context);
        self.exclusive = false;
    }
    pub fn clear(&mut self) {
        self.context = None;
        self.exclusive = false;
    }
}

/// Set for the frame when Escape pierced a text-field capture: backout peels ONE
/// layer per press — the capturing surface closes, but mode/selection stay.
#[derive(Resource, Default)]
pub struct EscapeFromCapture(pub bool);

/// Emitted when a key sequence resolves to nothing — every keypress deserves feedback
/// (design bar, spec §7): the shell shows "unbound" instead of silence.
#[derive(Message, Debug)]
pub struct KeysUnresolved(pub Vec<Chord>);

/// Palette/cheat-sheet data: every registered action (A8's sibling — derived, never
/// hand-maintained).
#[derive(Resource)]
pub struct ActionCatalog {
    pub actions: Vec<ActionDef>,
}

impl ActionCatalog {
    pub fn from_validated(validated: &ValidatedFeatures) -> Self {
        Self {
            actions: validated.actions.iter().map(|(_, d)| d.clone()).collect(),
        }
    }
    pub fn get(&self, id: &ActionId) -> Option<&ActionDef> {
        self.actions.iter().find(|a| &a.id == id)
    }
}

fn current_modifiers(keys: &ButtonInput<KeyCode>) -> Modifiers {
    Modifiers {
        ctrl: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        shift: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        alt: keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        cmd: keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
    }
}

fn is_modifier(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
    )
}

/// Active context layers, highest priority first. An overlay (gesture, focused
/// panel) is EXCLUSIVE — while a drag owns the pointer, `u`/`ctrl+s`/`:` must not
/// fall through and mutate state mid-gesture (flow-audit class: key fall-through).
pub fn active_contexts(
    state: &EditorState,
    mode: &CurrentMode,
    overlay: &OverlayContext,
    panel_focus: &crate::panels::PanelFocus,
    panel_catalog: &crate::panels::PanelCatalog,
) -> Vec<ContextId> {
    if state.active {
        // An EXCLUSIVE overlay grabs the keyboard entirely (gestures). A LAYERED
        // one wins its own keys and lets everything else fall through.
        if let Some(context) = &overlay.context {
            return if overlay.exclusive {
                vec![context.clone()]
            } else {
                vec![context.clone(), mode.context(), GLOBAL_CONTEXT]
            };
        }
        // A focused panel LAYERS its context over the mode (owner: the hierarchy is
        // just another way to navigate the scene — select a row, press w, move it).
        // Panel bindings win conflicts; everything unbound falls through to the mode.
        if let Some(panel) = &panel_focus.0
            && let Some(decl) = panel_catalog.get(panel)
        {
            return vec![decl.context.clone(), mode.context(), GLOBAL_CONTEXT];
        }
        vec![mode.context(), GLOBAL_CONTEXT]
    } else {
        vec![GLOBAL_CONTEXT]
    }
}

enum Resolution {
    Exact(ActionId),
    Prefix,
    None,
}

fn resolve_sequence(
    keymap: &ResolvedKeymapData,
    contexts: &[ContextId],
    pending: &[Chord],
) -> Resolution {
    // Layering: the first context with an exact match wins; a prefix match anywhere
    // keeps the sequence alive.
    let mut any_prefix = false;
    for context in contexts {
        let Some(entries) = keymap.by_context.get(context) else {
            continue;
        };
        for (binding, action) in entries {
            if binding.0 == pending {
                return Resolution::Exact(action.clone());
            }
            if binding.0.len() > pending.len() && binding.0.starts_with(pending) {
                any_prefix = true;
            }
        }
    }
    if any_prefix {
        Resolution::Prefix
    } else {
        Resolution::None
    }
}

/// Which-key (A8): the continuations available after `pending`, derived from the
/// dispatch table. Returns (next chord, full binding, action) sorted for display.
pub fn which_key_continuations(
    keymap: &ResolvedKeymapData,
    contexts: &[ContextId],
    pending: &[Chord],
) -> Vec<(Chord, Binding, ActionId)> {
    let mut seen: HashMap<Chord, (Binding, ActionId)> = HashMap::new();
    // Iterate lowest-priority first so higher-priority contexts overwrite.
    for context in contexts.iter().rev() {
        let Some(entries) = keymap.by_context.get(context) else {
            continue;
        };
        for (binding, action) in entries {
            if binding.0.len() > pending.len() && binding.0.starts_with(pending) {
                seen.insert(binding.0[pending.len()], (binding.clone(), action.clone()));
            }
        }
    }
    let mut result: Vec<_> = seen
        .into_iter()
        .map(|(chord, (binding, action))| (chord, binding, action))
        .collect();
    result.sort_by_key(|(chord, ..)| format!("{chord}"));
    result
}

/// THE input system (EditorSet::Input). Everything else consumes `ActionInvoked`.
pub fn resolve_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    keymap: Res<ResolvedKeymapData>,
    capture: Res<KeyCapture>,
    state: Res<EditorState>,
    overlay: Res<OverlayContext>,
    mode: Res<CurrentMode>,
    flying: Res<crate::camera::FlyingCamera>,
    panel_focus: Res<crate::panels::PanelFocus>,
    panel_catalog: Res<crate::panels::PanelCatalog>,
    mut escape_from_capture: ResMut<EscapeFromCapture>,
    mut pending: ResMut<PendingKeys>,
    mut actions: MessageWriter<ActionInvoked>,
    mut unresolved: MessageWriter<KeysUnresolved>,
    mut was_captured: Local<bool>,
) {
    let Some(keys) = keys else { return };
    // Fly-nav owns the keyboard while RMB is held (WASD is locomotion, not actions).
    if flying.0 {
        pending.0.clear();
        return;
    }
    // A capturing surface that commits-and-closes (palette Enter) releases the
    // flag DURING this frame's focused-input dispatch — before this system runs.
    // Without the one-frame latch the very key that committed would be resolved
    // a second time here (Enter → prefab.open while the palette also committed).
    let released_this_frame = *was_captured && !capture.0;
    *was_captured = capture.0;
    if released_this_frame {
        return;
    }
    if capture.0 {
        // Escape is the universal backout: it pierces text-field capture as a forced
        // escape-home so no window/state can ever trap the keyboard. The flag makes
        // it peel one layer only (the capturing surface).
        if keys.just_pressed(KeyCode::Escape) {
            escape_from_capture.0 = true;
            actions.write(ActionInvoked {
                action: ActionId::new_static("core.escape-home"),
                args: None,
                source: InvocationSource::Key,
            });
        }
        return;
    }
    let modifiers = current_modifiers(&keys);

    for key in keys.get_just_pressed() {
        if is_modifier(*key) {
            continue;
        }

        // Escape with a pending sequence just clears it (never resolves).
        if *key == KeyCode::Escape && !pending.0.is_empty() {
            pending.0.clear();
            continue;
        }

        pending.0.push(Chord {
            modifiers,
            key: *key,
        });
        let contexts = active_contexts(&state, &mode, &overlay, &panel_focus, &panel_catalog);
        match resolve_sequence(&keymap, &contexts, &pending.0) {
            Resolution::Exact(action) => {
                pending.0.clear();
                actions.write(ActionInvoked {
                    action,
                    args: None,
                    source: InvocationSource::Key,
                });
            }
            Resolution::Prefix => { /* keep collecting; which-key shows continuations */ }
            Resolution::None => {
                unresolved.write(KeysUnresolved(std::mem::take(&mut pending.0)));
            }
        }
    }
}

/// Kernel conventions, applied to actions from ANY invocation source (EditorSet::Tools):
/// `core.toggle-editor` flips ownership; `mode.<id>` enters a registered mode;
/// `core.escape-home` (bound to Escape in global) walks home to Normal — features may
/// also react to it (clear selection, close popups). Derived from the registry — no
/// hand-maintained switch (v1 anti-pattern).
pub fn apply_action_conventions(
    mut reader: MessageReader<ActionInvoked>,
    modes: Res<Modes>,
    mut state: ResMut<EditorState>,
    mut mode: ResMut<CurrentMode>,
    escape_from_capture: Res<EscapeFromCapture>,
    mut panel_focus: ResMut<crate::panels::PanelFocus>,
    mut mode_changed: MessageWriter<ModeChanged>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "core.toggle-editor" {
            state.active = !state.active;
        }
        if invoked.action.as_str() == "core.escape-home" && !escape_from_capture.0 {
            // One layer per press: a focused panel unfocuses FIRST; the mode walks
            // home only when the viewport already owns focus.
            if panel_focus.0.is_some() {
                panel_focus.0 = None;
            } else if mode.0 != MODE_NORMAL {
                set_mode(MODE_NORMAL, &mut mode, &mut mode_changed);
            }
        }
        if let Some(mode_id) = invoked.action.as_str().strip_prefix("mode.") {
            let target = ModeId::new(mode_id.to_string());
            if modes.get(&target).is_some() {
                set_mode(target, &mut mode, &mut mode_changed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorCorePlugin;
    use crate::keymap_data::{KeymapPaths, UserKeymap, UserOverride, build_keymap};
    use editor_api::feature::FeatureRegistry;

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test", "Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.mode(ModeDef::new("test-mode", "TestMode"))
                .action(
                    ActionDef::new("test.undo", "Undo")
                        .context("normal")
                        .bind("x"),
                )
                .action(
                    ActionDef::new("test.top", "Top")
                        .context("normal")
                        .bind("g g"),
                )
                .action(
                    ActionDef::new("test.place", "Place")
                        .context("test-mode")
                        .bind("x"),
                )
                .action(
                    ActionDef::new("mode.test-mode", "Test Mode")
                        .context("normal")
                        .bind("m"),
                );
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.finish();
        // Run startup (host_features) once.
        app.update();
        // Editor active for mode-context tests.
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    /// Simulate one clean key tap. No input plugin runs in these tests, so we do the
    /// per-frame `clear()` (of just_pressed/just_released) that
    /// `keyboard_input_system` would normally do.
    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(key);
        }
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    fn drain_actions(app: &mut App) -> Vec<ActionId> {
        let mut messages = app.world_mut().resource_mut::<Messages<ActionInvoked>>();
        messages.drain().map(|m| m.action.clone()).collect()
    }

    // A4: synthetic key -> exactly one ActionInvoked with the right id
    #[test]
    fn key_resolves_to_action_in_mode() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyX);
        let actions = drain_actions(&mut app);
        assert_eq!(actions, vec![ActionId::new_static("test.undo")]);
    }

    // A4: unbound key emits nothing
    #[test]
    fn unbound_key_emits_nothing() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyN); // n: unbound in every context
        assert!(drain_actions(&mut app).is_empty());
    }

    // A4: multi-chord sequence resolves after the full sequence
    #[test]
    fn sequence_resolves() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyG);
        assert!(drain_actions(&mut app).is_empty(), "prefix must not fire");
        press(&mut app, KeyCode::KeyG);
        assert_eq!(
            drain_actions(&mut app),
            vec![ActionId::new_static("test.top")]
        );
    }

    // A4: mode switch changes what resolves
    #[test]
    fn mode_switch_changes_resolution() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyM); // mode.test-mode
        assert_eq!(
            app.world().resource::<CurrentMode>().0,
            ModeId::new_static("test-mode")
        );
        press(&mut app, KeyCode::KeyX);
        let actions = drain_actions(&mut app);
        assert_eq!(actions, vec![ActionId::new_static("test.place")]);
        // Esc walks home
        press(&mut app, KeyCode::Escape);
        assert_eq!(app.world().resource::<CurrentMode>().0, MODE_NORMAL);
    }

    // A8: which-key continuations equal the registered bindings
    #[test]
    fn which_key_derives_from_registry() {
        let app = {
            let mut app = test_app();
            app.update();
            app
        };
        let keymap = app.world().resource::<ResolvedKeymapData>();
        let contexts = vec![ContextId::new_static("normal"), GLOBAL_CONTEXT];
        // After pressing nothing: single-chord starts should include u, g, i, f12...
        let conts = which_key_continuations(keymap, &contexts, &[]);
        let chords: Vec<String> = conts.iter().map(|(c, ..)| c.to_string()).collect();
        assert!(chords.contains(&"u".to_string()));
        assert!(chords.contains(&"g".to_string()));
        // After 'g': exactly the 'g g' continuation
        let g: Chord = "g".parse().unwrap();
        let conts = which_key_continuations(keymap, &contexts, &[g]);
        assert_eq!(conts.len(), 1);
        assert_eq!(conts[0].2, ActionId::new_static("test.top"));
    }

    // A5: user keymap overrides defaults; removing it restores them
    #[test]
    fn user_overlay_wins_and_reverts() {
        // Build validated registry directly (no app needed).
        let mut reg = FeatureRegistry::default();
        struct Core;
        impl EditorFeature for Core {
            fn manifest(&self) -> FeatureManifest {
                FeatureManifest::new("core", "Core")
            }
            fn register(&self, reg: &mut FeatureRegistry) {
                reg.mode(ModeDef::new("normal", "Normal")).action(
                    ActionDef::new("core.undo", "Undo")
                        .context("normal")
                        .bind("u"),
                );
            }
        }
        reg.register_feature(&Core);
        let validated = reg.validate().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keymap.ron");
        let user = UserKeymap {
            overrides: vec![UserOverride {
                context: "normal".into(),
                action: "core.undo".into(),
                binding: Some("ctrl+z".into()),
            }],
        };
        std::fs::write(&path, ron::to_string(&user).unwrap()).unwrap();

        let with_user = build_keymap(
            &validated,
            &KeymapPaths {
                user: Some(path.clone()),
            },
        )
        .unwrap();
        let normal = &with_user.by_context[&ContextId::new_static("normal")];
        assert_eq!(normal.len(), 1);
        assert_eq!(normal[0].0, "ctrl+z".parse().unwrap());

        // Without the file: defaults restored.
        std::fs::remove_file(&path).unwrap();
        let defaults = build_keymap(&validated, &KeymapPaths { user: Some(path) }).unwrap();
        let normal = &defaults.by_context[&ContextId::new_static("normal")];
        assert_eq!(normal[0].0, "u".parse().unwrap());
    }

    /// The kernel's OWN bindings have to be reachable, and a probe run is a
    /// slow way to learn they are not. This pins the four selection verbs to
    /// the keys the keymap doc promises.
    #[test]
    fn the_selection_verbs_are_bound_where_the_doc_says() {
        let mut app = App::new();
        app.add_plugins(crate::EditorCorePlugin);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;

        let keymap = app
            .world()
            .resource::<crate::keymap_data::ResolvedKeymapData>();
        let normal = editor_api::prelude::ContextId::new_static("normal");
        for (action, spelling) in [
            ("select.similar", "shift+8"),
            ("select.hide", "space h"),
            ("select.isolate", "space shift+h"),
            ("select.unhide-all", "space u"),
        ] {
            let binding: editor_api::keymap::Binding = spelling.parse().expect("parses");
            let rows = keymap
                .by_context
                .get(&normal)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let found = rows
                .iter()
                .find(|(b, _)| b.0 == binding.0)
                .map(|(_, id)| id.as_str().to_string());
            assert_eq!(
                found,
                Some(action.to_string()),
                "{action} is not on {spelling}"
            );
        }
    }
}
