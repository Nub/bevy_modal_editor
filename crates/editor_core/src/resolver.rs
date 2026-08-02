//! The single input resolver (spec §4, RFC §4): the ONLY reader of raw keyboard input
//! in the entire editor. Tracks multi-chord sequences, resolves through the active
//! context layers (mode first, then global), and emits `ActionInvoked`.
//! Which-key data (A8) is derived from the same table — no hand-maintained hints.

use crate::keymap_data::ResolvedKeymapData;
use crate::modes::{set_mode, CurrentMode, ModeChanged, Modes, MODE_NORMAL};
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

/// Palette/cheat-sheet data: every registered action (A8's sibling — derived, never
/// hand-maintained).
#[derive(Resource)]
pub struct ActionCatalog {
    pub actions: Vec<ActionDef>,
}

impl ActionCatalog {
    pub fn from_validated(validated: &ValidatedFeatures) -> Self {
        Self { actions: validated.actions.iter().map(|(_, d)| d.clone()).collect() }
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
        KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::ShiftLeft
            | KeyCode::ShiftRight | KeyCode::AltLeft | KeyCode::AltRight
            | KeyCode::SuperLeft | KeyCode::SuperRight
    )
}

/// Active context layers, highest priority first.
pub fn active_contexts(state: &EditorState, mode: &CurrentMode) -> Vec<ContextId> {
    if state.active {
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
        let Some(entries) = keymap.by_context.get(context) else { continue };
        for (binding, action) in entries {
            if binding.0 == pending {
                return Resolution::Exact(action.clone());
            }
            if binding.0.len() > pending.len() && binding.0.starts_with(pending) {
                any_prefix = true;
            }
        }
    }
    if any_prefix { Resolution::Prefix } else { Resolution::None }
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
        let Some(entries) = keymap.by_context.get(context) else { continue };
        for (binding, action) in entries {
            if binding.0.len() > pending.len() && binding.0.starts_with(pending) {
                seen.insert(binding.0[pending.len()], (binding.clone(), action.clone()));
            }
        }
    }
    let mut result: Vec<_> =
        seen.into_iter().map(|(chord, (binding, action))| (chord, binding, action)).collect();
    result.sort_by_key(|(chord, ..)| format!("{chord}"));
    result
}

/// THE input system (EditorSet::Input). Everything else consumes `ActionInvoked`.
pub fn resolve_input(
    keys: Res<ButtonInput<KeyCode>>,
    keymap: Res<ResolvedKeymapData>,
    modes: Res<Modes>,
    mut state: ResMut<EditorState>,
    mut mode: ResMut<CurrentMode>,
    mut pending: ResMut<PendingKeys>,
    mut actions: MessageWriter<ActionInvoked>,
    mut mode_changed: MessageWriter<ModeChanged>,
) {
    let modifiers = current_modifiers(&keys);

    for key in keys.get_just_pressed() {
        if is_modifier(*key) {
            continue;
        }

        // Escape is kernel-owned: clear pending; if clean, walk home to Normal.
        if *key == KeyCode::Escape && state.active {
            if pending.0.is_empty() {
                set_mode(MODE_NORMAL, &mut mode, &mut mode_changed);
            } else {
                pending.0.clear();
            }
            continue;
        }

        pending.0.push(Chord { modifiers, key: *key });
        let contexts = active_contexts(&state, &mode);
        match resolve_sequence(&keymap, &contexts, &pending.0) {
            Resolution::Exact(action) => {
                pending.0.clear();
                // Kernel-owned actions short-circuit here; everything else broadcasts.
                if action.as_str() == "core.toggle-editor" {
                    state.active = !state.active;
                }
                // Mode entry actions: any action named "mode.<id>" switches mode if
                // the mode exists (registered convention, not hand-maintained).
                if let Some(mode_id) = action.as_str().strip_prefix("mode.") {
                    let target = ModeId::new(mode_id.to_string());
                    if modes.get(&target).is_some() {
                        set_mode(target, &mut mode, &mut mode_changed);
                    }
                }
                actions.write(ActionInvoked {
                    action,
                    args: None,
                    source: InvocationSource::Key,
                });
            }
            Resolution::Prefix => { /* keep collecting; which-key shows continuations */ }
            Resolution::None => pending.0.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap_data::{build_keymap, KeymapPaths, UserKeymap, UserOverride};
    use crate::EditorCorePlugin;
    use editor_api::feature::FeatureRegistry;

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test", "Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.mode(ModeDef::new("insert", "Insert"))
                .action(ActionDef::new("test.undo", "Undo").context("normal").bind("u"))
                .action(ActionDef::new("test.top", "Top").context("normal").bind("g g"))
                .action(
                    ActionDef::new("test.place", "Place").context("insert").bind("u"),
                )
                .action(ActionDef::new("mode.insert", "Insert Mode").context("normal").bind("i"));
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
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(key);
        }
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    }

    fn drain_actions(app: &mut App) -> Vec<ActionId> {
        let mut messages = app.world_mut().resource_mut::<Messages<ActionInvoked>>();
        messages.drain().map(|m| m.action.clone()).collect()
    }

    // A4: synthetic key -> exactly one ActionInvoked with the right id
    #[test]
    fn key_resolves_to_action_in_mode() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyU);
        let actions = drain_actions(&mut app);
        assert_eq!(actions, vec![ActionId::new_static("test.undo")]);
    }

    // A4: unbound key emits nothing
    #[test]
    fn unbound_key_emits_nothing() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyQ);
        assert!(drain_actions(&mut app).is_empty());
    }

    // A4: multi-chord sequence resolves after the full sequence
    #[test]
    fn sequence_resolves() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyG);
        assert!(drain_actions(&mut app).is_empty(), "prefix must not fire");
        press(&mut app, KeyCode::KeyG);
        assert_eq!(drain_actions(&mut app), vec![ActionId::new_static("test.top")]);
    }

    // A4: mode switch changes what resolves
    #[test]
    fn mode_switch_changes_resolution() {
        let mut app = test_app();
        press(&mut app, KeyCode::KeyI); // mode.insert
        assert_eq!(
            app.world().resource::<CurrentMode>().0,
            ModeId::new_static("insert")
        );
        press(&mut app, KeyCode::KeyU);
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
                reg.mode(ModeDef::new("normal", "Normal"))
                    .action(ActionDef::new("core.undo", "Undo").context("normal").bind("u"));
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
            &KeymapPaths { user: Some(path.clone()) },
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
}
