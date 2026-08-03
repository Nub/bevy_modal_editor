//! `editor_core` — the kernel (spec §2). Hosts `editor_api` registrations, owns the
//! modal state machine and the single input resolver, and builds all dispatch data
//! from the validated registry — nothing here is hand-maintained per feature
//! (v1's central anti-pattern).
//!
//! M1 scope: feature host, modes, resolver, keymap layering, which-key data.
//! The `EditQueue` (M2) and panel shell (`editor_ui`) build on top.

pub mod camera;
pub mod clipboard;
pub mod edits;
pub mod gesture;
pub mod insert;
pub mod keymap_data;
pub mod modes;
pub mod panels;
pub mod resolver;
pub mod selection;
pub mod settings;

use bevy::prelude::*;
use editor_api::prelude::*;

pub mod prelude {
    pub use crate::clipboard::EditorClipboard;
    pub use crate::edits::{EditorComponents, History, HistoryRequests};
    pub use crate::camera::{is_viewport_camera, FlyingCamera};
    pub use crate::settings::EditorSettings;
    pub use crate::gesture::{GestureCounter, GestureMotion, MoveGesture, GESTURE_MOVE_CONTEXT};
    pub use crate::insert::{
        CursorGround, GridSnap, InsertState, KindCatalog, KindJustPicked, MODE_INSERT,
    };
    pub use crate::keymap_data::KeymapPaths;
    pub use crate::selection::{Selected, SelectionChanged};
    pub use crate::modes::{CurrentMode, ModeChanged, Modes, MODE_NORMAL};
    pub use crate::panels::{PanelCatalog, PanelFocus, PanelStates};
    pub use crate::resolver::{
        active_contexts, which_key_continuations, ActionCatalog, EditorState, KeyCapture,
        KeysUnresolved, OverlayContext, PendingKeys, PointerOverChrome, ResolvedKeymap,
    };
    pub use crate::EditorCorePlugin;
    pub use crate::{ProcessorCatalog, ValidatorCatalog};
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
        reg.mode(ModeDef::new("normal", "Normal").hint("RMB fly · click select · w move · i insert"))
            .mode(ModeDef::new("insert", "Insert").hint("click place · shift multi · esc done"))
            .action(
                ActionDef::new("mode.insert", "Insert Mode")
                    .describe("Place new entities")
                    .context("normal")
                    .bind("i"),
            )
            .action(
                ActionDef::new("core.toggle-grid-snap", "Toggle Grid Snap")
                    .describe("Quantize placement and movement to the grid")
                    .context("normal")
                    .context("insert")
                    .bind("space g"),
            )
            .action(
                ActionDef::new("core.toggle-editor", "Toggle Editor")
                    .describe("Switch between game and editor")
                    .bind("f12"),
            )
            .action(
                ActionDef::new("core.palette", "Command Palette")
                    .describe("Search and run any action")
                    .context("normal")
                    .context("insert")
                    .bind("shift+semicolon") // ':'
                    .bind("space p"), // leader style — also demos which-key
            )
            .action(
                ActionDef::new("core.find-object", "Find Object")
                    .describe("Search scene entities by name and select")
                    .context("normal")
                    .bind("space f"),
            )
            .action(
                ActionDef::new("panel.focus-left", "Focus Panel Left")
                    .describe("Move focus toward the left dock")
                    .bind("ctrl+h"),
            )
            .action(
                ActionDef::new("panel.focus-down", "Focus Panel Down")
                    .describe("Move focus downward (dock stack, then bottom dock)")
                    .bind("ctrl+j"),
            )
            .action(
                ActionDef::new("panel.focus-up", "Focus Panel Up")
                    .describe("Move focus upward")
                    .bind("ctrl+k"),
            )
            .action(
                ActionDef::new("panel.focus-right", "Focus Panel Right")
                    .describe("Move focus toward the right dock")
                    .bind("ctrl+l"),
            )
            // Undo/redo are GLOBAL (owner: "edit then u" must work from any panel
            // focus); the handler gates on editor ownership so play is untouched.
            .action(
                ActionDef::new("core.undo", "Undo")
                    .describe("Undo the last edit")
                    .bind("u"),
            )
            .action(
                ActionDef::new("core.redo", "Redo")
                    .describe("Redo the last undone edit")
                    .bind("ctrl+r"),
            );
        // Escape as data: global binding the conventions system (and features) react to.
        reg.action(ActionDef::new("core.escape-home", "Escape").bind("escape").hidden());
        // Selection is the text object (keymap doc): d cut, y yank, p paste.
        reg.action(
            ActionDef::new("select.delete", "Delete Selection")
                .describe("Delete (cut) the selected entities")
                .context("normal")
                .bind("d")
                .edit(),
        )
        .action(
            ActionDef::new("select.yank", "Yank Selection")
                .describe("Copy the selected entities to the clipboard")
                .context("normal")
                .bind("y"),
        )
        .action(
            ActionDef::new("select.paste", "Paste")
                .describe("Paste clipboard entities as new copies")
                .context("normal")
                .bind("p")
                .edit(),
        );
        // Selection.
        reg.action(
            ActionDef::new("select.all", "Select All")
                .describe("Select every scene entity")
                .context("normal")
                .bind("ctrl+a"),
        )
        .action(
            ActionDef::new("select.clear", "Clear Selection")
                .describe("Deselect everything")
                .context("normal"),
        );
        // Move gesture: its overlay keymap layer + actions.
        reg.context(gesture::GESTURE_MOVE_CONTEXT);
        reg.action(
            ActionDef::new("transform.move", "Move Selection")
                .describe("Start a move gesture on the selection")
                .context("normal")
                .bind("w")
                .edit(),
        )
        .action(
            ActionDef::new("transform.axis-x", "Constrain X")
                .context("gesture-move")
                .bind("x")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.axis-y", "Constrain Y")
                .context("gesture-move")
                .bind("y")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.axis-z", "Constrain Z")
                .context("gesture-move")
                .bind("z")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.commit", "Commit Gesture")
                .context("gesture-move")
                .bind("enter")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.cancel", "Cancel Gesture")
                .context("gesture-move")
                .bind("escape")
                .hidden(),
        );
    }
}

pub struct EditorCorePlugin;

impl Plugin for EditorCorePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ActionInvoked>()
            .add_message::<modes::ModeChanged>()
            .add_message::<resolver::KeysUnresolved>()
            .add_message::<Edited>()
            .init_resource::<resolver::EditorState>()
            .init_resource::<resolver::PendingKeys>()
            .init_resource::<resolver::KeyCapture>()
            .init_resource::<keymap_data::KeymapPaths>()
            .init_resource::<EditQueue>()
            .init_resource::<SceneIndex>()
            .init_resource::<edits::EditorComponents>()
            .init_resource::<edits::History>()
            .init_resource::<edits::HistoryRequests>()
            .init_resource::<resolver::OverlayContext>()
            .init_resource::<resolver::EscapeFromCapture>()
            .init_resource::<resolver::PointerOverChrome>()
            .init_resource::<camera::FlyingCamera>()
            .init_resource::<settings::EditorSettings>()
            .init_resource::<gesture::MoveGesture>()
            .init_resource::<gesture::GestureMotion>()
            .init_resource::<gesture::GestureCounter>()
            .init_resource::<insert::InsertState>()
            .init_resource::<insert::GridSnap>()
            .init_resource::<insert::CursorGround>()
            .init_resource::<insert::KindCatalog>()
            .init_resource::<insert::KindJustPicked>()
            .init_resource::<panels::PanelCatalog>()
            .init_resource::<panels::PanelStates>()
            .init_resource::<panels::PanelFocus>()
            .init_resource::<edits::MergeFrameEntries>()
            .init_resource::<clipboard::EditorClipboard>()
            .init_resource::<clipboard::ClipboardRequests>()
            .init_resource::<clipboard::PendingPasteSelect>()
            .add_message::<selection::SelectionChanged>();

        app.add_observer(edits::index_on_add);
        app.add_observer(edits::index_on_remove);
        app.add_observer(selection::on_pointer_press);

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
                (camera::editor_fly_camera, resolver::resolve_input)
                    .chain()
                    .in_set(EditorSet::Input),
                (
                    resolver::apply_action_conventions,
                    panels::handle_panel_actions,
                    clipboard::collect_clipboard_actions,
                    clipboard::perform_clipboard,
                    edits::handle_history_actions,
                    selection::handle_selection_actions,
                    gesture::handle_gesture_actions,
                    gesture::motion_from_cursor,
                    gesture::drive_gesture,
                    gesture::commit_on_click,
                    insert::handle_insert_actions,
                    insert::cursor_ground,
                    insert::sync_preview,
                    insert::place_on_click,
                    |mut flag: ResMut<resolver::EscapeFromCapture>| {
                        flag.0 = false;
                    },
                )
                    .chain()
                    .in_set(EditorSet::Tools),
                edits::apply_edits.in_set(EditorSet::Mutate),
                edits::ensure_entity_names.in_set(EditorSet::Sync),
                clipboard::select_pasted.in_set(EditorSet::Sync),
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

    // Registry-derived insert actions: one per entity kind (kernel convention).
    let kind_actions: Vec<(FeatureId, ActionDef)> = registry
        .kinds
        .iter()
        .map(|(feature, kind)| {
            let def = ActionDef {
                id: ActionId::new(format!("insert.kind.{}", kind.id)),
                name: format!("Insert: {}", kind.display_name).into(),
                description: std::borrow::Cow::Borrowed("Place a new entity of this kind"),
                contexts: vec![
                    ContextId::new_static("normal"),
                    ContextId::new_static("insert"),
                ],
                default_bindings: Vec::new(),
                flags: editor_api::actions::ActionFlags { is_edit: true, hidden: false },
            };
            (feature.clone(), def)
        })
        .collect();
    for (feature, def) in kind_actions {
        registry.synthesize_action(feature, def);
    }

    // Registry-derived panel actions: one `panel.toggle.<id>` per panel (kernel
    // convention — every panel is palette-toggleable without feature code).
    let panel_actions: Vec<(FeatureId, ActionDef)> = registry
        .panels
        .iter()
        .map(|(feature, panel)| {
            let def = ActionDef {
                id: ActionId::new(format!("panel.toggle.{}", panel.id)),
                name: format!("Toggle Panel: {}", panel.title).into(),
                description: std::borrow::Cow::Borrowed("Show or hide this panel"),
                contexts: Vec::new(), // global: panels toggle from anywhere
                default_bindings: Vec::new(),
                flags: editor_api::actions::ActionFlags::default(),
            };
            (feature.clone(), def)
        })
        .collect();
    for (feature, def) in panel_actions {
        registry.synthesize_action(feature, def);
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

    // Component registrations: reflection + the capture set, in one place (spec §5).
    {
        let registry = world.resource::<AppTypeRegistry>().clone();
        for (_, reg) in &validated.components {
            (reg.register)(&registry);
        }
        world.insert_resource(edits::EditorComponents {
            types: validated.components.iter().map(|(_, r)| r.clone()).collect(),
        });
    }

    world.insert_resource(insert::KindCatalog {
        kinds: validated.kinds.iter().map(|(_, k)| k.clone()).collect(),
    });
    world.insert_resource(ValidatorCatalog {
        validators: validated.validators.iter().map(|(_, v)| v.clone()).collect(),
    });
    world.insert_resource(ProcessorCatalog {
        processors: validated.processors.iter().map(|(_, p)| p.clone()).collect(),
    });
    world.insert_resource(panels::PanelCatalog {
        panels: validated.panels.iter().map(|(_, p)| p.clone()).collect(),
    });
    world.insert_resource(panels::PanelStates(
        validated.panels.iter().map(|(_, p)| (p.id.clone(), p.default_open)).collect(),
    ));
    world.insert_resource(modes::Modes::from_validated(&validated));
    world.insert_resource(modes::CurrentMode(MODE_NORMAL));
    world.insert_resource(resolver::ActionCatalog::from_validated(&validated));
    world.insert_resource(keymap);
}

pub use modes::MODE_NORMAL;

/// All registered import-time validators (M4-D2) — the ingestion pipeline runs
/// these; the registry is the ONE extension point (games add theirs like actions).
#[derive(Resource, Default)]
pub struct ValidatorCatalog {
    pub validators: Vec<editor_api::validate::ValidatorDef>,
}

/// All registered asset processors (M4-D3) — the Process runner consumes these.
#[derive(Resource, Default)]
pub struct ProcessorCatalog {
    pub processors: Vec<editor_api::pipeline::ProcessorDef>,
}
