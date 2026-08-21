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
pub mod hide;
pub mod insert;
pub mod keymap_data;
pub mod layout;
pub mod lock;
pub mod mirror;
pub mod modes;
pub mod panels;
pub mod resolver;
pub mod selection;
pub mod settings;
pub mod similar;

use bevy::prelude::*;
use editor_api::prelude::*;

pub mod prelude {
    pub use crate::EditorCorePlugin;
    pub use crate::camera::{FlyingCamera, is_viewport_camera};
    pub use crate::clipboard::EditorClipboard;
    pub use crate::edits::{EditorComponents, History, HistoryRequests, HistoryScope};
    pub use crate::gesture::{
        GESTURE_MOVE_CONTEXT, GestureCounter, GestureKind, GestureMotion, GesturePivot, MoveGesture,
    };
    pub use crate::insert::{
        AngleSnap, CursorGround, GridSnap, InsertState, KindCatalog, KindJustPicked, MODE_INSERT,
    };
    pub use crate::keymap_data::KeymapPaths;
    pub use crate::modes::{CurrentMode, MODE_NORMAL, ModeChanged, Modes};
    pub use crate::panels::{PanelCatalog, PanelFocus, PanelStates};
    pub use crate::resolver::{
        ActionCatalog, EditorState, KeyCapture, KeysUnresolved, OverlayContext, PendingKeys,
        PointerOverChrome, ResolvedKeymap, active_contexts, which_key_continuations,
    };
    pub use crate::selection::{
        Marquee, PendingSelect, Selected, SelectionChanged, SelectionScope, SelectionSealed,
    };
    pub use crate::settings::EditorSettings;
    pub use crate::{BakerCatalog, GizmoCatalog, ProcessorCatalog, ValidatorCatalog};
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
        reg.mode(
            ModeDef::new("normal", "Normal").hint("RMB fly · click select · w move · i insert"),
        )
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
            ActionDef::new("core.toggle-angle-snap", "Toggle Angle Snap")
                .describe("Quantize rotation to the angle step (15° by default)")
                .context("normal")
                .context("insert")
                .bind("space a"),
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
                .describe("Search the scene — or the selection's components when holding one")
                .context("normal")
                .bind("slash")
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
            ActionDef::new("camera.frame", "Frame Selection")
                .describe("Pull the camera back until the selection fits the view")
                .context("normal")
                .bind("z z"),
        )
        .action(
            ActionDef::new("camera.frame-scene", "Frame Scene")
                .describe("Fit the whole scene in view")
                .context("normal")
                .bind("z f"),
        )
        // The six canonical views (owner): 1 front, 2 left, 3 top — shift for
        // the opposite face. Orthographic, because reading alignment is the
        // whole point of standing square to an axis.
        .action(
            ActionDef::new("view.front", "View: Front")
                .context("normal")
                .bind("1"),
        )
        .action(
            ActionDef::new("view.back", "View: Back")
                .context("normal")
                .bind("shift+1"),
        )
        .action(
            ActionDef::new("view.left", "View: Left")
                .context("normal")
                .bind("2"),
        )
        .action(
            ActionDef::new("view.right", "View: Right")
                .context("normal")
                .bind("shift+2"),
        )
        .action(
            ActionDef::new("view.top", "View: Top")
                .context("normal")
                .bind("3"),
        )
        .action(
            ActionDef::new("view.bottom", "View: Bottom")
                .context("normal")
                .bind("shift+3"),
        )
        .action(
            ActionDef::new("view.perspective", "View: Perspective")
                .describe("Back to the normal perspective view")
                .context("normal")
                .bind("4"),
        )
        .action(
            ActionDef::new("view.toggle-grid", "Toggle Grid")
                .describe("Show or hide the editor's ground grid")
                .context("normal")
                .bind("space t g"),
        )
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
        reg.action(
            ActionDef::new("core.escape-home", "Escape")
                .bind("escape")
                .hidden(),
        );
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
            ActionDef::new("select.duplicate", "Duplicate Selection")
                .describe(
                    "Copy the selection in place and grab it — \
                     the register is left alone, so a yank survives a run of duplicates",
                )
                .context("normal")
                .bind("shift+d")
                .edit(),
        )
        .action(
            ActionDef::new("select.paste", "Paste")
                .describe("Paste clipboard entities as new copies")
                .context("normal")
                .bind("p")
                .edit(),
        );
        reg.action(
            ActionDef::new("object.lock", "Lock / Unlock Selection")
                .describe(
                    "Locked objects refuse every edit — move, delete, reparent, mate — \
                     until unlocked. Applies to the whole selection",
                )
                .context("normal")
                .bind("space l"),
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
        )
        .action(
            ActionDef::new("select.similar", "Select Similar")
                .describe("Select every object like this one — same prefab, model or kind")
                .context("normal")
                // `*`, spelled physically. `KeyCode` is a PHYSICAL key, so there
                // is no "*" token to parse; same convention as shift+semicolon
                // for `:`.
                .bind("shift+8"),
        )
        .action(
            ActionDef::new("select.hide", "Hide Selection")
                .describe(
                    "Take the selection out of the view — it stays in the level \
                     and in the hierarchy. Not undoable: unhide with space u",
                )
                .context("normal")
                .bind("space h"),
        )
        .action(
            ActionDef::new("select.isolate", "Isolate Selection")
                .describe(
                    "Hide everything but the selection; press again to restore \
                     exactly what was hidden before",
                )
                .context("normal")
                .bind("space shift+h"),
        )
        .action(
            ActionDef::new("select.unhide-all", "Unhide All")
                .describe("Bring back everything hidden")
                .context("normal")
                .bind("space u"),
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
            ActionDef::new("transform.rotate", "Rotate Selection")
                .describe(
                    "Start a rotate gesture on the selection — \
                     x/y/z constrain, typed amounts are DEGREES (yaw by default)",
                )
                .context("normal")
                .bind("e")
                .edit(),
        )
        .action(
            ActionDef::new("transform.scale", "Scale Selection")
                .describe(
                    "Start a scale gesture on the selection — \
                     x/y/z scale ONE axis, typed amounts are a FACTOR (2 = twice as big)",
                )
                .context("normal")
                .bind("r")
                .edit(),
        )
        .action(
            ActionDef::new("transform.mirror-x", "Mirror across X")
                .describe(
                    "Reflect the selection across the plane through its centre — \
                     placement only, geometry is never flipped and scale never \
                     goes negative. mirror flip symmetry",
                )
                .context("normal")
                .bind("space x shift+x")
                .edit(),
        )
        .action(
            ActionDef::new("transform.mirror-y", "Mirror across Y")
                .describe("Reflect the selection across the plane through its centre")
                .context("normal")
                .bind("space x shift+y")
                .edit(),
        )
        .action(
            ActionDef::new("transform.mirror-z", "Mirror across Z")
                .describe("Reflect the selection across the plane through its centre")
                .context("normal")
                .bind("space x shift+z")
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
            ActionDef::new("transform.digit-0", "Typed Amount 0")
                .context("gesture-move")
                .bind("0")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-1", "Typed Amount 1")
                .context("gesture-move")
                .bind("1")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-2", "Typed Amount 2")
                .context("gesture-move")
                .bind("2")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-3", "Typed Amount 3")
                .context("gesture-move")
                .bind("3")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-4", "Typed Amount 4")
                .context("gesture-move")
                .bind("4")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-5", "Typed Amount 5")
                .context("gesture-move")
                .bind("5")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-6", "Typed Amount 6")
                .context("gesture-move")
                .bind("6")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-7", "Typed Amount 7")
                .context("gesture-move")
                .bind("7")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-8", "Typed Amount 8")
                .context("gesture-move")
                .bind("8")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-9", "Typed Amount 9")
                .context("gesture-move")
                .bind("9")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-erase", "Erase Typed Amount")
                .context("gesture-move")
                .bind("backspace")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-dot", "Typed Decimal Point")
                .context("gesture-move")
                .bind("period")
                .hidden(),
        )
        .action(
            ActionDef::new("transform.digit-minus", "Typed Sign Toggle")
                .context("gesture-move")
                .bind("minus")
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
        // Locking persists WITH the level: a floor you locked is still locked
        // when you reopen the file, which is the entire point of locking it.
        reg.component::<lock::Locked>();
    }
}

pub struct EditorCorePlugin;

impl Plugin for EditorCorePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ActionInvoked>()
            .add_message::<modes::ModeChanged>()
            .add_message::<resolver::KeysUnresolved>()
            // The kernel refuses edits (a locked object) and must be able to
            // SAY so — the channel is registered here, not by whichever
            // feature crate happened to define it first.
            .add_message::<editor_api::feedback::SceneIoFeedback>()
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
            .init_resource::<edits::HistoryScope>()
            .init_resource::<resolver::OverlayContext>()
            .init_resource::<resolver::EscapeFromCapture>()
            .init_resource::<lock::LockRequests>()
            .init_resource::<hide::Hidden>()
            .init_resource::<hide::HideRequests>()
            .init_resource::<similar::IdentityCatalog>()
            .init_resource::<mirror::MirrorRequests>()
            .init_resource::<resolver::PointerOverChrome>()
            // Headless worlds have no `InputPlugin`, so the wheel message would
            // not exist and every system reading it would fail validation. The
            // kernel tolerates a missing AssetServer the same way.
            .add_message::<bevy::input::mouse::MouseWheel>()
            .init_resource::<selection::SelectionScope>()
            .init_resource::<selection::PendingSelect>()
            .init_resource::<selection::Marquee>()
            .init_resource::<camera::FlyingCamera>()
            .insert_resource(settings::EditorSettings::load_user())
            .init_resource::<gesture::MoveGesture>()
            .init_resource::<gesture::GestureMotion>()
            .init_resource::<gesture::GesturePivot>()
            .init_resource::<gesture::GestureCounter>()
            .init_resource::<insert::InsertState>()
            .init_resource::<insert::GridSnap>()
            .init_resource::<insert::AngleSnap>()
            .init_resource::<clipboard::PendingDuplicateGrab>()
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
        app.add_observer(selection::on_pointer_release);

        app.add_editor_feature(CoreFeature);

        app.configure_sets(
            Update,
            (
                EditorSet::Input,
                EditorSet::Tools,
                EditorSet::Mutate,
                EditorSet::Sync,
            )
                .chain(),
        );

        // Registration happens in a Startup system so features added after the plugin
        // (the normal case: game main composes plugins in any order) are included.
        app.add_systems(PreStartup, host_features);
        // The ONE writer of `Visibility`, in PostUpdate because the system that
        // hands the world to the game lives inside editor_scene's own chain and
        // the kernel must not depend on editor_scene to order against it.
        app.add_systems(
            PostUpdate,
            hide::sync_hidden_visibility
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
        app.add_systems(
            Update,
            (
                (
                    camera::editor_fly_camera,
                    camera::editor_zoom_camera,
                    camera::orbit_camera,
                    resolver::resolve_input,
                )
                    .chain()
                    .in_set(EditorSet::Input),
                (
                    resolver::apply_action_conventions,
                    panels::handle_panel_actions,
                    clipboard::collect_clipboard_actions,
                    clipboard::perform_clipboard,
                    edits::handle_history_actions,
                    selection::track_marquee,
                    selection::handle_selection_actions,
                    selection::select_pending,
                    camera::handle_frame_actions,
                    camera::handle_axis_views,
                    camera::handle_perspective_view,
                    // Nested: the gesture pipeline is one ordered unit, and the
                    // outer tuple is at bevy's system-tuple limit.
                    (
                        lock::collect_lock_actions,
                        hide::collect_hide_actions,
                        hide::perform_hide,
                        similar::perform_select_similar,
                        mirror::collect_mirror_actions,
                        mirror::perform_mirror,
                        gesture::handle_gesture_actions,
                        gesture::motion_from_cursor,
                        gesture::push_pull_gesture,
                        gesture::drive_gesture,
                        gesture::commit_on_click,
                    )
                        .chain(),
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
                (lock::perform_lock, edits::apply_edits)
                    .chain()
                    .in_set(EditorSet::Mutate),
                edits::ensure_entity_names.in_set(EditorSet::Sync),
                (clipboard::select_pasted, clipboard::grab_duplicate)
                    .chain()
                    .in_set(EditorSet::Sync),
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
                group: Some(editor_api::actions::PaletteGroup::PLACE),
                flags: editor_api::actions::ActionFlags {
                    is_edit: true,
                    hidden: false,
                },
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
                group: Some(editor_api::actions::PaletteGroup::VIEW),
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
            types: validated
                .components
                .iter()
                .map(|(_, r)| r.clone())
                .collect(),
        });
    }

    // The identity ladder for `*` (spec §9). Built AFTER component
    // registration, because a rung is validated against the type registry and
    // nothing is in it before that loop runs.
    //
    // A bad rung is a STARTUP PANIC, never a verb that quietly does nothing: a
    // key that stops resolving is a `*` that stops working, and silence is how
    // that ships.
    {
        let mut rungs: Vec<editor_api::identity::IdentityDef> = validated
            .identities
            .iter()
            .map(|(_, def)| def.clone())
            .collect();
        rungs.sort_by_key(|def| def.priority);
        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let registry = registry_arc.read();
        let mut seen = std::collections::HashSet::new();
        let mut errors: Vec<String> = Vec::new();
        for def in &rungs {
            if !seen.insert((def.type_path, def.key)) {
                errors.push(format!(
                    "duplicate identity rung {}#{}",
                    def.type_path, def.key
                ));
            }
            let Some(registration) = registry.get(def.component) else {
                errors.push(format!(
                    "identity rung {}: not in the AppTypeRegistry",
                    def.type_path
                ));
                continue;
            };
            if registration
                .data::<bevy::ecs::reflect::ReflectComponent>()
                .is_none()
            {
                errors.push(format!(
                    "identity rung {}: not #[reflect(Component)]",
                    def.type_path
                ));
            }
            if !def.key.is_empty() && def.key != "*" {
                let resolves = registration
                    .type_info()
                    .as_struct()
                    .ok()
                    .is_some_and(|info| info.field(def.key).is_some());
                if !resolves {
                    errors.push(format!(
                        "identity rung {}: no field {:?}",
                        def.type_path, def.key
                    ));
                }
            }
        }
        if !errors.is_empty() {
            panic!("identity registration failed:\n  {}", errors.join("\n  "));
        }
        drop(registry);
        world.insert_resource(similar::IdentityCatalog { rungs });
    }

    world.insert_resource(insert::KindCatalog {
        kinds: validated.kinds.iter().map(|(_, k)| k.clone()).collect(),
    });
    world.insert_resource(ValidatorCatalog {
        validators: validated
            .validators
            .iter()
            .map(|(_, v)| v.clone())
            .collect(),
    });
    world.insert_resource(LevelValidatorCatalog {
        validators: validated
            .level_validators
            .iter()
            .map(|(_, v)| v.clone())
            .collect(),
    });
    world.insert_resource(ProcessorCatalog {
        processors: validated
            .processors
            .iter()
            .map(|(_, p)| p.clone())
            .collect(),
    });
    world.insert_resource(BakerCatalog {
        bakers: validated.bakers.iter().map(|(_, b)| b.clone()).collect(),
    });
    world.insert_resource(GizmoCatalog {
        gizmos: validated.gizmos.iter().map(|(_, g)| g.clone()).collect(),
    });
    world.insert_resource(panels::PanelCatalog {
        panels: validated.panels.iter().map(|(_, p)| p.clone()).collect(),
    });
    world.insert_resource(panels::PanelStates(
        validated
            .panels
            .iter()
            .map(|(_, p)| (p.id.clone(), p.default_open))
            .collect(),
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

/// All registered LEVEL validators (v1 parity): rules the live scene must
/// satisfy — required configs, objects, components.
#[derive(Resource, Default)]
pub struct LevelValidatorCatalog {
    pub validators: Vec<editor_api::validate::LevelValidatorDef>,
}

/// All registered asset processors (M4-D3) — the Process runner consumes these.
#[derive(Resource, Default)]
pub struct ProcessorCatalog {
    pub processors: Vec<editor_api::pipeline::ProcessorDef>,
}

/// Every registered bake step (M4-D8), for the bake runner + CLI.
#[derive(Resource, Default)]
pub struct BakerCatalog {
    pub bakers: Vec<editor_api::bake::BakerDef>,
}

/// Every registered viewport gizmo (spec §7): games declare how their own
/// components LOOK, and the editor draws and picks them without knowing the
/// types.
#[derive(Resource, Default)]
pub struct GizmoCatalog {
    pub gizmos: Vec<editor_api::gizmos::GizmoDef>,
}
