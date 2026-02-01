use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use super::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteItem, PaletteResult, PaletteState};
use super::theme::colors;
use crate::editor::{EditorMode, EditorState};
use crate::scene::SceneEntity;
use crate::selection::Selected;

/// Resource to track find object palette state
#[derive(Resource)]
pub struct FindObjectState {
    pub open: bool,
    pub palette_state: PaletteState,
}

impl Default for FindObjectState {
    fn default() -> Self {
        Self {
            open: false,
            palette_state: PaletteState::default(),
        }
    }
}

pub struct FindObjectPlugin;

impl Plugin for FindObjectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FindObjectState>()
            .add_systems(Update, handle_find_toggle)
            .add_systems(EguiPrimaryContextPass, draw_find_palette);
    }
}

/// Open palette with F key (not in Hierarchy mode), or / key in Hierarchy mode
fn handle_find_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<FindObjectState>,
    editor_mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    // Don't open when editor is disabled
    if !editor_state.editor_active {
        return;
    }

    // Don't open if already open or UI wants keyboard input
    if state.open {
        return;
    }

    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    let in_hierarchy = *editor_mode.get() == EditorMode::Hierarchy;

    // F key opens find palette (not in Hierarchy mode where F is used for inline filtering)
    // "/" key opens find palette only in Hierarchy mode
    let f_pressed = keyboard.just_pressed(KeyCode::KeyF) && !in_hierarchy;
    let slash_pressed = keyboard.just_pressed(KeyCode::Slash) && in_hierarchy;

    if f_pressed || slash_pressed {
        state.open = true;
        state.palette_state.reset();
    }
}

/// Entry for a scene object that implements PaletteItem
struct ObjectEntry {
    entity: Entity,
    name: String,
}

impl PaletteItem for ObjectEntry {
    fn label(&self) -> &str {
        &self.name
    }
}

/// Draw the find object palette
fn draw_find_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<FindObjectState>,
    mut commands: Commands,
    scene_objects: Query<(Entity, &Name), With<SceneEntity>>,
    selected_entities: Query<Entity, With<Selected>>,
    editor_state: Res<EditorState>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    if !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Build list of scene objects
    let objects: Vec<ObjectEntry> = scene_objects
        .iter()
        .map(|(entity, name)| ObjectEntry {
            entity,
            name: name.as_str().to_string(),
        })
        .collect();

    // Handle empty scene
    if objects.is_empty() {
        // Draw a simple message window
        egui::Window::new("Find Object")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([400.0, 100.0])
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No objects in scene")
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                });
                ui.add_space(20.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Esc")
                            .small()
                            .strong()
                            .color(colors::ACCENT_BLUE),
                    );
                    ui.label(
                        egui::RichText::new("to close")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.open = false;
        }
        return Ok(());
    }

    let config = PaletteConfig {
        title: "FIND OBJECT",
        title_color: colors::ACCENT_CYAN,
        subtitle: "Search scene objects",
        hint_text: "Type to search...",
        action_label: "select",
        size: [400.0, 300.0],
        show_categories: false,
    };

    match draw_fuzzy_palette(ctx, &mut state.palette_state, &objects, &config) {
        PaletteResult::Selected(index) => {
            if let Some(obj) = objects.get(index) {
                // Deselect all currently selected
                for selected in selected_entities.iter() {
                    commands.entity(selected).remove::<Selected>();
                }
                // Select the new entity
                commands.entity(obj.entity).insert(Selected);
            }
            state.open = false;
        }
        PaletteResult::Closed => {
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}
