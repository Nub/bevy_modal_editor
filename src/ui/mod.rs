mod ai_editor;
mod camera_settings;
mod command_palette;
mod component_browser;
mod edit_info;
mod effect_editor;
pub mod fuzzy_palette;
pub mod gltf_preview;
mod hierarchy;
pub mod insert_preview;
pub mod preview_common;
mod inspector;
mod marks;
mod material_editor;
pub mod material_preview;
mod mesh_model_panel;
mod panels;
mod particle_editor;
mod reflect_editor;
mod settings;
pub mod theme;
mod toolbar;
mod uv_editor_panel;
pub mod validation;
mod view_gizmo;

pub use command_palette::*;
pub use component_browser::*;
pub use edit_info::*;
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
#[derive(Resource)]
pub struct InspectorPanelState {
    pub width: f32,
    /// Whether we need to take a snapshot before the next inspector edit.
    /// Resets to true when no changes are detected, so continuous drags
    /// only produce one undo snapshot.
    pub needs_snapshot: bool,
}

impl Default for InspectorPanelState {
    fn default() -> Self {
        Self {
            width: 0.0,
            needs_snapshot: true,
        }
    }
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
                material_editor::MaterialEditorPlugin,
                mesh_model_panel::MeshModelPanelPlugin,
                camera_settings::CameraSettingsPlugin,
                material_preview::MaterialPreviewPlugin,
                particle_editor::ParticleEditorPlugin,
                effect_editor::EffectEditorPlugin,
                ai_editor::AIEditorPlugin,
                insert_preview::InsertPreviewPlugin,
                gltf_preview::GltfPreviewPlugin,
                ToolbarPlugin,
                ViewGizmoPlugin,
                EditInfoPlugin,
            ))
            // Popups and dialogs - modal UI elements
            .add_plugins((
                CommandPalettePlugin,
                ComponentBrowserPlugin,
                MarksPlugin,
                uv_editor_panel::UvEditorPlugin,
            ))
            // Validation
            .add_plugins(validation::ValidationPlugin);
    }
}
