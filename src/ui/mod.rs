mod command_palette;
mod component_browser;
mod edit_info;
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
            .add_plugins(ThemePlugin)
            .add_plugins(SettingsPlugin)
            .add_plugins(PanelsPlugin)
            .add_plugins(HierarchyPlugin)
            .add_plugins(InspectorPlugin)
            .add_plugins(ToolbarPlugin)
            .add_plugins(ViewGizmoPlugin)
            .add_plugins(EditInfoPlugin)
            .add_plugins(MarksPlugin)
            .add_plugins(CommandPalettePlugin)
            .add_plugins(FindObjectPlugin)
            .add_plugins(ComponentBrowserPlugin);
    }
}
