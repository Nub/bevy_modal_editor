use bevy::prelude::*;
use bevy_editor_game::{MaterialLibrary, MaterialRef};
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use super::fuzzy_palette::{fuzzy_filter, PaletteItem, PaletteState};
use super::material_preview::PresetPreviewState;
use super::theme::colors;
use crate::editor::{EditorMode, EditorState};
use crate::materials::{apply_material_def_standalone, remove_all_material_components};
use crate::selection::Selected;
use crate::utils::should_process_input;

/// Resource tracking state of the material preset search palette.
#[derive(Resource)]
pub struct MaterialPresetPaletteState {
    pub open: bool,
    pub palette_state: PaletteState,
    /// Name of the previously previewed preset, for change detection.
    prev_previewed_name: Option<String>,
}

impl Default for MaterialPresetPaletteState {
    fn default() -> Self {
        Self {
            open: false,
            palette_state: PaletteState::default(),
            prev_previewed_name: None,
        }
    }
}

/// A library preset entry for fuzzy filtering.
struct PresetItem {
    name: String,
}

impl PaletteItem for PresetItem {
    fn label(&self) -> &str {
        &self.name
    }
}

pub struct MaterialPresetPalettePlugin;

impl Plugin for MaterialPresetPalettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialPresetPaletteState>()
            .add_systems(Update, handle_preset_palette_toggle)
            .add_systems(EguiPrimaryContextPass, draw_material_preset_palette);
    }
}

/// Open the preset palette with F in Material mode.
fn handle_preset_palette_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<MaterialPresetPaletteState>,
    editor_mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    if state.open {
        return;
    }
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }
    if *editor_mode.get() != EditorMode::Material {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyF) {
        state.open = true;
        state.palette_state.reset();
        state.prev_previewed_name = None;
    }
}

/// Draw the two-column material preset palette.
fn draw_material_preset_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<MaterialPresetPaletteState>,
    mut preview_state: ResMut<PresetPreviewState>,
    editor_state: Res<EditorState>,
    editor_mode: Res<State<EditorMode>>,
    library: Res<MaterialLibrary>,
    selected_entities: Query<Entity, With<Selected>>,
    mut commands: Commands,
) -> Result {
    if !editor_state.ui_enabled || !state.open {
        return Ok(());
    }

    // Close if mode changed away from Material
    if *editor_mode.get() != EditorMode::Material {
        state.open = false;
        preview_state.current_def = None;
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Build sorted list of library presets
    let mut names: Vec<String> = library.materials.keys().cloned().collect();
    names.sort();
    let items: Vec<PresetItem> = names.iter().map(|n| PresetItem { name: n.clone() }).collect();

    // Filter
    let filtered = fuzzy_filter(&items, &state.palette_state.query);

    // Clamp selected index
    if !filtered.is_empty() {
        state.palette_state.selected_index =
            state.palette_state.selected_index.min(filtered.len() - 1);
    }

    // Keyboard input
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    if escape_pressed {
        state.open = false;
        preview_state.current_def = None;
        return Ok(());
    }

    if enter_pressed && !filtered.is_empty() {
        if let Some(fi) = filtered.get(state.palette_state.selected_index) {
            let preset_name = fi.item.name.clone();
            if let Some(def) = library.materials.get(&preset_name) {
                let def = def.clone();
                let entities: Vec<Entity> = selected_entities.iter().collect();
                commands.queue(move |world: &mut World| {
                    for entity in &entities {
                        remove_all_material_components(world, *entity);
                        if let Ok(mut e) = world.get_entity_mut(*entity) {
                            e.insert(MaterialRef::Library(preset_name.clone()));
                        }
                        apply_material_def_standalone(world, *entity, &def);
                    }
                });
            }
            state.open = false;
            preview_state.current_def = None;
            return Ok(());
        }
    }

    if down_pressed && !filtered.is_empty() {
        state.palette_state.selected_index =
            (state.palette_state.selected_index + 1).min(filtered.len() - 1);
    }
    if up_pressed {
        state.palette_state.selected_index = state.palette_state.selected_index.saturating_sub(1);
    }

    // Update preview override when highlighted item changes
    let current_name = filtered
        .get(state.palette_state.selected_index)
        .map(|fi| fi.item.name.clone());
    if current_name != state.prev_previewed_name {
        state.prev_previewed_name = current_name.clone();
        preview_state.current_def = current_name
            .as_ref()
            .and_then(|n| library.materials.get(n))
            .cloned();
    }

    let preview_texture_id = preview_state.egui_texture_id;

    // Draw window
    let mut click_result: Option<String> = None;
    egui::Window::new("material_preset_palette")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([580.0, 340.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // -- Full-width header: mode indicator + search --
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("PRESETS")
                        .small()
                        .strong()
                        .color(colors::ACCENT_PURPLE),
                );
                ui.label(
                    egui::RichText::new("- Material library")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            });
            ui.add_space(4.0);

            let response = ui.add(
                egui::TextEdit::singleline(&mut state.palette_state.query)
                    .hint_text("Type to search presets...")
                    .desired_width(f32::INFINITY),
            );
            if state.palette_state.just_opened {
                response.request_focus();
                state.palette_state.just_opened = false;
            }

            ui.separator();

            // -- Two-column middle: list (left) + preview (right) --
            let footer_reserve = 28.0;
            let middle_height = (ui.available_height() - footer_reserve).max(0.0);
            let middle_width = ui.available_width();
            let right_width = 230.0;
            let sep_width = 8.0;
            let left_width = (middle_width - right_width - sep_width).max(0.0);

            ui.allocate_ui(egui::vec2(middle_width, middle_height), |ui| {
                ui.horizontal_top(|ui| {
                    // Left: scrollable item list
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_width, middle_height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                ui.set_min_width(left_width);
                                if filtered.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No matches found")
                                            .color(colors::TEXT_MUTED)
                                            .italics(),
                                    );
                                } else {
                                    for (display_idx, fi) in filtered.iter().enumerate() {
                                        let is_selected =
                                            display_idx == state.palette_state.selected_index;
                                        let text_color = if is_selected {
                                            colors::TEXT_PRIMARY
                                        } else {
                                            colors::TEXT_SECONDARY
                                        };

                                        let response = ui.selectable_label(
                                            is_selected,
                                            egui::RichText::new(fi.item.label())
                                                .color(text_color),
                                        );

                                        if response.clicked() {
                                            click_result = Some(fi.item.name.clone());
                                        }

                                        if is_selected {
                                            response.scroll_to_me(Some(egui::Align::Center));
                                        }
                                    }
                                }
                            });
                    },
                    );

                    ui.separator();

                    // Right: preview
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Preview")
                                .small()
                                .strong()
                                .color(colors::TEXT_SECONDARY),
                        );
                        ui.add_space(4.0);

                        if let Some(tex_id) = preview_texture_id {
                            let size = ui.available_width().min(220.0);
                            ui.image(egui::load::SizedTexture::new(tex_id, [size, size]));
                        } else {
                            ui.label(
                                egui::RichText::new("Preview loading...")
                                    .color(colors::TEXT_MUTED)
                                    .italics(),
                            );
                        }

                        if let Some(ref name) = state.prev_previewed_name {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(name)
                                    .color(colors::TEXT_PRIMARY)
                                    .strong(),
                            );
                        }
                    });
                });
            });

            // -- Full-width footer --
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Enter")
                        .small()
                        .strong()
                        .color(colors::ACCENT_BLUE),
                );
                ui.label(
                    egui::RichText::new("to apply")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(10.0);
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

    // Handle click selection
    if let Some(preset_name) = click_result {
        if let Some(def) = library.materials.get(&preset_name) {
            let def = def.clone();
            let entities: Vec<Entity> = selected_entities.iter().collect();
            commands.queue(move |world: &mut World| {
                for entity in &entities {
                    remove_all_material_components(world, *entity);
                    if let Ok(mut e) = world.get_entity_mut(*entity) {
                        e.insert(MaterialRef::Library(preset_name.clone()));
                    }
                    apply_material_def_standalone(world, *entity, &def);
                }
            });
        }
        state.open = false;
        preview_state.current_def = None;
    }

    Ok(())
}
