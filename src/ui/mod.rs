mod command_palette;
mod edit_info;
mod find_object;
mod hierarchy;
mod inspector;
mod marks;
mod panels;
mod settings;
mod toolbar;
mod view_gizmo;

pub use command_palette::*;
pub use edit_info::*;
pub use find_object::*;
pub use hierarchy::*;
pub use inspector::*;
pub use marks::*;
pub use panels::*;
pub use settings::*;
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
            .add_plugins(SettingsPlugin)
            .add_plugins(PanelsPlugin)
            .add_plugins(HierarchyPlugin)
            .add_plugins(InspectorPlugin)
            .add_plugins(ToolbarPlugin)
            .add_plugins(ViewGizmoPlugin)
            .add_plugins(EditInfoPlugin)
            .add_plugins(MarksPlugin)
            .add_plugins(CommandPalettePlugin)
            .add_plugins(FindObjectPlugin);
    }
}
