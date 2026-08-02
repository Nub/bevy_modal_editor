//! Editor chrome (spec §2 `editor_ui`): the canonical editor UI every game gets by
//! depending on this crate — command palette, status bar, which-key popup, and the
//! style system. Games contribute *content* (kinds, components, features) through
//! `editor_api`; they never own chrome.
//!
//! `EditorUiPlugin` is the single editor entry point for a game binary: it brings
//! the kernel (`editor_core`), scene I/O (`editor_scene`), feathers theming, and
//! all chrome surfaces. Fonts ship embedded — games don't copy asset files.

pub mod style;

mod ghost;
mod palette;
mod statusbar;
mod which_key;

pub use palette::{PaletteFilter, PaletteState};

use bevy::asset::embedded_asset;
use bevy::feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins};
use bevy::prelude::*;
use editor_core::prelude::*;

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
        ));

        app.init_resource::<which_key::WhichKey>();
        app.init_resource::<statusbar::StatusFlash>();
        app.add_systems(
            Update,
            (
                ghost::apply_ghost_material,
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
            )
                .chain(),
        );
    }
}
