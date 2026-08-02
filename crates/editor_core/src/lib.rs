//! `editor_core` — the kernel (spec §2). Hosts `editor_api` registrations, owns the
//! modal state machine and the single input resolver, and builds all dispatch data
//! from the validated registry — nothing here is hand-maintained per feature
//! (v1's central anti-pattern).
//!
//! M1 scope: feature host, modes, resolver, keymap layering, which-key data.
//! The `EditQueue` (M2) and panel shell (`editor_ui`) build on top.

pub mod keymap_data;
pub mod modes;
pub mod resolver;

use bevy::prelude::*;
use editor_api::prelude::*;

pub mod prelude {
    pub use crate::keymap_data::KeymapPaths;
    pub use crate::modes::{CurrentMode, ModeChanged, Modes, MODE_NORMAL};
    pub use crate::resolver::{
        active_contexts, which_key_continuations, ActionCatalog, EditorState, KeyCapture,
        PendingKeys, ResolvedKeymap,
    };
    pub use crate::EditorCorePlugin;
    pub use editor_api::prelude::*;
}

/// Kernel-owned system sets, ordered (spec §8: explicit ordering by construction).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum EditorSet {
    /// Input resolution: keys -> ActionInvoked. Nothing else reads raw keys.
    Input,
    /// Feature systems consuming actions / driving tools.
    Tools,
    /// Scene mutation (EditQueue application — M2).
    Mutate,
    /// Derived-state sync (regenerate hooks, which-key, statusline data).
    Sync,
}

/// The kernel's own feature: built-in modes and actions, registered through the same
/// front door as everything else — the kernel eats its own contract.
struct CoreFeature;

impl EditorFeature for CoreFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("core", "Editor Core")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.mode(ModeDef::new("normal", "Normal").hint("navigate/select"))
            .action(
                ActionDef::new("core.toggle-editor", "Toggle Editor")
                    .describe("Switch between game and editor")
                    .bind("f12"),
            )
            .action(
                ActionDef::new("core.palette", "Command Palette")
                    .describe("Search and run any action")
                    .context("normal")
                    .bind("shift+semicolon") // ':'
                    .bind("space p"), // leader style — also demos which-key
            );
    }
}

pub struct EditorCorePlugin;

impl Plugin for EditorCorePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ActionInvoked>()
            .add_message::<modes::ModeChanged>()
            .init_resource::<resolver::EditorState>()
            .init_resource::<resolver::PendingKeys>()
            .init_resource::<resolver::KeyCapture>()
            .init_resource::<keymap_data::KeymapPaths>();

        app.add_editor_feature(CoreFeature);

        app.configure_sets(
            Update,
            (EditorSet::Input, EditorSet::Tools, EditorSet::Mutate, EditorSet::Sync).chain(),
        );

        // Registration happens in a Startup system so features added after the plugin
        // (the normal case: game main composes plugins in any order) are included.
        app.add_systems(PreStartup, host_features);
        app.add_systems(
            Update,
            (
                resolver::resolve_input.in_set(EditorSet::Input),
                resolver::apply_action_conventions.in_set(EditorSet::Tools),
            ),
        );
    }
}

/// Drain `PendingFeatures`, validate, and build dispatch data. Any registration
/// problem is a startup panic listing every error — never a silent skip (spec §8).
fn host_features(world: &mut World) {
    let pending = world
        .remove_resource::<PendingFeatures>()
        .unwrap_or_default();

    let mut registry = FeatureRegistry::default();
    for feature in &pending.0 {
        registry.register_feature(feature.as_ref());
    }

    let validated = match registry.validate() {
        Ok(v) => v,
        Err(errors) => {
            let joined: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!(
                "editor feature registration failed with {} error(s):\n  {}",
                joined.len(),
                joined.join("\n  ")
            );
        }
    };

    let paths = world.resource::<keymap_data::KeymapPaths>().clone();
    let keymap = match keymap_data::build_keymap(&validated, &paths) {
        Ok(k) => k,
        Err(e) => panic!("keymap load failed: {e}"),
    };

    world.insert_resource(modes::Modes::from_validated(&validated));
    world.insert_resource(modes::CurrentMode(MODE_NORMAL));
    world.insert_resource(resolver::ActionCatalog::from_validated(&validated));
    world.insert_resource(keymap);
}

pub use modes::MODE_NORMAL;
