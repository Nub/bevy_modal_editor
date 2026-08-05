//! Editor chrome (spec §2 `editor_ui`): the canonical editor UI every game gets by
//! depending on this crate — command palette, status bar, which-key popup, and the
//! style system. Games contribute *content* (kinds, components, features) through
//! `editor_api`; they never own chrome.
//!
//! `EditorUiPlugin` is the single editor entry point for a game binary: it brings
//! the kernel (`editor_core`), scene I/O (`editor_scene`), feathers theming, and
//! all chrome surfaces. Fonts ship embedded — games don't copy asset files.

pub mod style;

mod dock;
mod ghost;
mod hierarchy;
mod inspector;
mod outline;
mod palette;
mod palette_engine;
mod palette_preview;
mod probe_prefab;
mod probe_user;
mod prompt;
mod statusbar;
mod which_key;

pub use palette::{PaletteFilter, PaletteState};

use bevy::asset::embedded_asset;
use bevy::feathers::{FeathersPlugins, dark_theme::create_dark_theme, theme::UiTheme};
use bevy::prelude::*;
use editor_core::prelude::*;

/// The editor's own panels (hierarchy, inspector) — registered through the same
/// front door as any feature crate's panels.
struct EditorUiFeature;

impl EditorFeature for EditorUiFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("editor-ui", "Editor UI")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.panel(PanelDecl {
            id: PanelId::new_static("hierarchy"),
            title: "Hierarchy",
            placement: Placement::Left,
            context: ContextId::new_static("hierarchy"),
            content: PanelContent::Custom,
            default_open: true,
        });
        // Hierarchy focus-context actions (keymap doc): hidden from the palette
        // (panel-scoped keys, discoverable via which-key while focused).
        for (id, name, binding) in [
            ("hierarchy.down", "Row Down", "j"),
            ("hierarchy.up", "Row Up", "k"),
            ("hierarchy.top", "Top", "g g"),
            ("hierarchy.bottom", "Bottom", "shift+g"),
            ("hierarchy.select", "Select Row", "enter"),
            ("hierarchy.fold", "Fold / To Parent", "h"),
            ("hierarchy.unfold", "Unfold", "l"),
            (
                "hierarchy.reparent-in",
                "Reparent Into Sibling",
                "shift+period",
            ),
            (
                "hierarchy.reparent-out",
                "Reparent To Grandparent",
                "shift+comma",
            ),
        ] {
            reg.action(
                ActionDef::new(id, name)
                    .context("hierarchy")
                    .bind(binding)
                    .hidden(),
            );
        }
        reg.panel(PanelDecl {
            id: PanelId::new_static("inspector"),
            title: "Inspector",
            placement: Placement::Right,
            context: ContextId::new_static("inspector"),
            content: PanelContent::Properties(PropertySource::Selection),
            default_open: true,
        });
    }
}

/// ONE writer for `KeyCapture` (flow-audit: competing writers): the resolver stands
/// down exactly while an editable text field owns keyboard focus — the palette
/// input, inspector number fields, future rename boxes.
fn sync_key_capture(
    focus: Res<bevy::input_focus::InputFocus>,
    editable: Query<(), With<bevy::text::EditableText>>,
    mut capture: ResMut<KeyCapture>,
) {
    let captured = focus.get().is_some_and(|entity| editable.contains(entity));
    if capture.0 != captured {
        capture.0 = captured;
    }
}

pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "fonts/Inter-Regular.ttf");
        embedded_asset!(app, "fonts/Inter-Medium.ttf");
        embedded_asset!(app, "fonts/FiraCodeNerdFont-Regular.ttf");

        // User keymap overrides (rebind without recompiling). Convention: a game may
        // insert its own `KeymapPaths` before this plugin to relocate the file.
        if app.world().get_resource::<KeymapPaths>().is_none() {
            app.insert_resource(KeymapPaths {
                user: Some("editor-keymap.ron".into()),
            });
        }

        app.add_plugins(FeathersPlugins)
            .add_plugins(bevy::picking::mesh_picking::MeshPickingPlugin)
            .insert_resource({
                // Scrollbars are indicators, not focus (owner): drop the accent
                // thumb for a low-contrast neutral.
                let mut theme = UiTheme(create_dark_theme());
                theme.set_color("feathers.scrollbar.bg", Color::NONE);
                theme.set_color(
                    "feathers.scrollbar.thumb",
                    Color::srgba(1.0, 1.0, 1.0, 0.16),
                );
                theme.set_color(
                    "feathers.scrollbar.thumb.hover",
                    Color::srgba(1.0, 1.0, 1.0, 0.32),
                );
                theme
            });

        app.add_plugins((
            EditorCorePlugin,
            editor_scene::EditorScenePlugin,
            palette::PalettePlugin,
            editor_prefabs::EditorPrefabsPlugin,
            bevy_outliner::OutlinePlugin,
        ));
        app.add_editor_feature(EditorUiFeature);

        app.init_resource::<which_key::WhichKey>();
        app.init_resource::<probe_user::UserProbe>();
        app.init_resource::<palette_preview::PreviewSubject>();
        app.init_resource::<hierarchy::HierarchyState>();
        app.init_resource::<inspector::InspectorModel>();
        app.init_resource::<inspector::InspectorGroups>();
        app.insert_resource(inspector::default_overrides());
        if !app.is_plugin_added::<bevy::input_focus::tab_navigation::TabNavigationPlugin>() {
            app.add_plugins(bevy::input_focus::tab_navigation::TabNavigationPlugin);
        }
        app.add_systems(
            Update,
            hierarchy::handle_hierarchy_actions.in_set(editor_core::EditorSet::Tools),
        );
        app.init_resource::<statusbar::StatusFlash>();
        app.add_systems(
            Update,
            (
                dock::track_pointer_over_chrome,
                dock::sync_dock_chrome,
                dock::style_scrollbars,
                hierarchy::watch_hierarchy_inputs,
                hierarchy::watch_hierarchy_window,
                hierarchy::rebuild_hierarchy,
                hierarchy::scroll_cursor_into_view,
                inspector::watch_inspector_inputs,
                inspector::collect_inspector,
                inspector::render_inspector,
                inspector::stamp_tab_indices,
                inspector::probe_inspector.run_if(|| std::env::var("INSPECTOR_PROBE").is_ok()),
                sync_key_capture,
                ghost::apply_ghost_material,
                outline::ensure_outline_camera,
                outline::sync_selection_outlines,
                statusbar::collect_io_feedback,
                which_key::compute_which_key,
                statusbar::update_statusbar,
                which_key::rebuild_which_key,
            )
                .chain()
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            (
                prompt::attach_prompt_input,
                prompt::sync_prompt,
                prompt::close_prompt_on_escape,
                dock::sync_open_frame,
                palette_preview::sync_preview_content,
                palette_preview::inherit_preview_layer,
                palette_preview::turn_preview,
                probe_prefab::probe_prefab.run_if(|| std::env::var("PREFAB_PROBE").is_ok()),
                probe_user::probe_user.run_if(|| std::env::var("USER_PROBE").is_ok()),
            )
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Startup,
            (
                style::load_ui_fonts,
                ghost::init_ghost_material,
                statusbar::spawn_statusbar,
                which_key::spawn_which_key,
                prompt::spawn_prompt,
                palette_preview::setup_preview_rig,
                dock::spawn_docks,
                dock::spawn_open_frame,
                dock::attach_scrollbars,
            )
                .chain(),
        );
    }
}
