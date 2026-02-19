//! Effect editor panel — card-based rule list for editing effect sequences.
//!
//! Single right-side panel with:
//! - Header: entity name, playback controls, preset buttons
//! - Scrollable card list of rules (each rule = trigger + actions)
//! - Mini timeline strip at bottom for time-triggered rules

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::effects::data::*;
use crate::effects::EffectLibrary;
use crate::particles::ParticleLibrary;
use crate::scene::PrimitiveShape;
use crate::selection::Selected;
use crate::ui::command_palette::{
    CommandPaletteState, GltfPickResult, TexturePickResult, TextureSlot,
};
use crate::ui::theme::{
    colors, draw_pin_button, grid_label, panel, panel_frame, section_header,
};

// ---------------------------------------------------------------------------
// Plugin + State
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct EffectEditorState {
    /// Which rule card is currently expanded for editing.
    pub expanded_rule: Option<usize>,
}

pub struct EffectEditorPlugin;

impl Plugin for EffectEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EffectEditorState>()
            .add_systems(EguiPrimaryContextPass, draw_effect_panel);
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const CARD_ROUNDING: u8 = 4;
const ACCENT_STRIPE_WIDTH: f32 = 3.0;

const MINI_TIMELINE_HEIGHT: f32 = 24.0;
const PLAYHEAD_WIDTH: f32 = 2.0;
const MINI_DOT_RADIUS: f32 = 4.0;

// ---------------------------------------------------------------------------
// Trigger accent colors
// ---------------------------------------------------------------------------

fn trigger_accent(trigger: &EffectTrigger) -> egui::Color32 {
    match trigger {
        EffectTrigger::AtTime(_) => colors::ACCENT_GREEN,
        EffectTrigger::OnCollision { .. } => colors::ACCENT_ORANGE,
        EffectTrigger::OnEffectEvent(_) => colors::ACCENT_BLUE,
        EffectTrigger::AfterRule { .. } => colors::STATUS_WARNING,
        EffectTrigger::RepeatingInterval { .. } => colors::ACCENT_CYAN,
        EffectTrigger::OnSpawn => colors::ACCENT_PURPLE,
        EffectTrigger::AfterIdleTimeout { .. } => colors::ACCENT_CYAN,
    }
}

fn trigger_summary(trigger: &EffectTrigger) -> String {
    match trigger {
        EffectTrigger::AtTime(t) => format!("at {:.1}s", t),
        EffectTrigger::OnCollision { tag } => {
            if tag.is_empty() {
                "on collision".into()
            } else {
                format!("on collision ({})", tag)
            }
        }
        EffectTrigger::OnEffectEvent(name) => {
            if name.is_empty() {
                "on event".into()
            } else {
                format!("on event \"{}\"", name)
            }
        }
        EffectTrigger::AfterRule { source_rule, delay } => {
            if *delay > 0.0 {
                format!("after \"{}\" +{:.1}s", source_rule, delay)
            } else {
                format!("after \"{}\"", source_rule)
            }
        }
        EffectTrigger::RepeatingInterval { interval, max_count } => {
            if let Some(max) = max_count {
                format!("every {:.1}s (x{})", interval, max)
            } else {
                format!("every {:.1}s", interval)
            }
        }
        EffectTrigger::OnSpawn => "on spawn".into(),
        EffectTrigger::AfterIdleTimeout { timeout } => format!("idle {:.1}s", timeout),
    }
}

/// Color accent for an action by category.
fn action_accent(action: &EffectAction) -> egui::Color32 {
    match action {
        EffectAction::SpawnPrimitive { .. }
        | EffectAction::SpawnParticle { .. }
        | EffectAction::SpawnGltf { .. }
        | EffectAction::SpawnDecal { .. }
        | EffectAction::SpawnEffect { .. } => colors::ACCENT_GREEN,
        EffectAction::SetVelocity { .. }
        | EffectAction::ApplyImpulse { .. }
        | EffectAction::SetGravity { .. } => colors::ACCENT_ORANGE,
        EffectAction::Despawn { .. } | EffectAction::EmitEvent(_) => colors::ACCENT_BLUE,
        EffectAction::TweenValue { .. } => colors::ACCENT_PURPLE,
        EffectAction::InsertComponent { .. } | EffectAction::RemoveComponent { .. } => {
            colors::ACCENT_CYAN
        }
    }
}

// ---------------------------------------------------------------------------
// Action card (modifier_card style matching particle editor)
// ---------------------------------------------------------------------------

fn action_card(
    ui: &mut egui::Ui,
    label: &str,
    accent: egui::Color32,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut removed = false;

    let frame = egui::Frame::new()
        .fill(colors::BG_MEDIUM)
        .corner_radius(egui::CornerRadius::same(CARD_ROUNDING))
        .inner_margin(egui::Margin::same(6));

    let resp = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).strong().color(colors::TEXT_PRIMARY));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("\u{00d7}").color(colors::STATUS_ERROR),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Remove action")
                    .clicked()
                {
                    removed = true;
                }
            });
        });
        ui.add_space(2.0);
        body(ui);
    });

    // Paint accent stripe over left edge
    let card_rect = resp.response.rect;
    let stripe = egui::Rect::from_min_max(
        card_rect.left_top(),
        egui::pos2(
            card_rect.left() + ACCENT_STRIPE_WIDTH,
            card_rect.bottom(),
        ),
    );
    ui.painter().rect_filled(
        stripe,
        egui::CornerRadius {
            nw: CARD_ROUNDING,
            sw: CARD_ROUNDING,
            ne: 0,
            se: 0,
        },
        accent,
    );

    ui.add_space(4.0);
    removed
}

// ---------------------------------------------------------------------------
// Category header for action groups
// ---------------------------------------------------------------------------

fn action_category_header(
    ui: &mut egui::Ui,
    label: &str,
    accent: egui::Color32,
    options: &[(&str, usize)],
    actions: &mut Vec<EffectAction>,
) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .strong()
                .size(12.0)
                .color(accent),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(egui::RichText::new("+").strong().color(accent), |ui| {
                for (name, variant_idx) in options {
                    if ui.button(*name).clicked() {
                        actions.push(EffectAction::from_variant_index(*variant_idx));
                        ui.close();
                    }
                }
            });
        });
    });
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Tag helpers
// ---------------------------------------------------------------------------

fn collect_defined_tags(marker: &EffectMarker) -> Vec<String> {
    let mut tags = Vec::new();
    for step in &marker.steps {
        for action in &step.actions {
            let tag = match action {
                EffectAction::SpawnPrimitive { tag, .. }
                | EffectAction::SpawnParticle { tag, .. }
                | EffectAction::SpawnGltf { tag, .. }
                | EffectAction::SpawnDecal { tag, .. }
                | EffectAction::SpawnEffect { tag, .. } => tag,
                _ => continue,
            };
            if !tag.is_empty() && !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

fn tag_combo(ui: &mut egui::Ui, id: &str, tag: &mut String, defined_tags: &[String]) {
    if defined_tags.is_empty() {
        ui.add(egui::TextEdit::singleline(tag).desired_width(100.0));
    } else {
        let display = if tag.is_empty() {
            "(none)"
        } else {
            tag.as_str()
        };
        egui::ComboBox::from_id_salt(id)
            .selected_text(display)
            .width(100.0)
            .show_ui(ui, |ui| {
                for t in defined_tags {
                    ui.selectable_value(tag, t.clone(), t.as_str());
                }
            });
    }
}

fn collect_rule_names(marker: &EffectMarker) -> Vec<String> {
    marker
        .steps
        .iter()
        .filter(|s| !s.name.is_empty())
        .map(|s| s.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Main panel system
// ---------------------------------------------------------------------------

fn draw_effect_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    let is_pinned = world
        .resource::<PinnedWindows>()
        .0
        .contains(&EditorMode::Effect);
    if current_mode != EditorMode::Effect && !is_pinned {
        return;
    }

    // Get the single selected entity with an EffectMarker
    let entity = {
        let mut q = world.query_filtered::<Entity, (With<Selected>, With<EffectMarker>)>();
        match q.iter(world).next() {
            Some(e) => e,
            None => {
                draw_empty_panel(world, is_pinned, current_mode);
                return;
            }
        }
    };

    // Clone data for editing
    let mut marker = world.get::<EffectMarker>(entity).unwrap().clone();
    let original = marker.clone();

    let mut entity_name = world
        .get::<Name>(entity)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();
    let original_name = entity_name.clone();

    let playback_state = world
        .get::<EffectPlayback>(entity)
        .map(|p| p.state)
        .unwrap_or(PlaybackState::Stopped);
    let playback_elapsed = world
        .get::<EffectPlayback>(entity)
        .map(|p| p.elapsed)
        .unwrap_or(0.0);

    // Get egui context
    let ctx = {
        let Some(mut egui_ctx) = world
            .query::<&mut bevy_egui::EguiContext>()
            .iter_mut(world)
            .next()
        else {
            return;
        };
        egui_ctx.get_mut().clone()
    };

    // Extract editor state
    let mut state = world
        .remove_resource::<EffectEditorState>()
        .unwrap_or_default();

    // Bounds-check expanded_rule
    if let Some(idx) = state.expanded_rule {
        if idx >= marker.steps.len() {
            state.expanded_rule = None;
        }
    }

    let defined_tags = collect_defined_tags(&marker);
    let rule_names = collect_rule_names(&marker);

    // Collect particle preset names
    let mut particle_presets: Vec<String> = world
        .get_resource::<ParticleLibrary>()
        .map(|lib| lib.effects.keys().cloned().collect())
        .unwrap_or_default();
    particle_presets.sort();

    // Collect effect preset names
    let mut effect_presets: Vec<String> = world
        .get_resource::<EffectLibrary>()
        .map(|lib| lib.effects.keys().cloned().collect())
        .unwrap_or_default();
    effect_presets.sort();

    // Collect deferred actions
    let mut pin_toggled = false;
    let mut save_preset_clicked = false;
    let mut browse_presets_clicked = false;
    let mut play_clicked = false;
    let mut pause_clicked = false;
    let mut stop_clicked = false;

    // Draw the single right-side panel
    draw_rules_panel(
        &ctx,
        &mut marker,
        &mut entity_name,
        &mut state,
        &defined_tags,
        &rule_names,
        &particle_presets,
        &effect_presets,
        playback_state,
        playback_elapsed,
        is_pinned,
        current_mode,
        &mut pin_toggled,
        &mut play_clicked,
        &mut pause_clicked,
        &mut stop_clicked,
        &mut save_preset_clicked,
        &mut browse_presets_clicked,
    );

    // Handle deferred texture browse request from SpawnDecal action
    let browse_decal_texture = ctx
        .memory(|m| m.data.get_temp::<bool>(egui::Id::new("effect_decal_browse")))
        == Some(true);
    if browse_decal_texture {
        ctx.memory_mut(|m| {
            m.data.remove::<bool>(egui::Id::new("effect_decal_browse"));
        });
        world
            .resource_mut::<CommandPaletteState>()
            .open_pick_texture(TextureSlot::EffectDecalTexture, Some(entity));
    }

    // Consume texture pick result for effect decal
    {
        let pick_data = world.resource_mut::<TexturePickResult>().0.take();
        if let Some(pick) = pick_data {
            if pick.slot == TextureSlot::EffectDecalTexture && pick.entity == Some(entity) {
                let mut applied = false;
                for step in &mut marker.steps {
                    for action in &mut step.actions {
                        if let EffectAction::SpawnDecal { texture_path, .. } = action {
                            *texture_path = pick.path.clone();
                            applied = true;
                            break;
                        }
                    }
                    if applied {
                        break;
                    }
                }
                if !applied {
                    world.resource_mut::<TexturePickResult>().0 = Some(pick);
                }
            } else {
                world.resource_mut::<TexturePickResult>().0 = Some(pick);
            }
        }
    }

    // Handle deferred GLTF browse request from SpawnGltf action
    let browse_gltf = ctx
        .memory(|m| m.data.get_temp::<bool>(egui::Id::new("effect_gltf_browse")))
        == Some(true);
    if browse_gltf {
        ctx.memory_mut(|m| {
            m.data.remove::<bool>(egui::Id::new("effect_gltf_browse"));
        });
        world
            .resource_mut::<CommandPaletteState>()
            .open_pick_gltf(Some(entity));
    }

    // Consume GLTF pick result for effect SpawnGltf
    {
        let pick_data = world.resource_mut::<GltfPickResult>().0.take();
        if let Some(pick) = pick_data {
            if pick.entity == Some(entity) {
                let mut applied = false;
                for step in &mut marker.steps {
                    for action in &mut step.actions {
                        if let EffectAction::SpawnGltf { path, .. } = action {
                            *path = pick.path.clone();
                            applied = true;
                            break;
                        }
                    }
                    if applied {
                        break;
                    }
                }
                if !applied {
                    world.resource_mut::<GltfPickResult>().0 = Some(pick);
                }
            } else {
                world.resource_mut::<GltfPickResult>().0 = Some(pick);
            }
        }
    }

    // Write back marker if changed
    let changed = ron::to_string(&marker).ok() != ron::to_string(&original).ok();
    if changed {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert(marker);
        }
    }

    // Apply name changes
    if entity_name != original_name {
        if let Some(mut name) = world.get_mut::<Name>(entity) {
            name.set(entity_name.clone());
        }
    }

    // Handle playback controls
    if play_clicked {
        if let Some(mut pb) = world.get_mut::<EffectPlayback>(entity) {
            pb.state = PlaybackState::Playing;
        }
    }
    if pause_clicked {
        if let Some(mut pb) = world.get_mut::<EffectPlayback>(entity) {
            pb.state = PlaybackState::Paused;
        }
    }
    if stop_clicked {
        let spawned: Vec<Entity> = world
            .get::<EffectPlayback>(entity)
            .map(|pb| pb.spawned.values().copied().collect())
            .unwrap_or_default();
        for child in spawned {
            world.despawn(child);
        }
        if let Some(mut pb) = world.get_mut::<EffectPlayback>(entity) {
            pb.elapsed = 0.0;
            pb.fired_steps.clear();
            pb.pending_events.clear();
            pb.last_collision_point = None;
            pb.spawned.clear();
            pb.rule_fire_times.clear();
            pb.last_fire_time = 0.0;
            pb.active_tweens.clear();
            pb.repeat_counts.clear();
            pb.state = PlaybackState::Stopped;
        }
    }

    // Toggle pin
    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::Effect) {
            pinned.0.insert(EditorMode::Effect);
        }
    }

    // Save preset
    if save_preset_clicked {
        let name_for_preset = entity_name.clone();
        let marker_for_save = world.get::<EffectMarker>(entity).cloned().unwrap_or_default();
        let mut library = world.resource_mut::<EffectLibrary>();
        let preset_name = {
            let base = if name_for_preset.is_empty() {
                "New Effect".to_string()
            } else {
                name_for_preset
            };
            if !library.effects.contains_key(&base) {
                base
            } else {
                let mut candidate = base.clone();
                for i in 2.. {
                    candidate = format!("{} {}", base, i);
                    if !library.effects.contains_key(&candidate) {
                        break;
                    }
                }
                candidate
            }
        };
        library
            .effects
            .insert(preset_name.clone(), marker_for_save);
        info!("Saved effect preset '{}'", preset_name);
    }

    // Browse presets
    if browse_presets_clicked {
        world
            .resource_mut::<CommandPaletteState>()
            .open_effect_preset();
    }

    // Reinsert editor state
    world.insert_resource(state);
}

// ---------------------------------------------------------------------------
// Empty panel (no effect entity selected)
// ---------------------------------------------------------------------------

fn draw_empty_panel(world: &mut World, is_pinned: bool, current_mode: EditorMode) {
    let ctx = {
        let Some(mut egui_ctx) = world
            .query::<&mut bevy_egui::EguiContext>()
            .iter_mut(world)
            .next()
        else {
            return;
        };
        egui_ctx.get_mut().clone()
    };

    let available_height = panel::available_height(&ctx);

    let displaced = is_pinned
        && current_mode != EditorMode::Effect
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (
            egui::Align2::LEFT_TOP,
            [panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
    } else {
        (
            egui::Align2::RIGHT_TOP,
            [-panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
    };

    let mut pin_toggled = false;

    egui::Window::new("Effect Rules")
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                pin_toggled = draw_pin_button(ui, is_pinned);
            });

            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Select an effect entity")
                        .color(colors::TEXT_MUTED)
                        .italics(),
                );
            });
        });

    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::Effect) {
            pinned.0.insert(EditorMode::Effect);
        }
    }
}

// ---------------------------------------------------------------------------
// Rules Panel (single right-side panel)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_rules_panel(
    ctx: &egui::Context,
    marker: &mut EffectMarker,
    entity_name: &mut String,
    state: &mut EffectEditorState,
    defined_tags: &[String],
    rule_names: &[String],
    particle_presets: &[String],
    effect_presets: &[String],
    playback_state: PlaybackState,
    playback_elapsed: f32,
    is_pinned: bool,
    current_mode: EditorMode,
    pin_toggled: &mut bool,
    play_clicked: &mut bool,
    pause_clicked: &mut bool,
    stop_clicked: &mut bool,
    save_preset_clicked: &mut bool,
    browse_presets_clicked: &mut bool,
) {
    let available_height = panel::available_height(ctx);

    let displaced = is_pinned
        && current_mode != EditorMode::Effect
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (
            egui::Align2::LEFT_TOP,
            [panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
    } else {
        (
            egui::Align2::RIGHT_TOP,
            [-panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
    };

    egui::Window::new("Effect Rules")
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(ctx, |ui| {
            // Pin button
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                *pin_toggled = draw_pin_button(ui, is_pinned);
            });

            // Header: entity name + playback controls
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(entity_name)
                        .font(egui::FontId::proportional(16.0))
                        .text_color(colors::TEXT_PRIMARY)
                        .margin(egui::vec2(8.0, 6.0)),
                );
            });

            ui.horizontal(|ui| {
                // Playback status
                let (status_label, status_color) = match playback_state {
                    PlaybackState::Playing => ("Playing", colors::STATUS_SUCCESS),
                    PlaybackState::Paused => ("Paused", colors::STATUS_WARNING),
                    PlaybackState::Stopped => ("Stopped", colors::TEXT_MUTED),
                };
                ui.label(
                    egui::RichText::new(format!("{} {:.1}s", status_label, playback_elapsed))
                        .small()
                        .color(status_color),
                );

                if ui
                    .button(
                        egui::RichText::new("\u{25b6}")
                            .small()
                            .color(colors::STATUS_SUCCESS),
                    )
                    .on_hover_text("Play")
                    .clicked()
                {
                    *play_clicked = true;
                }
                if ui
                    .button(
                        egui::RichText::new("\u{23f8}")
                            .small()
                            .color(colors::STATUS_WARNING),
                    )
                    .on_hover_text("Pause")
                    .clicked()
                {
                    *pause_clicked = true;
                }
                if ui
                    .button(
                        egui::RichText::new("\u{25a0}")
                            .small()
                            .color(colors::STATUS_ERROR),
                    )
                    .on_hover_text("Stop")
                    .clicked()
                {
                    *stop_clicked = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(
                            egui::RichText::new("Save Preset")
                                .small()
                                .color(colors::ACCENT_GREEN),
                        )
                        .on_hover_text("Save current effect as a named preset")
                        .clicked()
                    {
                        *save_preset_clicked = true;
                    }
                    if ui
                        .button(
                            egui::RichText::new("Browse")
                                .small()
                                .color(colors::ACCENT_ORANGE),
                        )
                        .on_hover_text("Browse effect presets (F)")
                        .clicked()
                    {
                        *browse_presets_clicked = true;
                    }
                });
            });

            ui.separator();

            // Scrollable card list
            let mini_timeline_space = MINI_TIMELINE_HEIGHT + 16.0;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height() - mini_timeline_space)
                .show(ui, |ui| {
                    ui.set_min_width(230.0);

                    let mut remove_step = None;

                    for step_idx in 0..marker.steps.len() {
                        let is_expanded = state.expanded_rule == Some(step_idx);
                        if draw_rule_card(
                            ui,
                            marker,
                            step_idx,
                            is_expanded,
                            state,
                            defined_tags,
                            rule_names,
                            particle_presets,
                            effect_presets,
                        ) {
                            remove_step = Some(step_idx);
                        }
                    }

                    if let Some(idx) = remove_step {
                        marker.steps.remove(idx);
                        if state.expanded_rule == Some(idx) {
                            state.expanded_rule = None;
                        } else if let Some(expanded) = state.expanded_rule {
                            if expanded > idx {
                                state.expanded_rule = Some(expanded - 1);
                            }
                        }
                    }

                    // Add rule button
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.menu_button(
                            egui::RichText::new("+ Add Rule")
                                .strong()
                                .color(colors::ACCENT_GREEN),
                            |ui| {
                                for (i, label) in
                                    EffectTrigger::VARIANT_LABELS.iter().enumerate()
                                {
                                    if ui.button(*label).clicked() {
                                        let new_idx = marker.steps.len();
                                        marker.steps.push(EffectStep {
                                            name: format!("rule_{}", new_idx + 1),
                                            trigger: EffectTrigger::from_variant_index(i),
                                            actions: Vec::new(),
                                        });
                                        state.expanded_rule = Some(new_idx);
                                        ui.close();
                                    }
                                }
                            },
                        );
                    });

                    ui.add_space(4.0);
                });

            // Mini timeline strip at bottom
            draw_mini_timeline(ui, marker, playback_state, playback_elapsed);
        });
}

// ---------------------------------------------------------------------------
// Rule card
// ---------------------------------------------------------------------------

/// Draw a single rule as a collapsible card. Returns true if removed.
#[allow(clippy::too_many_arguments)]
fn draw_rule_card(
    ui: &mut egui::Ui,
    marker: &mut EffectMarker,
    step_idx: usize,
    is_expanded: bool,
    state: &mut EffectEditorState,
    defined_tags: &[String],
    rule_names: &[String],
    particle_presets: &[String],
    effect_presets: &[String],
) -> bool {
    let mut removed = false;
    let step = &marker.steps[step_idx];
    let accent = trigger_accent(&step.trigger);
    let summary = trigger_summary(&step.trigger);
    let action_count = step.actions.len();
    let step_name = step.name.clone();

    let frame = egui::Frame::new()
        .fill(colors::BG_MEDIUM)
        .corner_radius(egui::CornerRadius::same(CARD_ROUNDING))
        .inner_margin(egui::Margin::same(6));

    let resp = frame.show(ui, |ui| {
        // Collapsed header: WHEN {summary} THEN {N actions}
        let header_resp = ui.horizontal(|ui| {
            // Name
            let name_display = if step_name.is_empty() {
                format!("Rule #{}", step_idx + 1)
            } else {
                step_name.clone()
            };
            ui.label(
                egui::RichText::new(name_display)
                    .strong()
                    .color(colors::TEXT_PRIMARY),
            );

            ui.label(
                egui::RichText::new(format!("WHEN {} THEN {} action(s)", summary, action_count))
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("\u{00d7}").color(colors::STATUS_ERROR),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Remove rule")
                    .clicked()
                {
                    removed = true;
                }

                // Expand/collapse toggle
                let toggle_text = if is_expanded { "\u{25b2}" } else { "\u{25bc}" };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(toggle_text).color(colors::TEXT_MUTED),
                        )
                        .frame(false),
                    )
                    .on_hover_text(if is_expanded { "Collapse" } else { "Expand" })
                    .clicked()
                {
                    if is_expanded {
                        state.expanded_rule = None;
                    } else {
                        state.expanded_rule = Some(step_idx);
                    }
                }
            });
        });

        // Click on header area to toggle
        if header_resp
            .response
            .interact(egui::Sense::click())
            .clicked()
            && !removed
        {
            if is_expanded {
                state.expanded_rule = None;
            } else {
                state.expanded_rule = Some(step_idx);
            }
        }

        // Expanded content
        if is_expanded && !removed {
            ui.separator();

            // Name editor
            let step = &mut marker.steps[step_idx];
            ui.horizontal(|ui| {
                grid_label(ui, "Name");
                ui.add(
                    egui::TextEdit::singleline(&mut step.name)
                        .desired_width(140.0)
                        .font(egui::FontId::proportional(13.0)),
                );
            });

            ui.add_space(4.0);

            // Trigger editor
            section_header(ui, "Trigger", true, |ui| {
                let step = &mut marker.steps[step_idx];
                draw_trigger_editor(ui, &mut step.trigger, step_idx, defined_tags, rule_names);
            });

            ui.add_space(4.0);

            // Actions editor
            section_header(ui, "Actions", true, |ui| {
                draw_actions_list(
                    ui,
                    marker,
                    step_idx,
                    defined_tags,
                    rule_names,
                    particle_presets,
                    effect_presets,
                );
            });
        }
    });

    // Paint accent stripe over left edge
    let card_rect = resp.response.rect;
    let stripe = egui::Rect::from_min_max(
        card_rect.left_top(),
        egui::pos2(
            card_rect.left() + ACCENT_STRIPE_WIDTH,
            card_rect.bottom(),
        ),
    );
    ui.painter().rect_filled(
        stripe,
        egui::CornerRadius {
            nw: CARD_ROUNDING,
            sw: CARD_ROUNDING,
            ne: 0,
            se: 0,
        },
        accent,
    );

    ui.add_space(4.0);
    removed
}

// ---------------------------------------------------------------------------
// Mini timeline strip
// ---------------------------------------------------------------------------

fn draw_mini_timeline(
    ui: &mut egui::Ui,
    marker: &EffectMarker,
    playback_state: PlaybackState,
    playback_elapsed: f32,
) {
    ui.separator();

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), MINI_TIMELINE_HEIGHT),
        egui::Sense::hover(),
    );

    let painter = ui.painter_at(rect);

    // Background
    painter.rect_filled(rect, 0.0, colors::BG_DARK);

    // Time markers (every second)
    let max_time = marker
        .steps
        .iter()
        .filter_map(|s| {
            if let EffectTrigger::AtTime(t) = &s.trigger {
                Some(*t)
            } else {
                None
            }
        })
        .fold(5.0f32, f32::max)
        + 1.0;

    let usable_width = rect.width() - 4.0;
    let time_to_x = |t: f32| rect.left() + 2.0 + (t / max_time) * usable_width;

    // Tick marks
    let tick_interval = if max_time > 20.0 {
        5.0
    } else if max_time > 10.0 {
        2.0
    } else {
        1.0
    };
    let mut t = 0.0;
    while t <= max_time {
        let x = time_to_x(t);
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom() - 6.0),
                egui::pos2(x, rect.bottom()),
            ],
            egui::Stroke::new(1.0, colors::TEXT_MUTED),
        );
        t += tick_interval;
    }

    // Draw dots for time-triggered rules
    for (idx, step) in marker.steps.iter().enumerate() {
        if let EffectTrigger::AtTime(t) = &step.trigger {
            let x = time_to_x(*t);
            let y = rect.center().y;
            let color = trigger_accent(&step.trigger);
            painter.circle_filled(egui::pos2(x, y), MINI_DOT_RADIUS, color);

            // Tooltip on hover
            let dot_rect = egui::Rect::from_center_size(
                egui::pos2(x, y),
                egui::vec2(MINI_DOT_RADIUS * 3.0, MINI_DOT_RADIUS * 3.0),
            );
            let dot_resp = ui.interact(
                dot_rect,
                egui::Id::new(format!("mini_dot_{}", idx)),
                egui::Sense::hover(),
            );
            if dot_resp.hovered() {
                dot_resp.on_hover_ui(|ui| {
                    let name = if step.name.is_empty() {
                        format!("Rule #{}", idx + 1)
                    } else {
                        step.name.clone()
                    };
                    ui.label(format!("{} @ {:.1}s", name, t));
                });
            }
        }
    }

    // Playhead
    if playback_state == PlaybackState::Playing {
        let x = time_to_x(playback_elapsed);
        if x >= rect.left() && x <= rect.right() {
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(PLAYHEAD_WIDTH, colors::STATUS_ERROR),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Trigger editor
// ---------------------------------------------------------------------------

fn draw_trigger_editor(
    ui: &mut egui::Ui,
    trigger: &mut EffectTrigger,
    step_idx: usize,
    defined_tags: &[String],
    rule_names: &[String],
) {
    let mut current_variant = trigger.variant_index();

    egui::Grid::new(format!("trigger_grid_{step_idx}"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Trigger");
            egui::ComboBox::from_id_salt(format!("trigger_type_{step_idx}"))
                .selected_text(EffectTrigger::VARIANT_LABELS[current_variant])
                .show_ui(ui, |ui| {
                    for (i, label) in EffectTrigger::VARIANT_LABELS.iter().enumerate() {
                        ui.selectable_value(&mut current_variant, i, *label);
                    }
                });
            ui.end_row();
        });

    if current_variant != trigger.variant_index() {
        *trigger = EffectTrigger::from_variant_index(current_variant);
    }

    match trigger {
        EffectTrigger::AtTime(t) => {
            egui::Grid::new(format!("trigger_time_{step_idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Time");
                    ui.add(
                        egui::DragValue::new(t)
                            .speed(0.1)
                            .range(0.0..=300.0)
                            .max_decimals(2)
                            .suffix(" s"),
                    );
                    ui.end_row();
                });
        }
        EffectTrigger::OnCollision { tag } => {
            egui::Grid::new(format!("trigger_coll_{step_idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    tag_combo(ui, &format!("trigger_tag_{step_idx}"), tag, defined_tags);
                    ui.end_row();
                });
        }
        EffectTrigger::OnEffectEvent(name) => {
            egui::Grid::new(format!("trigger_event_{step_idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Event");
                    ui.add(egui::TextEdit::singleline(name).desired_width(120.0));
                    ui.end_row();
                });
        }
        EffectTrigger::AfterRule { source_rule, delay } => {
            egui::Grid::new(format!("trigger_after_{step_idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Source Rule");
                    if rule_names.is_empty() {
                        ui.add(
                            egui::TextEdit::singleline(source_rule).desired_width(120.0),
                        );
                    } else {
                        let display = if source_rule.is_empty() {
                            "(none)"
                        } else {
                            source_rule.as_str()
                        };
                        egui::ComboBox::from_id_salt(format!("after_rule_{step_idx}"))
                            .selected_text(display)
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                for name in rule_names {
                                    ui.selectable_value(
                                        source_rule,
                                        name.clone(),
                                        name.as_str(),
                                    );
                                }
                            });
                    }
                    ui.end_row();

                    grid_label(ui, "Delay");
                    ui.add(
                        egui::DragValue::new(delay)
                            .speed(0.1)
                            .range(0.0..=60.0)
                            .max_decimals(2)
                            .suffix(" s"),
                    );
                    ui.end_row();
                });
        }
        EffectTrigger::RepeatingInterval { interval, max_count } => {
            egui::Grid::new(format!("trigger_repeat_{step_idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Interval");
                    ui.add(
                        egui::DragValue::new(interval)
                            .speed(0.1)
                            .range(0.01..=60.0)
                            .max_decimals(2)
                            .suffix(" s"),
                    );
                    ui.end_row();

                    grid_label(ui, "Max Count");
                    let mut has_max = max_count.is_some();
                    let mut max_val = max_count.unwrap_or(10);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut has_max, "").changed() {
                            if has_max {
                                *max_count = Some(max_val);
                            } else {
                                *max_count = None;
                            }
                        }
                        if has_max {
                            if ui
                                .add(egui::DragValue::new(&mut max_val).range(1..=1000))
                                .changed()
                            {
                                *max_count = Some(max_val);
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("unlimited")
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                        }
                    });
                    ui.end_row();
                });
        }
        EffectTrigger::OnSpawn => {
            ui.label(
                egui::RichText::new("Fires once when the effect starts playing")
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        }
        EffectTrigger::AfterIdleTimeout { timeout } => {
            egui::Grid::new(format!("trigger_idle_{step_idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Timeout");
                    ui.add(
                        egui::DragValue::new(timeout)
                            .speed(0.1)
                            .range(0.1..=60.0)
                            .max_decimals(2)
                            .suffix(" s"),
                    );
                    ui.end_row();
                });
        }
    }
}

// ---------------------------------------------------------------------------
// Actions list within a rule
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_actions_list(
    ui: &mut egui::Ui,
    marker: &mut EffectMarker,
    step_idx: usize,
    defined_tags: &[String],
    _rule_names: &[String],
    particle_presets: &[String],
    effect_presets: &[String],
) {
    // Partition actions into categories
    let actions = &marker.steps[step_idx].actions;
    let mut spawn_indices = Vec::new();
    let mut physics_indices = Vec::new();
    let mut event_indices = Vec::new();
    let mut animation_indices = Vec::new();
    let mut ecs_indices = Vec::new();

    for (i, action) in actions.iter().enumerate() {
        match action {
            EffectAction::SpawnPrimitive { .. }
            | EffectAction::SpawnParticle { .. }
            | EffectAction::SpawnGltf { .. }
            | EffectAction::SpawnDecal { .. }
            | EffectAction::SpawnEffect { .. } => spawn_indices.push(i),
            EffectAction::SetVelocity { .. }
            | EffectAction::ApplyImpulse { .. }
            | EffectAction::SetGravity { .. } => physics_indices.push(i),
            EffectAction::Despawn { .. } | EffectAction::EmitEvent(_) => {
                event_indices.push(i)
            }
            EffectAction::TweenValue { .. } => animation_indices.push(i),
            EffectAction::InsertComponent { .. } | EffectAction::RemoveComponent { .. } => {
                ecs_indices.push(i)
            }
        }
    }

    let mut remove_action = None;

    // SPAWN category
    action_category_header(
        ui,
        "SPAWN",
        colors::ACCENT_GREEN,
        &[
            ("Spawn Primitive", 0),
            ("Spawn Particle", 1),
            ("Spawn Decal", 7),
            ("Spawn GLTF", 8),
            ("Spawn Effect", 9),
        ],
        &mut marker.steps[step_idx].actions,
    );
    for &action_idx in &spawn_indices {
        let label = marker.steps[step_idx].actions[action_idx].label();
        let accent = action_accent(&marker.steps[step_idx].actions[action_idx]);
        let action = &mut marker.steps[step_idx].actions[action_idx];
        if action_card(ui, label, accent, |ui| {
            draw_action_editor(
                ui,
                action,
                step_idx,
                action_idx,
                defined_tags,
                particle_presets,
                effect_presets,
            );
        }) {
            remove_action = Some(action_idx);
        }
    }

    // PHYSICS category
    action_category_header(
        ui,
        "PHYSICS",
        colors::ACCENT_ORANGE,
        &[
            ("Set Velocity", 2),
            ("Apply Impulse", 3),
            ("Set Gravity", 6),
        ],
        &mut marker.steps[step_idx].actions,
    );
    for &action_idx in &physics_indices {
        let label = marker.steps[step_idx].actions[action_idx].label();
        let accent = action_accent(&marker.steps[step_idx].actions[action_idx]);
        let action = &mut marker.steps[step_idx].actions[action_idx];
        if action_card(ui, label, accent, |ui| {
            draw_action_editor(
                ui,
                action,
                step_idx,
                action_idx,
                defined_tags,
                particle_presets,
                effect_presets,
            );
        }) {
            remove_action = Some(action_idx);
        }
    }

    // EVENTS category
    action_category_header(
        ui,
        "EVENTS",
        colors::ACCENT_BLUE,
        &[("Despawn", 4), ("Emit Event", 5)],
        &mut marker.steps[step_idx].actions,
    );
    for &action_idx in &event_indices {
        let label = marker.steps[step_idx].actions[action_idx].label();
        let accent = action_accent(&marker.steps[step_idx].actions[action_idx]);
        let action = &mut marker.steps[step_idx].actions[action_idx];
        if action_card(ui, label, accent, |ui| {
            draw_action_editor(
                ui,
                action,
                step_idx,
                action_idx,
                defined_tags,
                particle_presets,
                effect_presets,
            );
        }) {
            remove_action = Some(action_idx);
        }
    }

    // ANIMATION category
    action_category_header(
        ui,
        "ANIMATION",
        colors::ACCENT_PURPLE,
        &[("Tween Value", 12)],
        &mut marker.steps[step_idx].actions,
    );
    for &action_idx in &animation_indices {
        let label = marker.steps[step_idx].actions[action_idx].label();
        let accent = action_accent(&marker.steps[step_idx].actions[action_idx]);
        let action = &mut marker.steps[step_idx].actions[action_idx];
        if action_card(ui, label, accent, |ui| {
            draw_action_editor(
                ui,
                action,
                step_idx,
                action_idx,
                defined_tags,
                particle_presets,
                effect_presets,
            );
        }) {
            remove_action = Some(action_idx);
        }
    }

    // ECS category
    action_category_header(
        ui,
        "ECS",
        colors::ACCENT_CYAN,
        &[("Insert Component", 10), ("Remove Component", 11)],
        &mut marker.steps[step_idx].actions,
    );
    for &action_idx in &ecs_indices {
        let label = marker.steps[step_idx].actions[action_idx].label();
        let accent = action_accent(&marker.steps[step_idx].actions[action_idx]);
        let action = &mut marker.steps[step_idx].actions[action_idx];
        if action_card(ui, label, accent, |ui| {
            draw_action_editor(
                ui,
                action,
                step_idx,
                action_idx,
                defined_tags,
                particle_presets,
                effect_presets,
            );
        }) {
            remove_action = Some(action_idx);
        }
    }

    if let Some(idx) = remove_action {
        marker.steps[step_idx].actions.remove(idx);
    }
}

// ---------------------------------------------------------------------------
// Action editor
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_action_editor(
    ui: &mut egui::Ui,
    action: &mut EffectAction,
    step_idx: usize,
    action_idx: usize,
    defined_tags: &[String],
    particle_presets: &[String],
    effect_presets: &[String],
) {
    let id_salt = format!("action_{step_idx}_{action_idx}");

    let mut current_variant = action.variant_index();
    egui::Grid::new(format!("action_type_{id_salt}"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Type");
            egui::ComboBox::from_id_salt(format!("action_combo_{id_salt}"))
                .selected_text(EffectAction::VARIANT_LABELS[current_variant])
                .show_ui(ui, |ui| {
                    for (i, label) in EffectAction::VARIANT_LABELS.iter().enumerate() {
                        ui.selectable_value(&mut current_variant, i, *label);
                    }
                });
            ui.end_row();
        });

    if current_variant != action.variant_index() {
        *action = EffectAction::from_variant_index(current_variant);
    }

    match action {
        EffectAction::SpawnPrimitive {
            tag,
            shape,
            offset,
            material,
            rigid_body,
        } => {
            egui::Grid::new(format!("spawn_prim_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    ui.add(egui::TextEdit::singleline(tag).desired_width(100.0));
                    ui.end_row();

                    grid_label(ui, "Shape");
                    egui::ComboBox::from_id_salt(format!("shape_{id_salt}"))
                        .selected_text(shape.display_name())
                        .show_ui(ui, |ui| {
                            for s in &[
                                PrimitiveShape::Cube,
                                PrimitiveShape::Sphere,
                                PrimitiveShape::Cylinder,
                                PrimitiveShape::Capsule,
                            ] {
                                ui.selectable_value(shape, *s, s.display_name());
                            }
                        });
                    ui.end_row();

                    grid_label(ui, "Offset");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut offset.x).speed(0.1).prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut offset.y).speed(0.1).prefix("y:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut offset.z).speed(0.1).prefix("z:"),
                        );
                    });
                    ui.end_row();

                    grid_label(ui, "Physics");
                    let mut has_rb = rigid_body.is_some();
                    if ui.checkbox(&mut has_rb, "").changed() {
                        if has_rb {
                            *rigid_body = Some(RigidBodyKind::Dynamic);
                        } else {
                            *rigid_body = None;
                        }
                    }
                    ui.end_row();

                    if let Some(rb) = rigid_body {
                        grid_label(ui, "Body");
                        egui::ComboBox::from_id_salt(format!("rb_{id_salt}"))
                            .selected_text(rb.label())
                            .show_ui(ui, |ui| {
                                for kind in &RigidBodyKind::ALL {
                                    ui.selectable_value(rb, *kind, kind.label());
                                }
                            });
                        ui.end_row();
                    }

                    grid_label(ui, "Material");
                    let mut mat_str = material.clone().unwrap_or_default();
                    if ui
                        .add(egui::TextEdit::singleline(&mut mat_str).desired_width(100.0))
                        .changed()
                    {
                        *material = if mat_str.is_empty() {
                            None
                        } else {
                            Some(mat_str)
                        };
                    }
                    ui.end_row();
                });
        }
        EffectAction::SpawnParticle { tag, preset, at } => {
            egui::Grid::new(format!("spawn_part_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    ui.add(egui::TextEdit::singleline(tag).desired_width(100.0));
                    ui.end_row();

                    grid_label(ui, "Preset");
                    let selected_text =
                        if preset.is_empty() { "(none)" } else { preset.as_str() };
                    egui::ComboBox::from_id_salt(format!("preset_{id_salt}"))
                        .selected_text(selected_text)
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for name in particle_presets {
                                ui.selectable_value(preset, name.clone(), name.as_str());
                            }
                        });
                    ui.end_row();

                    draw_spawn_location_editor(ui, at, &format!("part_loc_{id_salt}"));
                });
        }
        EffectAction::SetVelocity { tag, velocity } => {
            egui::Grid::new(format!("set_vel_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    tag_combo(ui, &format!("tag_vel_{id_salt}"), tag, defined_tags);
                    ui.end_row();

                    grid_label(ui, "Velocity");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut velocity.x)
                                .speed(0.1)
                                .prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut velocity.y)
                                .speed(0.1)
                                .prefix("y:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut velocity.z)
                                .speed(0.1)
                                .prefix("z:"),
                        );
                    });
                    ui.end_row();
                });
        }
        EffectAction::ApplyImpulse { tag, impulse } => {
            egui::Grid::new(format!("impulse_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    tag_combo(ui, &format!("tag_imp_{id_salt}"), tag, defined_tags);
                    ui.end_row();

                    grid_label(ui, "Impulse");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut impulse.x)
                                .speed(0.1)
                                .prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut impulse.y)
                                .speed(0.1)
                                .prefix("y:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut impulse.z)
                                .speed(0.1)
                                .prefix("z:"),
                        );
                    });
                    ui.end_row();
                });
        }
        EffectAction::Despawn { tag } => {
            egui::Grid::new(format!("despawn_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    tag_combo(ui, &format!("tag_desp_{id_salt}"), tag, defined_tags);
                    ui.end_row();
                });
        }
        EffectAction::EmitEvent(name) => {
            egui::Grid::new(format!("emit_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Event");
                    ui.add(egui::TextEdit::singleline(name).desired_width(120.0));
                    ui.end_row();
                });
        }
        EffectAction::SetGravity { tag, enabled } => {
            egui::Grid::new(format!("gravity_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    tag_combo(ui, &format!("tag_grav_{id_salt}"), tag, defined_tags);
                    ui.end_row();

                    grid_label(ui, "Gravity");
                    ui.checkbox(enabled, "Enabled");
                    ui.end_row();
                });
        }
        EffectAction::SpawnDecal {
            tag,
            texture_path,
            at,
            scale,
        } => {
            egui::Grid::new(format!("spawn_decal_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    ui.add(egui::TextEdit::singleline(tag).desired_width(100.0));
                    ui.end_row();

                    grid_label(ui, "Texture");
                    ui.horizontal(|ui| {
                        let display = if texture_path.is_empty() {
                            "None"
                        } else {
                            texture_path
                                .rsplit('/')
                                .next()
                                .unwrap_or(texture_path.as_str())
                        };
                        ui.label(
                            egui::RichText::new(display)
                                .color(if texture_path.is_empty() {
                                    colors::TEXT_MUTED
                                } else {
                                    colors::TEXT_PRIMARY
                                })
                                .small(),
                        );
                        if ui
                            .small_button("Browse")
                            .on_hover_text("Pick a texture file")
                            .clicked()
                        {
                            ui.memory_mut(|m| {
                                m.data.insert_temp(
                                    egui::Id::new("effect_decal_browse"),
                                    true,
                                );
                            });
                        }
                        if !texture_path.is_empty()
                            && ui
                                .small_button("X")
                                .on_hover_text("Clear texture")
                                .clicked()
                        {
                            texture_path.clear();
                        }
                    });
                    ui.end_row();

                    draw_spawn_location_editor(ui, at, &format!("decal_loc_{id_salt}"));

                    grid_label(ui, "Scale");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut scale.x).speed(0.1).prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut scale.y).speed(0.1).prefix("y:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut scale.z).speed(0.1).prefix("z:"),
                        );
                    });
                    ui.end_row();
                });
        }
        EffectAction::SpawnGltf {
            tag,
            path,
            at,
            scale,
            rigid_body,
        } => {
            egui::Grid::new(format!("spawn_gltf_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    ui.add(egui::TextEdit::singleline(tag).desired_width(100.0));
                    ui.end_row();

                    grid_label(ui, "Model");
                    ui.horizontal(|ui| {
                        let display = if path.is_empty() {
                            "None"
                        } else {
                            path.rsplit('/').next().unwrap_or(path.as_str())
                        };
                        ui.label(
                            egui::RichText::new(display)
                                .color(if path.is_empty() {
                                    colors::TEXT_MUTED
                                } else {
                                    colors::TEXT_PRIMARY
                                })
                                .small(),
                        );
                        if ui
                            .small_button("Browse")
                            .on_hover_text("Pick a GLTF/GLB model")
                            .clicked()
                        {
                            ui.memory_mut(|m| {
                                m.data
                                    .insert_temp(egui::Id::new("effect_gltf_browse"), true);
                            });
                        }
                        if !path.is_empty()
                            && ui
                                .small_button("X")
                                .on_hover_text("Clear model")
                                .clicked()
                        {
                            path.clear();
                        }
                    });
                    ui.end_row();

                    draw_spawn_location_editor(ui, at, &format!("gltf_loc_{id_salt}"));

                    grid_label(ui, "Scale");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut scale.x).speed(0.1).prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut scale.y).speed(0.1).prefix("y:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut scale.z).speed(0.1).prefix("z:"),
                        );
                    });
                    ui.end_row();

                    grid_label(ui, "Physics");
                    let mut has_rb = rigid_body.is_some();
                    if ui.checkbox(&mut has_rb, "").changed() {
                        if has_rb {
                            *rigid_body = Some(RigidBodyKind::Dynamic);
                        } else {
                            *rigid_body = None;
                        }
                    }
                    ui.end_row();

                    if let Some(rb) = rigid_body {
                        grid_label(ui, "Body");
                        egui::ComboBox::from_id_salt(format!("gltf_rb_{id_salt}"))
                            .selected_text(rb.label())
                            .show_ui(ui, |ui| {
                                for kind in &RigidBodyKind::ALL {
                                    ui.selectable_value(rb, *kind, kind.label());
                                }
                            });
                        ui.end_row();
                    }
                });
        }
        EffectAction::SpawnEffect {
            tag,
            preset,
            at,
            inherit_velocity,
        } => {
            egui::Grid::new(format!("spawn_fx_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Tag");
                    ui.add(egui::TextEdit::singleline(tag).desired_width(100.0));
                    ui.end_row();

                    grid_label(ui, "Preset");
                    let selected_text =
                        if preset.is_empty() { "(none)" } else { preset.as_str() };
                    egui::ComboBox::from_id_salt(format!("fx_preset_{id_salt}"))
                        .selected_text(selected_text)
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for name in effect_presets {
                                ui.selectable_value(preset, name.clone(), name.as_str());
                            }
                        });
                    ui.end_row();

                    draw_spawn_location_editor(ui, at, &format!("fx_loc_{id_salt}"));

                    grid_label(ui, "Inherit Vel.");
                    ui.checkbox(inherit_velocity, "");
                    ui.end_row();
                });
        }
        EffectAction::InsertComponent {
            target_tag,
            component_type,
            field_values,
        } => {
            egui::Grid::new(format!("insert_comp_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Target Tag");
                    tag_combo(
                        ui,
                        &format!("tag_ins_{id_salt}"),
                        target_tag,
                        defined_tags,
                    );
                    ui.end_row();

                    grid_label(ui, "Component");
                    ui.add(
                        egui::TextEdit::singleline(component_type).desired_width(120.0),
                    );
                    ui.end_row();
                });

            // Field overrides
            if !field_values.is_empty() {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Field Overrides")
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
                let mut remove_key = None;
                let keys: Vec<String> = field_values.keys().cloned().collect();
                for key in &keys {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(key)
                                .small()
                                .color(colors::TEXT_SECONDARY),
                        );
                        if let Some(val) = field_values.get_mut(key) {
                            ui.add(egui::TextEdit::singleline(val).desired_width(80.0));
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("\u{00d7}")
                                        .small()
                                        .color(colors::STATUS_ERROR),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            remove_key = Some(key.clone());
                        }
                    });
                }
                if let Some(key) = remove_key {
                    field_values.remove(&key);
                }
            }

            // Add field button
            ui.horizontal(|ui| {
                if ui
                    .small_button(
                        egui::RichText::new("+ Field").color(colors::ACCENT_GREEN),
                    )
                    .clicked()
                {
                    let key = format!("field_{}", field_values.len());
                    field_values.insert(key, String::new());
                }
            });
        }
        EffectAction::RemoveComponent {
            target_tag,
            component_type,
        } => {
            egui::Grid::new(format!("remove_comp_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Target Tag");
                    tag_combo(
                        ui,
                        &format!("tag_rem_{id_salt}"),
                        target_tag,
                        defined_tags,
                    );
                    ui.end_row();

                    grid_label(ui, "Component");
                    ui.add(
                        egui::TextEdit::singleline(component_type).desired_width(120.0),
                    );
                    ui.end_row();
                });
        }
        EffectAction::TweenValue {
            target_tag,
            property,
            from,
            to,
            duration,
            easing,
        } => {
            egui::Grid::new(format!("tween_{id_salt}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Target Tag");
                    tag_combo(
                        ui,
                        &format!("tag_tw_{id_salt}"),
                        target_tag,
                        defined_tags,
                    );
                    ui.end_row();

                    grid_label(ui, "Property");
                    let mut prop_idx = property.variant_index();
                    egui::ComboBox::from_id_salt(format!("tw_prop_{id_salt}"))
                        .selected_text(TweenProperty::VARIANT_LABELS[prop_idx])
                        .show_ui(ui, |ui| {
                            for (i, label) in
                                TweenProperty::VARIANT_LABELS.iter().enumerate()
                            {
                                ui.selectable_value(&mut prop_idx, i, *label);
                            }
                        });
                    if prop_idx != property.variant_index() {
                        *property = TweenProperty::from_variant_index(prop_idx);
                    }
                    ui.end_row();

                    grid_label(ui, "From");
                    ui.add(egui::DragValue::new(from).speed(0.1));
                    ui.end_row();

                    grid_label(ui, "To");
                    ui.add(egui::DragValue::new(to).speed(0.1));
                    ui.end_row();

                    grid_label(ui, "Duration");
                    ui.add(
                        egui::DragValue::new(duration)
                            .speed(0.1)
                            .range(0.01..=60.0)
                            .suffix(" s"),
                    );
                    ui.end_row();

                    grid_label(ui, "Easing");
                    egui::ComboBox::from_id_salt(format!("tw_ease_{id_salt}"))
                        .selected_text(easing.label())
                        .show_ui(ui, |ui| {
                            for e in &EasingType::ALL {
                                ui.selectable_value(easing, *e, e.label());
                            }
                        });
                    ui.end_row();
                });
        }
    }
}

// ---------------------------------------------------------------------------
// Shared spawn location editor
// ---------------------------------------------------------------------------

fn draw_spawn_location_editor(ui: &mut egui::Ui, at: &mut SpawnLocation, id_salt: &str) {
    grid_label(ui, "Location");
    let is_offset = matches!(at, SpawnLocation::Offset(_));
    let mut loc_idx: usize = if is_offset { 0 } else { 1 };
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(if is_offset {
            "Offset"
        } else {
            "Collision Point"
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut loc_idx, 0, "Offset");
            ui.selectable_value(&mut loc_idx, 1, "Collision Point");
        });
    if loc_idx == 0 && !is_offset {
        *at = SpawnLocation::Offset(Vec3::ZERO);
    } else if loc_idx == 1 && is_offset {
        *at = SpawnLocation::CollisionPoint;
    }
    ui.end_row();

    if let SpawnLocation::Offset(offset) = at {
        grid_label(ui, "Offset");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut offset.x).speed(0.1).prefix("x:"));
            ui.add(egui::DragValue::new(&mut offset.y).speed(0.1).prefix("y:"));
            ui.add(egui::DragValue::new(&mut offset.z).speed(0.1).prefix("z:"));
        });
        ui.end_row();
    }
}
