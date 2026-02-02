mod command_palette;
mod component_browser;
mod edit_info;
mod file_dialog;
mod find_object;
pub mod fuzzy_palette;
mod hierarchy;
mod inspector;
mod marks;
mod panels;
mod reflect_editor;
mod settings;
mod theme;
mod toolbar;
mod view_gizmo;

pub use command_palette::*;
pub use component_browser::*;
pub use edit_info::*;
pub use file_dialog::*;
pub use find_object::*;
pub use fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, CategorizedItem, FilteredItem, KeywordItem, PaletteConfig,
    PaletteItem, PaletteResult, PaletteState, SimpleItem,
};
pub use hierarchy::*;
pub use inspector::*;
pub use marks::*;
pub use panels::*;
pub use reflect_editor::*;
pub use settings::*;
pub use theme::*;
pub use toolbar::*;
pub use view_gizmo::*;

use bevy::prelude::*;

/// Resource tracking the current width of the right inspector panel
#[derive(Resource, Default)]
pub struct InspectorPanelState {
    pub width: f32,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectorPanelState>()
            .init_resource::<ImmutableComponentCache>()
            // Core UI - theming and global settings
            .add_plugins((ThemePlugin, SettingsPlugin))
            // Main panels - persistent UI areas
            .add_plugins((
                PanelsPlugin,
                HierarchyPlugin,
                InspectorPlugin,
                ToolbarPlugin,
                ViewGizmoPlugin,
                EditInfoPlugin,
            ))
            // Popups and dialogs - modal UI elements
            .add_plugins((
                CommandPalettePlugin,
                FindObjectPlugin,
                ComponentBrowserPlugin,
                FileDialogPlugin,
                MarksPlugin,
            ));
    }
}
