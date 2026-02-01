use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::editor::EditorMode;
use crate::scene::SceneEntity;
use crate::selection::Selected;
use crate::ui::theme::colors;

/// Resource to track find object palette state
#[derive(Resource)]
pub struct FindObjectState {
    pub open: bool,
    pub query: String,
    pub selected_index: usize,
    pub just_opened: bool,
}

impl Default for FindObjectState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected_index: 0,
            just_opened: false,
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

/// Open palette with F key, or / key when in Hierarchy mode
fn handle_find_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<FindObjectState>,
    editor_mode: Res<State<EditorMode>>,
    mut contexts: EguiContexts,
) {
    // Don't open if already open or UI wants keyboard input
    if state.open {
        return;
    }

    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    // F key works in any mode, "/" only works in Hierarchy mode
    let f_pressed = keyboard.just_pressed(KeyCode::KeyF);
    let slash_pressed = keyboard.just_pressed(KeyCode::Slash) && *editor_mode.get() == EditorMode::Hierarchy;

    if f_pressed || slash_pressed {
        state.open = true;
        state.query.clear();
        state.selected_index = 0;
        state.just_opened = true;
    }
}

/// Entry for a scene object
struct ObjectEntry {
    entity: Entity,
    name: String,
}

/// Get filtered and sorted objects based on query
fn filter_objects<'a>(objects: &'a [ObjectEntry], query: &str) -> Vec<(usize, &'a ObjectEntry, i64)> {
    let matcher = SkimMatcherV2::default();

    if query.is_empty() {
        return objects
            .iter()
            .enumerate()
            .map(|(idx, obj)| (idx, obj, 0i64))
            .collect();
    }

    let mut results: Vec<(usize, &ObjectEntry, i64)> = objects
        .iter()
        .enumerate()
        .filter_map(|(idx, obj)| {
            matcher.fuzzy_match(&obj.name, query)
                .map(|score| (idx, obj, score))
        })
        .collect();

    // Sort by score (higher is better)
    results.sort_by(|a, b| b.2.cmp(&a.2));
    results
}

/// Draw the find object palette
fn draw_find_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<FindObjectState>,
    mut commands: Commands,
    scene_objects: Query<(Entity, &Name), With<SceneEntity>>,
    selected_entities: Query<Entity, With<Selected>>,
) -> Result {
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

    let filtered = filter_objects(&objects, &state.query);

    // Clamp selected index
    if !filtered.is_empty() {
        state.selected_index = state.selected_index.min(filtered.len() - 1);
    }

    let mut should_close = false;
    let mut entity_to_select: Option<Entity> = None;

    // Check for keyboard input before rendering UI
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    // Handle Enter to select object
    if enter_pressed {
        if let Some((_, obj, _)) = filtered.get(state.selected_index) {
            entity_to_select = Some(obj.entity);
            should_close = true;
        }
    }

    // Handle Escape to close
    if escape_pressed {
        should_close = true;
    }

    // Handle arrow keys for navigation
    if down_pressed && !filtered.is_empty() {
        state.selected_index = (state.selected_index + 1).min(filtered.len() - 1);
    }
    if up_pressed {
        state.selected_index = state.selected_index.saturating_sub(1);
    }

    egui::Window::new("Find Object")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 300.0])
        .show(ctx, |ui| {
            // Search input
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Search scene objects...")
                    .desired_width(f32::INFINITY)
            );

            // Focus the input when just opened
            if state.just_opened {
                response.request_focus();
                state.just_opened = false;
            }

            ui.separator();

            // Object list
            egui::ScrollArea::vertical()
                .max_height(250.0)
                .show(ui, |ui| {
                    if objects.is_empty() {
                        ui.label(egui::RichText::new("No objects in scene").color(colors::TEXT_MUTED));
                    } else if filtered.is_empty() {
                        ui.label(egui::RichText::new("No matching objects").color(colors::TEXT_MUTED));
                    } else {
                        for (display_idx, (_, obj, _)) in filtered.iter().enumerate() {
                            let is_selected = display_idx == state.selected_index;
                            let text_color = if is_selected {
                                colors::TEXT_PRIMARY
                            } else {
                                colors::TEXT_SECONDARY
                            };

                            let response = ui.selectable_label(
                                is_selected,
                                egui::RichText::new(&obj.name).color(text_color),
                            );

                            if response.clicked() {
                                entity_to_select = Some(obj.entity);
                                should_close = true;
                            }

                            if is_selected {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                    }
                });

            ui.separator();

            // Help text and count
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Enter").small().strong().color(colors::ACCENT_BLUE));
                ui.label(egui::RichText::new("to select").small().color(colors::TEXT_MUTED));
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Esc").small().strong().color(colors::ACCENT_BLUE));
                ui.label(egui::RichText::new("to close").small().color(colors::TEXT_MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("{} objects", objects.len())).small().color(colors::TEXT_MUTED));
                });
            });
        });

    // Handle selection after UI
    if let Some(entity) = entity_to_select {
        // Deselect all currently selected
        for selected in selected_entities.iter() {
            commands.entity(selected).remove::<Selected>();
        }
        // Select the new entity
        commands.entity(entity).insert(Selected);
    }

    if should_close {
        state.open = false;
    }

    Ok(())
}
