//! Effect editor panel for editing effect sequences on selected entities.
//!
//! Two-window layout:
//! - **Timeline window** (bottom-anchored) — horizontal lanes grouped by trigger type
//! - **Detail panel** (right side) — shows selected step's trigger fields and action list

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::effects::data::*;
use crate::effects::EffectLibrary;
use crate::particles::ParticleLibrary;
use crate::scene::PrimitiveShape;
use crate::selection::Selected;
use crate::ui::command_palette::{CommandPaletteState, GltfPickResult, TexturePickResult, TextureSlot};
use crate::ui::theme::{colors, draw_pin_button, grid_label, panel, panel_frame, window_frame};

// ---------------------------------------------------------------------------
// Plugin + State
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct EffectEditorState {
    pub selected_step: Option<usize>,
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

const ACCENT_STRIPE_WIDTH: f32 = 3.0;
const BLOCK_WIDTH: f32 = 90.0;
const BLOCK_HEIGHT: f32 = 28.0;
const BLOCK_SPACING: f32 = 4.0;
const LANE_LABEL_WIDTH: f32 = 70.0;
const TIME_SCALE: f32 = 100.0; // pixels per second

const STEP_COLORS: &[egui::Color32] = &[
    colors::ACCENT_ORANGE,
    colors::ACCENT_GREEN,
    colors::ACCENT_BLUE,
    colors::ACCENT_PURPLE,
    colors::ACCENT_CYAN,
];

fn step_color(idx: usize) -> egui::Color32 {
    STEP_COLORS[idx % STEP_COLORS.len()]
}

// ---------------------------------------------------------------------------
// Action card (reused from original)
// ---------------------------------------------------------------------------

fn action_card(
    ui: &mut egui::Ui,
    label: &str,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut removed = false;

    let frame = egui::Frame::new()
        .fill(colors::BG_LIGHT)
        .corner_radius(egui::CornerRadius::same(2))
        .inner_margin(egui::Margin::same(4));

    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("\u{00d7}")
                                .small()
                                .color(colors::STATUS_ERROR),
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
        body(ui);
    });

    ui.add_space(2.0);
    removed
}

// ---------------------------------------------------------------------------
// Tag helpers
// ---------------------------------------------------------------------------

fn collect_defined_tags(marker: &EffectMarker) -> Vec<String> {
    let mut tags = Vec::new();
    for step in &marker.steps {
        for action in &step.actions {
            let tag = match action {
                EffectAction::SpawnPrimitive { tag, .. } => tag,
                EffectAction::SpawnParticle { tag, .. } => tag,
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
        let display = if tag.is_empty() { "(none)" } else { tag.as_str() };
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
    let mut state = world.remove_resource::<EffectEditorState>().unwrap_or_default();

    // Bounds-check selected_step
    if let Some(idx) = state.selected_step {
        if idx >= marker.steps.len() {
            state.selected_step = if marker.steps.is_empty() {
                None
            } else {
                Some(marker.steps.len() - 1)
            };
        }
    }

    let defined_tags = collect_defined_tags(&marker);

    // Collect particle preset names for SpawnParticle action dropdown
    let mut particle_presets: Vec<String> = world
        .get_resource::<ParticleLibrary>()
        .map(|lib| lib.effects.keys().cloned().collect())
        .unwrap_or_default();
    particle_presets.sort();

    // Collect deferred actions
    let mut pin_toggled = false;
    let mut save_preset_clicked = false;
    let mut browse_presets_clicked = false;
    let mut play_clicked = false;
    let mut pause_clicked = false;
    let mut stop_clicked = false;

    // Draw timeline window (bottom)
    let timeline_height = draw_timeline_window(
        &ctx,
        &mut marker,
        &mut entity_name,
        &mut state,
        playback_state,
        playback_elapsed,
        &mut play_clicked,
        &mut pause_clicked,
        &mut stop_clicked,
        &mut save_preset_clicked,
        &mut browse_presets_clicked,
    );

    // Draw detail panel (right side)
    draw_detail_panel(
        &ctx,
        &mut marker,
        &mut state,
        &defined_tags,
        &particle_presets,
        is_pinned,
        current_mode,
        &mut pin_toggled,
        timeline_height,
    );

    // Handle deferred texture browse request from SpawnDecal action
    let browse_decal_texture =
        ctx.memory(|m| m.data.get_temp::<bool>(egui::Id::new("effect_decal_browse"))) == Some(true);
    if browse_decal_texture {
        ctx.memory_mut(|m| m.data.remove::<bool>(egui::Id::new("effect_decal_browse")));
        world
            .resource_mut::<CommandPaletteState>()
            .open_pick_texture(TextureSlot::EffectDecalTexture, Some(entity));
    }

    // Consume texture pick result for effect decal
    {
        let pick_data = world.resource_mut::<TexturePickResult>().0.take();
        if let Some(pick) = pick_data {
            if pick.slot == TextureSlot::EffectDecalTexture && pick.entity == Some(entity) {
                // Find the SpawnDecal action and set its texture_path
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
                    // No SpawnDecal found — put it back
                    world.resource_mut::<TexturePickResult>().0 = Some(pick);
                }
            } else {
                // Different slot or entity — put it back
                world.resource_mut::<TexturePickResult>().0 = Some(pick);
            }
        }
    }

    // Handle deferred GLTF browse request from SpawnGltf action
    let browse_gltf =
        ctx.memory(|m| m.data.get_temp::<bool>(egui::Id::new("effect_gltf_browse"))) == Some(true);
    if browse_gltf {
        ctx.memory_mut(|m| m.data.remove::<bool>(egui::Id::new("effect_gltf_browse")));
        world
            .resource_mut::<CommandPaletteState>()
            .open_pick_gltf(Some(entity));
    }

    // Consume GLTF pick result for effect SpawnGltf
    {
        let pick_data = world.resource_mut::<GltfPickResult>().0.take();
        if let Some(pick) = pick_data {
            if pick.entity == Some(entity) {
                // Find the first SpawnGltf action and set its path
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
        library.effects.insert(preset_name.clone(), marker_for_save);
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

    egui::Window::new("Effect Sequencer")
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
// Timeline Window (bottom-anchored)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_timeline_window(
    ctx: &egui::Context,
    marker: &mut EffectMarker,
    entity_name: &mut String,
    state: &mut EffectEditorState,
    playback_state: PlaybackState,
    playback_elapsed: f32,
    play_clicked: &mut bool,
    pause_clicked: &mut bool,
    stop_clicked: &mut bool,
    save_preset_clicked: &mut bool,
    browse_presets_clicked: &mut bool,
) -> f32 {
    let viewport_width = ctx.input(|i| i.viewport_rect().width());
    // Leave room for the sequencer panel on the right + padding between them
    let timeline_width = viewport_width - panel::DEFAULT_WIDTH - panel::WINDOW_PADDING * 3.0;

    let response = egui::Window::new("Effect Timeline")
        .default_width(timeline_width)
        .max_width(timeline_width)
        .default_height(160.0)
        .anchor(
            egui::Align2::LEFT_BOTTOM,
            [
                panel::WINDOW_PADDING,
                -(panel::STATUS_BAR_HEIGHT + panel::WINDOW_PADDING),
            ],
        )
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .frame(window_frame(&ctx.style()))
        .show(ctx, |ui| {
            // Header row
            ui.horizontal(|ui| {
                // Entity name (compact)
                ui.add(
                    egui::TextEdit::singleline(entity_name)
                        .desired_width(120.0)
                        .font(egui::FontId::proportional(13.0)),
                );

                ui.separator();

                // Playback status
                match playback_state {
                    PlaybackState::Playing => {
                        ui.label(
                            egui::RichText::new("Playing")
                                .small()
                                .color(colors::STATUS_SUCCESS),
                        );
                    }
                    PlaybackState::Paused => {
                        ui.label(
                            egui::RichText::new("Paused")
                                .small()
                                .color(colors::STATUS_WARNING),
                        );
                    }
                    PlaybackState::Stopped => {
                        ui.label(
                            egui::RichText::new("Stopped")
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                    }
                }

                if ui
                    .button(egui::RichText::new("\u{25b6}").small().color(colors::STATUS_SUCCESS))
                    .on_hover_text("Play")
                    .clicked()
                {
                    *play_clicked = true;
                }
                if ui
                    .button(egui::RichText::new("\u{23f8}").small().color(colors::STATUS_WARNING))
                    .on_hover_text("Pause")
                    .clicked()
                {
                    *pause_clicked = true;
                }
                if ui
                    .button(egui::RichText::new("\u{25a0}").small().color(colors::STATUS_ERROR))
                    .on_hover_text("Stop")
                    .clicked()
                {
                    *stop_clicked = true;
                }

                // Right-aligned preset buttons
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

            // Lanes
            let mut new_step_trigger: Option<EffectTrigger> = None;

            // Collect step indices per lane
            let mut time_steps: Vec<(usize, f32)> = Vec::new();
            let mut collision_steps: Vec<(usize, String)> = Vec::new();
            let mut event_steps: Vec<(usize, String)> = Vec::new();

            for (idx, step) in marker.steps.iter().enumerate() {
                match &step.trigger {
                    EffectTrigger::AtTime(t) => time_steps.push((idx, *t)),
                    EffectTrigger::OnCollision { tag } => {
                        collision_steps.push((idx, tag.clone()))
                    }
                    EffectTrigger::OnEffectEvent(name) => {
                        event_steps.push((idx, name.clone()))
                    }
                }
            }

            // Sort time steps by time
            time_steps.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            // Time lane
            draw_lane(
                ui,
                "Time",
                &time_steps,
                marker,
                state,
                playback_state,
                playback_elapsed,
                true,
                |_, t| format!("{:.1}s", t),
            );
            if ui.memory(|m| m.data.get_temp::<bool>(egui::Id::new("add_time_step"))) == Some(true)
            {
                ui.memory_mut(|m| m.data.remove::<bool>(egui::Id::new("add_time_step")));
                new_step_trigger = Some(EffectTrigger::AtTime(0.0));
            }

            // Collision lane
            draw_lane(
                ui,
                "Collision",
                &collision_steps,
                marker,
                state,
                playback_state,
                playback_elapsed,
                false,
                |_, tag| {
                    if tag.is_empty() {
                        "(no tag)".to_string()
                    } else {
                        tag.clone()
                    }
                },
            );
            if ui.memory(|m| m.data.get_temp::<bool>(egui::Id::new("add_collision_step")))
                == Some(true)
            {
                ui.memory_mut(|m| m.data.remove::<bool>(egui::Id::new("add_collision_step")));
                new_step_trigger = Some(EffectTrigger::OnCollision {
                    tag: String::new(),
                });
            }

            // Event lane
            draw_lane(
                ui,
                "Event",
                &event_steps,
                marker,
                state,
                playback_state,
                playback_elapsed,
                false,
                |_, name| {
                    if name.is_empty() {
                        "(unnamed)".to_string()
                    } else {
                        name.clone()
                    }
                },
            );
            if ui.memory(|m| m.data.get_temp::<bool>(egui::Id::new("add_event_step")))
                == Some(true)
            {
                ui.memory_mut(|m| m.data.remove::<bool>(egui::Id::new("add_event_step")));
                new_step_trigger = Some(EffectTrigger::OnEffectEvent(String::new()));
            }

            // Handle adding new steps
            if let Some(trigger) = new_step_trigger {
                let new_idx = marker.steps.len();
                marker.steps.push(EffectStep {
                    name: format!("step_{}", new_idx + 1),
                    trigger,
                    actions: Vec::new(),
                });
                state.selected_step = Some(new_idx);
            }
        });

    response
        .map(|r| r.response.rect.height())
        .unwrap_or(160.0)
}

/// Draw a single lane row with step blocks.
#[allow(clippy::too_many_arguments)]
fn draw_lane<T: Clone>(
    ui: &mut egui::Ui,
    lane_name: &str,
    steps: &[(usize, T)],
    marker: &EffectMarker,
    state: &mut EffectEditorState,
    playback_state: PlaybackState,
    playback_elapsed: f32,
    is_time_lane: bool,
    block_label_fn: impl Fn(&EffectMarker, &T) -> String,
) {
    let add_id = egui::Id::new(format!("add_{}_step", lane_name.to_lowercase()));

    ui.horizontal(|ui| {
        // Lane label
        ui.allocate_ui_with_layout(
            egui::vec2(LANE_LABEL_WIDTH, BLOCK_HEIGHT),
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.label(
                    egui::RichText::new(lane_name)
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            },
        );

        // Step blocks area — use a child UI so we can paint over it
        let (blocks_rect, _) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width() - 30.0, // leave room for + button
                BLOCK_HEIGHT,
            ),
            egui::Sense::hover(),
        );

        let painter = ui.painter_at(blocks_rect);

        // Draw playhead for time lane
        if is_time_lane && playback_state == PlaybackState::Playing {
            let playhead_x = blocks_rect.left() + playback_elapsed * TIME_SCALE;
            if playhead_x >= blocks_rect.left() && playhead_x <= blocks_rect.right() {
                painter.line_segment(
                    [
                        egui::pos2(playhead_x, blocks_rect.top()),
                        egui::pos2(playhead_x, blocks_rect.bottom()),
                    ],
                    egui::Stroke::new(2.0, colors::STATUS_ERROR),
                );
            }
        }

        // Draw step blocks
        for (seq_idx, (step_idx, data)) in steps.iter().enumerate() {
            let block_x = if is_time_lane {
                // Position proportionally by time
                if let EffectTrigger::AtTime(t) = &marker.steps[*step_idx].trigger {
                    blocks_rect.left() + t * TIME_SCALE
                } else {
                    blocks_rect.left() + seq_idx as f32 * (BLOCK_WIDTH + BLOCK_SPACING)
                }
            } else {
                blocks_rect.left() + seq_idx as f32 * (BLOCK_WIDTH + BLOCK_SPACING)
            };

            let block_rect = egui::Rect::from_min_size(
                egui::pos2(block_x, blocks_rect.top()),
                egui::vec2(BLOCK_WIDTH, BLOCK_HEIGHT),
            );

            // Skip if out of bounds
            if block_rect.left() > blocks_rect.right() {
                continue;
            }

            let is_selected = state.selected_step == Some(*step_idx);
            let accent = step_color(*step_idx);

            // Background
            let fill = if is_selected {
                colors::SELECTION_BG
            } else {
                colors::BG_MEDIUM
            };
            painter.rect_filled(block_rect, egui::CornerRadius::same(3), fill);

            // Accent stripe
            let stripe_rect = egui::Rect::from_min_max(
                block_rect.left_top(),
                egui::pos2(
                    block_rect.left() + ACCENT_STRIPE_WIDTH,
                    block_rect.bottom(),
                ),
            );
            painter.rect_filled(
                stripe_rect,
                egui::CornerRadius {
                    nw: 3,
                    sw: 3,
                    ne: 0,
                    se: 0,
                },
                accent,
            );

            // Selected border
            if is_selected {
                painter.rect_stroke(
                    block_rect,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(1.5, accent),
                    egui::StrokeKind::Inside,
                );
            }

            // Block label (clipped)
            let label_text = block_label_fn(marker, data);
            let step_name = &marker.steps[*step_idx].name;
            let display = if step_name.is_empty() {
                label_text
            } else {
                step_name.clone()
            };

            let text_rect = egui::Rect::from_min_max(
                egui::pos2(
                    block_rect.left() + ACCENT_STRIPE_WIDTH + 4.0,
                    block_rect.top() + 2.0,
                ),
                egui::pos2(block_rect.right() - 2.0, block_rect.bottom() - 2.0),
            );

            let galley = painter.layout_no_wrap(
                display,
                egui::FontId::proportional(11.0),
                colors::TEXT_PRIMARY,
            );
            painter.with_clip_rect(text_rect).galley(
                text_rect.left_top(),
                galley,
                colors::TEXT_PRIMARY,
            );

            // Click interaction
            let block_resp = ui.interact(
                block_rect,
                egui::Id::new(format!("step_block_{}", step_idx)),
                egui::Sense::click(),
            );
            if block_resp.clicked() {
                state.selected_step = Some(*step_idx);
            }
        }

        // "+" button
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("+")
                        .color(colors::ACCENT_GREEN),
                )
                .min_size(egui::vec2(24.0, BLOCK_HEIGHT)),
            )
            .on_hover_text(format!("Add {} step", lane_name.to_lowercase()))
            .clicked()
        {
            ui.memory_mut(|m| m.data.insert_temp(add_id, true));
        }
    });
}

// ---------------------------------------------------------------------------
// Detail Panel (right side)
// ---------------------------------------------------------------------------

fn draw_detail_panel(
    ctx: &egui::Context,
    marker: &mut EffectMarker,
    state: &mut EffectEditorState,
    defined_tags: &[String],
    particle_presets: &[String],
    is_pinned: bool,
    current_mode: EditorMode,
    pin_toggled: &mut bool,
    timeline_height: f32,
) {
    let available_height = panel::available_height(ctx) - timeline_height - panel::WINDOW_PADDING;

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

    egui::Window::new("Effect Sequencer")
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(true)
        .frame(panel_frame(&ctx.style()))
        .show(ctx, |ui| {
            // Pin button
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                *pin_toggled = draw_pin_button(ui, is_pinned);
            });

            match state.selected_step {
                None => {
                    // No step selected
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Select a step in the timeline")
                                .color(colors::TEXT_MUTED)
                                .italics(),
                        );
                    });
                }
                Some(step_idx) => {
                    draw_step_detail(ui, marker, state, step_idx, defined_tags, particle_presets);
                }
            }
        });
}

fn draw_step_detail(
    ui: &mut egui::Ui,
    marker: &mut EffectMarker,
    state: &mut EffectEditorState,
    step_idx: usize,
    defined_tags: &[String],
    particle_presets: &[String],
) {
    let step_count = marker.steps.len();
    if step_idx >= step_count {
        return;
    }

    // Header: step name + index + remove
    ui.horizontal(|ui| {
        let step = &mut marker.steps[step_idx];
        ui.add(
            egui::TextEdit::singleline(&mut step.name)
                .font(egui::FontId::proportional(16.0))
                .text_color(colors::TEXT_PRIMARY)
                .margin(egui::vec2(8.0, 6.0)),
        );

        ui.label(
            egui::RichText::new(format!("#{}", step_idx + 1))
                .small()
                .color(colors::TEXT_MUTED),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Remove")
                            .small()
                            .color(colors::STATUS_ERROR),
                    )
                    .frame(false),
                )
                .on_hover_text("Remove this step")
                .clicked()
            {
                // Defer removal until after drawing
                ui.memory_mut(|m| {
                    m.data
                        .insert_temp(egui::Id::new("remove_step_flag"), true);
                });
            }
        });
    });

    ui.separator();

    // Check for deferred removal
    let should_remove =
        ui.memory(|m| m.data.get_temp::<bool>(egui::Id::new("remove_step_flag"))) == Some(true);
    if should_remove {
        ui.memory_mut(|m| m.data.remove::<bool>(egui::Id::new("remove_step_flag")));
        marker.steps.remove(step_idx);
        // Adjust selection
        if marker.steps.is_empty() {
            state.selected_step = None;
        } else if step_idx >= marker.steps.len() {
            state.selected_step = Some(marker.steps.len() - 1);
        } else {
            // Keep same index (now points to next step)
            state.selected_step = Some(step_idx);
        }
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(230.0);

            // Trigger section
            ui.label(
                egui::RichText::new("Trigger")
                    .strong()
                    .color(colors::TEXT_SECONDARY),
            );
            ui.add_space(2.0);

            {
                let step = &mut marker.steps[step_idx];
                draw_trigger_editor(ui, &mut step.trigger, step_idx, defined_tags);
            }

            ui.add_space(8.0);

            // Actions section
            ui.label(
                egui::RichText::new("Actions")
                    .strong()
                    .color(colors::TEXT_SECONDARY),
            );
            ui.add_space(2.0);

            let mut remove_action = None;
            let action_count = marker.steps[step_idx].actions.len();
            for action_idx in 0..action_count {
                let label = marker.steps[step_idx].actions[action_idx].label();
                let action = &mut marker.steps[step_idx].actions[action_idx];
                if action_card(ui, label, |ui| {
                    draw_action_editor(ui, action, step_idx, action_idx, defined_tags, particle_presets);
                }) {
                    remove_action = Some(action_idx);
                }
            }
            if let Some(idx) = remove_action {
                marker.steps[step_idx].actions.remove(idx);
            }

            // Add action button
            ui.menu_button(
                egui::RichText::new("+ Add Action").color(colors::ACCENT_GREEN),
                |ui| {
                    for (i, label) in EffectAction::VARIANT_LABELS.iter().enumerate() {
                        if ui.button(*label).clicked() {
                            marker.steps[step_idx]
                                .actions
                                .push(EffectAction::from_variant_index(i));
                            ui.close();
                        }
                    }
                },
            );

            ui.add_space(8.0);
        });
}

// ---------------------------------------------------------------------------
// Trigger editor
// ---------------------------------------------------------------------------

fn draw_trigger_editor(
    ui: &mut egui::Ui,
    trigger: &mut EffectTrigger,
    step_idx: usize,
    defined_tags: &[String],
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
    }
}

// ---------------------------------------------------------------------------
// Action editor
// ---------------------------------------------------------------------------

fn draw_action_editor(
    ui: &mut egui::Ui,
    action: &mut EffectAction,
    step_idx: usize,
    action_idx: usize,
    defined_tags: &[String],
    particle_presets: &[String],
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
                        ui.add(egui::DragValue::new(&mut offset.x).speed(0.1).prefix("x:"));
                        ui.add(egui::DragValue::new(&mut offset.y).speed(0.1).prefix("y:"));
                        ui.add(egui::DragValue::new(&mut offset.z).speed(0.1).prefix("z:"));
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
                    let selected_text = if preset.is_empty() { "(none)" } else { preset.as_str() };
                    egui::ComboBox::from_id_salt(format!("preset_{id_salt}"))
                        .selected_text(selected_text)
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for name in particle_presets {
                                ui.selectable_value(preset, name.clone(), name.as_str());
                            }
                        });
                    ui.end_row();

                    grid_label(ui, "Location");
                    let is_offset = matches!(at, SpawnLocation::Offset(_));
                    let mut loc_idx: usize = if is_offset { 0 } else { 1 };
                    egui::ComboBox::from_id_salt(format!("loc_{id_salt}"))
                        .selected_text(if is_offset { "Offset" } else { "Collision Point" })
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
                    }
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
                            egui::DragValue::new(&mut velocity.x).speed(0.1).prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut velocity.y).speed(0.1).prefix("y:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut velocity.z).speed(0.1).prefix("z:"),
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
                            egui::DragValue::new(&mut impulse.x).speed(0.1).prefix("x:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut impulse.y).speed(0.1).prefix("y:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut impulse.z).speed(0.1).prefix("z:"),
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
        EffectAction::SpawnDecal { tag, texture_path, at, scale } => {
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
                            texture_path.rsplit('/').next().unwrap_or(texture_path.as_str())
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

                    grid_label(ui, "Location");
                    let is_offset = matches!(at, SpawnLocation::Offset(_));
                    let mut loc_idx: usize = if is_offset { 0 } else { 1 };
                    egui::ComboBox::from_id_salt(format!("decal_loc_{id_salt}"))
                        .selected_text(if is_offset { "Offset" } else { "Collision Point" })
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

                    grid_label(ui, "Scale");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut scale.x).speed(0.1).prefix("x:"));
                        ui.add(egui::DragValue::new(&mut scale.y).speed(0.1).prefix("y:"));
                        ui.add(egui::DragValue::new(&mut scale.z).speed(0.1).prefix("z:"));
                    });
                    ui.end_row();
                });
        }
        EffectAction::SpawnGltf { tag, path, at, scale, rigid_body } => {
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
                                m.data.insert_temp(
                                    egui::Id::new("effect_gltf_browse"),
                                    true,
                                );
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

                    grid_label(ui, "Location");
                    let is_offset = matches!(at, SpawnLocation::Offset(_));
                    let mut loc_idx: usize = if is_offset { 0 } else { 1 };
                    egui::ComboBox::from_id_salt(format!("gltf_loc_{id_salt}"))
                        .selected_text(if is_offset { "Offset" } else { "Collision Point" })
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

                    grid_label(ui, "Scale");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut scale.x).speed(0.1).prefix("x:"));
                        ui.add(egui::DragValue::new(&mut scale.y).speed(0.1).prefix("y:"));
                        ui.add(egui::DragValue::new(&mut scale.z).speed(0.1).prefix("z:"));
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
    }
}
