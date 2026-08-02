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
mod outline;
mod palette;
mod statusbar;
mod which_key;

pub use palette::{PaletteFilter, PaletteState};

use bevy::asset::embedded_asset;
use bevy::feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins};
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
            ("hierarchy.reparent-in", "Reparent Into Sibling", "shift+period"),
            ("hierarchy.reparent-out", "Reparent To Grandparent", "shift+comma"),
        ] {
            reg.action(
                ActionDef::new(id, name).context("hierarchy").bind(binding).hidden(),
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

pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "fonts/Inter-Regular.ttf");
        embedded_asset!(app, "fonts/Inter-Medium.ttf");
        embedded_asset!(app, "fonts/FiraCodeNerdFont-Regular.ttf");

        // User keymap overrides (rebind without recompiling). Convention: a game may
        // insert its own `KeymapPaths` before this plugin to relocate the file.
        if app.world().get_resource::<KeymapPaths>().is_none() {
            app.insert_resource(KeymapPaths { user: Some("editor-keymap.ron".into()) });
        }

        app.add_plugins(FeathersPlugins)
            .add_plugins(bevy::picking::mesh_picking::MeshPickingPlugin)
            .insert_resource(UiTheme(create_dark_theme()));

        app.add_plugins((
            EditorCorePlugin,
            editor_scene::EditorScenePlugin,
            palette::PalettePlugin,
            bevy_outliner::OutlinePlugin,
        ));
        app.add_editor_feature(EditorUiFeature);

        app.init_resource::<which_key::WhichKey>();
        app.init_resource::<hierarchy::HierarchyState>();
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
                hierarchy::watch_hierarchy_inputs,
                hierarchy::rebuild_hierarchy,
                hierarchy::scroll_cursor_into_view,
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
            Startup,
            (
                style::load_ui_fonts,
                ghost::init_ghost_material,
                statusbar::spawn_statusbar,
                which_key::spawn_which_key,
                dock::spawn_docks,
            )
                .chain(),
        );
    }
}
