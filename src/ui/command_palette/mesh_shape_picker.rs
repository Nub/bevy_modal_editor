//! Mesh shape picker palette — browse primitive shapes and mesh library entries
//! for the VFX mesh particle config.

use bevy::prelude::*;
use bevy_editor_game::MeshLibrary;
use bevy_egui::egui;
use bevy_vfx::data::MeshShape;

use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::theme::colors;

use super::CommandPaletteState;

/// Result of the mesh shape picker palette.
#[derive(Resource, Default)]
pub struct MeshShapePickResult(pub Option<MeshShape>);

struct ShapeItem {
    label: String,
    shape: MeshShape,
    category: &'static str,
}

impl PaletteItem for ShapeItem {
    fn label(&self) -> &str {
        &self.label
    }

    fn category(&self) -> Option<&str> {
        Some(self.category)
    }
}

pub(super) fn draw_mesh_shape_picker_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    mesh_library: &Res<MeshLibrary>,
    pick_result: &mut ResMut<MeshShapePickResult>,
) -> Result {
    let mut palette_state = PaletteState::from_bridge(
        std::mem::take(&mut state.query),
        state.selected_index,
        state.just_opened,
    );

    // Build items: built-in shapes first, then library meshes
    let mut items: Vec<ShapeItem> = MeshShape::BUILTIN
        .iter()
        .map(|s| ShapeItem {
            label: s.label().to_string(),
            shape: s.clone(),
            category: "Primitives",
        })
        .collect();

    let mut lib_names: Vec<String> = mesh_library.meshes.keys().cloned().collect();
    lib_names.sort();
    items.extend(lib_names.into_iter().map(|name| {
        let display = name.rsplit("::").next().unwrap_or(&name).to_string();
        ShapeItem {
            label: display,
            shape: MeshShape::Custom(name),
            category: "Library",
        }
    }));

    let config = PaletteConfig {
        title: "MESH SHAPE",
        title_color: colors::ACCENT_PURPLE,
        subtitle: "Particle mesh",
        hint_text: "Type to search shapes...",
        action_label: "select",
        size: [340.0, 340.0],
        show_categories: true,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, config);

    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            pick_result.0 = Some(items[index].shape.clone());
            state.open = false;
        }
        PaletteResult::Closed => {
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}
