//! VFX editor panel — Niagara-inspired card-based modifier editor.
//!
//! Color-coded sections:
//! - **Spawner**: orange accent
//! - **Init**: green accent
//! - **Update**: blue accent
//! - **Render**: purple accent

use bevy::prelude::*;
use bevy_editor_game::MaterialLibrary;
use bevy_egui::{egui, EguiPrimaryContextPass};
use bevy_vfx::curve::{Curve, CurveKey, Gradient, GradientKey, Interp};
use bevy_vfx::data::*;
use bevy_vfx::mesh_particles::MeshParticleStates;

use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::selection::Selected;
use crate::ui::command_palette::{CommandPaletteState, TexturePickResult, TextureSlot};
use crate::ui::theme::{colors, draw_pin_button, grid_label, panel, panel_frame};

pub struct VfxEditorPlugin;

impl Plugin for VfxEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_vfx_panel);
    }
}

// ---------------------------------------------------------------------------
// Card / section drawing helpers
// ---------------------------------------------------------------------------

const CARD_ROUNDING: u8 = 4;
const ACCENT_STRIPE_WIDTH: f32 = 3.0;

fn modifier_card(
    ui: &mut egui::Ui,
    label: &str,
    accent: egui::Color32,
    _id: egui::Id,
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
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{00d7}").color(colors::STATUS_ERROR),
                    )
                    .frame(false),
                );
                if btn.on_hover_text("Remove modifier").clicked() {
                    removed = true;
                }
            });
        });
        ui.add_space(2.0);
        body(ui);
    });

    let card_rect = resp.response.rect;
    let stripe = egui::Rect::from_min_max(
        card_rect.left_top(),
        egui::pos2(card_rect.left() + ACCENT_STRIPE_WIDTH, card_rect.bottom()),
    );
    ui.painter().rect_filled(
        stripe,
        egui::CornerRadius { nw: CARD_ROUNDING, sw: CARD_ROUNDING, ne: 0, se: 0 },
        accent,
    );
    ui.add_space(4.0);

    removed
}

fn category_header<T>(
    ui: &mut egui::Ui,
    label: &str,
    accent: egui::Color32,
    options: &[(&str, fn() -> T)],
    list: &mut Vec<T>,
) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong().size(12.0).color(accent));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(egui::RichText::new("+").strong().color(accent), |ui| {
                for (name, factory) in options {
                    if ui.button(*name).clicked() {
                        list.push(factory());
                        ui.close();
                    }
                }
            });
        });
    });
    ui.add_space(4.0);
}

fn gradient_preview_bar(ui: &mut egui::Ui, gradient: &Gradient) {
    let desired = egui::vec2(ui.available_width(), 16.0);
    let (rect, _resp) = ui.allocate_exact_size(desired, egui::Sense::hover());

    if gradient.keys.is_empty() {
        return;
    }

    let painter = ui.painter();
    let n_segments = 64;

    for seg in 0..n_segments {
        let t0 = seg as f32 / n_segments as f32;
        let t1 = (seg + 1) as f32 / n_segments as f32;
        let t_mid = (t0 + t1) * 0.5;

        let c = gradient.sample(t_mid);

        let color = egui::Color32::from_rgba_unmultiplied(
            (c.red * 255.0).clamp(0.0, 255.0) as u8,
            (c.green * 255.0).clamp(0.0, 255.0) as u8,
            (c.blue * 255.0).clamp(0.0, 255.0) as u8,
            (c.alpha * 255.0).clamp(0.0, 255.0) as u8,
        );

        let x0 = rect.left() + t0 * rect.width();
        let x1 = rect.left() + t1 * rect.width();
        let seg_rect = egui::Rect::from_min_max(
            egui::pos2(x0, rect.top()),
            egui::pos2(x1, rect.bottom()),
        );
        painter.rect_filled(seg_rect, 0.0, color);
    }

    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(2),
        egui::Stroke::new(1.0, colors::WIDGET_BORDER),
        egui::StrokeKind::Inside,
    );
}

// ---------------------------------------------------------------------------
// Main panel entry point
// ---------------------------------------------------------------------------

fn draw_vfx_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    let is_pinned = world.resource::<PinnedWindows>().0.contains(&EditorMode::Particle);
    if current_mode != EditorMode::Particle && !is_pinned {
        return;
    }

    // Get the single selected entity with a VfxSystem
    let entity = {
        let mut q = world.query_filtered::<Entity, (With<Selected>, With<VfxSystem>)>();
        match q.iter(world).next() {
            Some(e) => e,
            None => {
                draw_empty_panel(world, is_pinned, current_mode);
                return;
            }
        }
    };

    // Read which emitter tab is selected (stored in egui memory)
    let target_emitter_idx = {
        let ctx = world
            .query::<&mut bevy_egui::EguiContext>()
            .iter_mut(world)
            .next()
            .map(|mut c| c.get_mut().clone());
        ctx.map(|c| {
            c.memory(|mem| {
                mem.data
                    .get_temp::<usize>(egui::Id::new("vfx_selected_emitter"))
                    .unwrap_or(0)
            })
        })
        .unwrap_or(0)
    };

    // Check for texture pick result — apply to the selected emitter
    let pick_data = world.resource_mut::<TexturePickResult>().0.take();
    if let Some(pick) = pick_data {
        if pick.slot == TextureSlot::ParticleTexture && pick.entity == Some(entity) {
            if let Some(mut system) = world.get_mut::<VfxSystem>(entity) {
                if let Some(emitter) = system.emitters.get_mut(target_emitter_idx) {
                    if let RenderModule::Billboard(ref mut config) = emitter.render {
                        config.texture = Some(pick.path.clone());
                    }
                }
            }
        }
    }

    // Check for mesh shape pick result — apply to the selected emitter
    let shape_pick = world
        .resource_mut::<crate::ui::command_palette::MeshShapePickResult>()
        .0
        .take();
    if let Some(shape) = shape_pick {
        if let Some(mut system) = world.get_mut::<VfxSystem>(entity) {
            if let Some(emitter) = system.emitters.get_mut(target_emitter_idx) {
                if let RenderModule::Mesh(ref mut config) = emitter.render {
                    config.shape = shape;
                }
            }
        }
    }

    // Collect material library names for the mesh config ComboBox
    let mut material_names: Vec<String> = world
        .resource::<MaterialLibrary>()
        .materials
        .keys()
        .cloned()
        .collect();
    material_names.sort();

    // Clone the system data for editing
    let mut system = world.get::<VfxSystem>(entity).unwrap().clone();
    let original = system.clone();

    // Clone entity name for editing
    let mut entity_name = world
        .get::<Name>(entity)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();
    let original_name = entity_name.clone();

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
        && current_mode != EditorMode::Particle
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    let mut pin_toggled = false;
    let mut save_preset_clicked = false;
    let mut browse_presets_clicked = false;

    // Track which emitter is selected for editing
    let selected_emitter_id = ctx.memory(|mem| {
        mem.data.get_temp::<usize>(egui::Id::new("vfx_selected_emitter")).unwrap_or(0)
    });
    let mut selected_emitter = selected_emitter_id.min(system.emitters.len().saturating_sub(1));

    egui::Window::new("VFX System")
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .min_height(available_height)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            // Pin button and preset buttons
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                pin_toggled = draw_pin_button(ui, is_pinned);
                if ui
                    .button(egui::RichText::new("Save Preset").small().color(colors::ACCENT_GREEN))
                    .on_hover_text("Save current VFX system as a named preset")
                    .clicked()
                {
                    save_preset_clicked = true;
                }
                if ui
                    .button(egui::RichText::new("Browse").small().color(colors::ACCENT_ORANGE))
                    .on_hover_text("Browse VFX presets (F)")
                    .clicked()
                {
                    browse_presets_clicked = true;
                }
            });

            // Editable entity name
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut entity_name)
                    .font(egui::FontId::proportional(16.0))
                    .text_color(colors::TEXT_PRIMARY)
                    .margin(egui::vec2(8.0, 6.0)),
            );
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("ID: {:?}", entity))
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            });

            // System-level settings
            ui.add_space(4.0);
            egui::Grid::new("vfx_system_settings")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Duration");
                    ui.add(
                        egui::DragValue::new(&mut system.duration)
                            .speed(0.1)
                            .range(0.0..=600.0)
                            .max_decimals(1)
                            .suffix(" s"),
                    );
                    ui.end_row();

                    grid_label(ui, "Looping");
                    ui.checkbox(&mut system.looping, "");
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.separator();

            // Emitter list
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EMITTERS").strong().size(12.0).color(colors::TEXT_SECONDARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(egui::RichText::new("+ Add").color(colors::ACCENT_GREEN)).clicked() {
                        let idx = system.emitters.len();
                        system.emitters.push(EmitterDef {
                            name: format!("Emitter {}", idx + 1),
                            ..default()
                        });
                        selected_emitter = idx;
                    }
                });
            });

            // Draw emitter list items
            let mut remove_emitter = None;
            let emitter_count = system.emitters.len();
            for (i, emitter) in system.emitters.iter_mut().enumerate() {
                let is_selected = i == selected_emitter;
                let bg = if is_selected { colors::SELECTION_BG } else { colors::BG_DARK };
                let frame = egui::Frame::new()
                    .fill(bg)
                    .corner_radius(egui::CornerRadius::same(3))
                    .inner_margin(egui::Margin::symmetric(6, 3));

                frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Clickable name
                        if ui.selectable_label(is_selected, &emitter.name).clicked() {
                            selected_emitter = i;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Remove button (only if > 1 emitter)
                            if emitter_count > 1 {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("\u{00d7}").color(colors::STATUS_ERROR),
                                    ).frame(false)
                                ).on_hover_text("Remove emitter").clicked() {
                                    remove_emitter = Some(i);
                                }
                            }

                            // Enable toggle
                            ui.checkbox(&mut emitter.enabled, "");

                            // Render mode badge
                            ui.label(
                                egui::RichText::new(emitter.render.label())
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                        });
                    });
                });
            }

            if let Some(idx) = remove_emitter {
                system.emitters.remove(idx);
                if selected_emitter >= system.emitters.len() {
                    selected_emitter = system.emitters.len().saturating_sub(1);
                }
            }

            ui.add_space(4.0);
            ui.separator();

            // Selected emitter detail editor
            if let Some(emitter) = system.emitters.get_mut(selected_emitter) {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(280.0);

                        // Emitter name
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            grid_label(ui, "Name");
                            ui.text_edit_singleline(&mut emitter.name);
                        });

                        // Emitter settings
                        draw_emitter_settings(ui, emitter, selected_emitter);
                        ui.add_space(4.0);

                        // Spawner card (orange)
                        draw_spawner_card(ui, &mut emitter.spawn);

                        // Init section (green)
                        draw_init_section(ui, &mut emitter.init);

                        // Update section (blue)
                        draw_update_section(ui, &mut emitter.update);

                        // Render section (purple)
                        if draw_render_section(ui, &mut emitter.render, &material_names) {
                            ui.ctx().memory_mut(|mem| {
                                mem.data.insert_temp(egui::Id::new("vfx_mesh_shape_browse"), true);
                            });
                        }

                        ui.add_space(8.0);
                    });
            }
        });

    // Store selected emitter index
    ctx.memory_mut(|mem| {
        mem.data.insert_temp(egui::Id::new("vfx_selected_emitter"), selected_emitter);
    });

    // Check for texture browse request
    let browse_texture = ctx.memory(|mem| {
        mem.data.get_temp::<bool>(egui::Id::new("vfx_texture_browse")).unwrap_or(false)
    });
    if browse_texture {
        ctx.memory_mut(|mem| {
            mem.data.insert_temp(egui::Id::new("vfx_texture_browse"), false);
        });
        world
            .resource_mut::<CommandPaletteState>()
            .open_pick_texture(TextureSlot::ParticleTexture, Some(entity));
    }

    // Check for mesh shape browse request
    let browse_mesh_shape = ctx.memory(|mem| {
        mem.data.get_temp::<bool>(egui::Id::new("vfx_mesh_shape_browse")).unwrap_or(false)
    });
    if browse_mesh_shape {
        ctx.memory_mut(|mem| {
            mem.data.insert_temp(egui::Id::new("vfx_mesh_shape_browse"), false);
        });
        world
            .resource_mut::<CommandPaletteState>()
            .open_mesh_shape_picker();
    }

    // Write back if changed
    let changed = ron::to_string(&system).ok() != ron::to_string(&original).ok();
    if changed {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert(system.clone());
        }

        // Apply library materials to existing particle children using the full
        // material pipeline (supports custom shader extensions).
        for (emitter_idx, emitter) in system.emitters.iter().enumerate() {
            if let RenderModule::Mesh(ref config) = emitter.render {
                if let Some(ref mat_name) = config.material_path {
                    let mat_ref = bevy_editor_game::MaterialRef::Library(mat_name.clone());
                    let def = {
                        let library = world.resource::<MaterialLibrary>();
                        crate::materials::resolve_material_ref(&mat_ref, library).cloned()
                    };

                    if let Some(def) = def {
                        // Update existing particle children with the full material
                        if let Some(states) = world.get::<MeshParticleStates>(entity) {
                            let entities: Vec<Entity> = states
                                .entries
                                .iter()
                                .filter(|s| s.emitter_index == emitter_idx)
                                .flat_map(|s| s.particles.iter().map(|p| p.entity))
                                .collect();
                            for child in entities {
                                crate::materials::apply_material_def_standalone(
                                    world, child, &def,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    if entity_name != original_name {
        if let Some(mut name) = world.get_mut::<Name>(entity) {
            name.set(entity_name.clone());
        }
    }

    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::Particle) {
            pinned.0.insert(EditorMode::Particle);
        }
    }

    if save_preset_clicked {
        let mut library = world.resource_mut::<VfxLibrary>();
        let preset_name = {
            let base = if entity_name.is_empty() { "New VFX".to_string() } else { entity_name };
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
        library.effects.insert(preset_name.clone(), system);
        info!("Saved VFX preset '{}'", preset_name);
    }

    if browse_presets_clicked {
        world
            .resource_mut::<CommandPaletteState>()
            .open_particle_preset();
    }
}

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
        && current_mode != EditorMode::Particle
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    let mut pin_toggled = false;

    egui::Window::new("VFX System")
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .min_height(available_height)
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
                    egui::RichText::new("Select a VFX entity")
                        .color(colors::TEXT_MUTED)
                        .italics(),
                );
            });
        });

    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::Particle) {
            pinned.0.insert(EditorMode::Particle);
        }
    }
}

// ---------------------------------------------------------------------------
// Emitter settings grid
// ---------------------------------------------------------------------------

fn draw_emitter_settings(ui: &mut egui::Ui, emitter: &mut EmitterDef, emitter_idx: usize) {
    egui::Grid::new(format!("emitter_settings_{emitter_idx}"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Capacity");
            let mut cap = emitter.capacity as i32;
            if ui.add(egui::DragValue::new(&mut cap).range(1..=1_000_000).speed(100)).changed() {
                emitter.capacity = cap.max(1) as u32;
            }
            ui.end_row();

            grid_label(ui, "Sim Space");
            egui::ComboBox::from_id_salt(format!("sim_space_{emitter_idx}"))
                .selected_text(emitter.sim_space.label())
                .show_ui(ui, |ui| {
                    for mode in &SimSpace::ALL {
                        ui.selectable_value(&mut emitter.sim_space, *mode, mode.label());
                    }
                });
            ui.end_row();

            grid_label(ui, "Alpha");
            egui::ComboBox::from_id_salt(format!("alpha_mode_{emitter_idx}"))
                .selected_text(emitter.alpha_mode.label())
                .show_ui(ui, |ui| {
                    for mode in &VfxAlphaMode::ALL {
                        ui.selectable_value(&mut emitter.alpha_mode, *mode, mode.label());
                    }
                });
            ui.end_row();
        });
}

// ---------------------------------------------------------------------------
// Spawner card (orange)
// ---------------------------------------------------------------------------

fn draw_spawner_card(ui: &mut egui::Ui, spawn: &mut SpawnModule) {
    let accent = colors::ACCENT_ORANGE;

    let frame = egui::Frame::new()
        .fill(colors::BG_MEDIUM)
        .corner_radius(egui::CornerRadius::same(CARD_ROUNDING))
        .inner_margin(egui::Margin::same(6));

    let resp = frame.show(ui, |ui| {
        ui.label(egui::RichText::new("Spawner").strong().color(colors::TEXT_PRIMARY));
        ui.add_space(2.0);

        let mode_idx = match spawn {
            SpawnModule::Rate(_) => 0,
            SpawnModule::Burst { .. } => 1,
            SpawnModule::Once(_) => 2,
            SpawnModule::Distance { .. } => 3,
        };
        let mut new_mode = mode_idx;

        egui::Grid::new("spawner_mode_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Mode");
                egui::ComboBox::from_id_salt("spawner_mode")
                    .selected_text(spawn.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut new_mode, 0, "Rate");
                        ui.selectable_value(&mut new_mode, 1, "Burst");
                        ui.selectable_value(&mut new_mode, 2, "Once");
                        ui.selectable_value(&mut new_mode, 3, "Distance");
                    });
                ui.end_row();
            });

        if new_mode != mode_idx {
            *spawn = match new_mode {
                0 => SpawnModule::Rate(50.0),
                1 => SpawnModule::Burst { count: 30, interval: 0.5, max_cycles: None },
                2 => SpawnModule::Once(100),
                _ => SpawnModule::Distance { spacing: 0.5 },
            };
        }

        egui::Grid::new("spawner_values_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| match spawn {
                SpawnModule::Rate(rate) => {
                    grid_label(ui, "Rate");
                    ui.add(egui::DragValue::new(rate).speed(1.0).range(0.1..=100000.0).max_decimals(1));
                    ui.end_row();
                }
                SpawnModule::Once(count) => {
                    grid_label(ui, "Count");
                    let mut c = *count as i32;
                    if ui.add(egui::DragValue::new(&mut c).speed(1.0).range(1..=100000)).changed() {
                        *count = c.max(1) as u32;
                    }
                    ui.end_row();
                }
                SpawnModule::Burst { count, interval, max_cycles } => {
                    grid_label(ui, "Count");
                    let mut c = *count as i32;
                    if ui.add(egui::DragValue::new(&mut c).speed(1.0).range(1..=100000)).changed() {
                        *count = c.max(1) as u32;
                    }
                    ui.end_row();

                    grid_label(ui, "Interval");
                    ui.add(egui::DragValue::new(interval).speed(0.1).range(0.01..=60.0).max_decimals(2).suffix(" s"));
                    ui.end_row();

                    grid_label(ui, "Max Cycles");
                    let mut has_max = max_cycles.is_some();
                    let mut val = max_cycles.unwrap_or(10) as i32;
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut has_max, "");
                        if has_max {
                            ui.add(egui::DragValue::new(&mut val).range(1..=10000));
                            *max_cycles = Some(val.max(1) as u32);
                        } else {
                            *max_cycles = None;
                            ui.label(egui::RichText::new("Infinite").color(colors::TEXT_MUTED));
                        }
                    });
                    ui.end_row();
                }
                SpawnModule::Distance { spacing } => {
                    grid_label(ui, "Spacing");
                    ui.add(egui::DragValue::new(spacing).speed(0.01).range(0.01..=100.0).max_decimals(2).suffix(" m"));
                    ui.end_row();
                }
            });
    });

    let card_rect = resp.response.rect;
    let stripe = egui::Rect::from_min_max(
        card_rect.left_top(),
        egui::pos2(card_rect.left() + ACCENT_STRIPE_WIDTH, card_rect.bottom()),
    );
    ui.painter().rect_filled(
        stripe,
        egui::CornerRadius { nw: CARD_ROUNDING, sw: CARD_ROUNDING, ne: 0, se: 0 },
        accent,
    );
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Init section (green)
// ---------------------------------------------------------------------------

fn draw_init_section(ui: &mut egui::Ui, modifiers: &mut Vec<InitModule>) {
    category_header(ui, "INIT", colors::ACCENT_GREEN, InitModule::ADD_OPTIONS, modifiers);

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("init_card_{i}"));
        if modifier_card(ui, m.label(), colors::ACCENT_GREEN, id, |ui| {
            draw_init_body(ui, m, i);
        }) {
            remove_idx = Some(i);
        }
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_init_body(ui: &mut egui::Ui, m: &mut InitModule, idx: usize) {
    match m {
        InitModule::SetLifetime(range) => {
            draw_scalar_range(ui, "Lifetime", range, 0.1, 0.01..=120.0, idx);
        }
        InitModule::SetPosition(shape) => {
            draw_shape_emitter(ui, shape, idx);
        }
        InitModule::SetVelocity(mode) => {
            draw_velocity_mode(ui, mode, idx);
        }
        InitModule::SetColor(source) => {
            match source {
                ColorSource::Constant(c) => {
                    draw_linear_rgba_color(ui, "Color", c);
                }
                ColorSource::RandomFromGradient(g) => {
                    gradient_preview_bar(ui, g);
                    draw_gradient_keys(ui, &mut g.keys, idx);
                }
            }
        }
        InitModule::SetSize(range) => {
            draw_scalar_range(ui, "Size", range, 0.01, 0.001..=100.0, idx);
        }
        InitModule::SetRotation(range) => {
            draw_scalar_range(ui, "Rotation", range, 0.01, -6.28..=6.28, idx);
        }
        InitModule::SetOrientation(mode) => {
            egui::Grid::new(format!("orient_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Mode");
                    egui::ComboBox::from_id_salt(format!("orient_mode_{idx}"))
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for m in &OrientMode::ALL {
                                ui.selectable_value(mode, *m, m.label());
                            }
                        });
                    ui.end_row();
                });
        }
        InitModule::SetScale3d { x, y, z } => {
            draw_scalar_range(ui, "Scale X", x, 0.01, 0.001..=100.0, idx);
            draw_scalar_range(ui, "Scale Y", y, 0.01, 0.001..=100.0, idx);
            draw_scalar_range(ui, "Scale Z", z, 0.01, 0.001..=100.0, idx);
        }
        InitModule::SetUvScale(scale) => {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut scale[0]).speed(0.1).prefix("u:").max_decimals(2));
                ui.add(egui::DragValue::new(&mut scale[1]).speed(0.1).prefix("v:").max_decimals(2));
            });
        }
        InitModule::InheritVelocity { ratio } => {
            egui::Grid::new(format!("inherit_vel_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Ratio");
                    ui.add(egui::DragValue::new(ratio).speed(0.01).range(0.0..=2.0).max_decimals(2));
                    ui.end_row();
                });
        }
    }
}

fn draw_shape_emitter(ui: &mut egui::Ui, shape: &mut ShapeEmitter, idx: usize) {
    let shape_idx = match shape {
        ShapeEmitter::Point(_) => 0,
        ShapeEmitter::Sphere { .. } => 1,
        ShapeEmitter::Box { .. } => 2,
        ShapeEmitter::Circle { .. } => 3,
        ShapeEmitter::Cone { .. } => 4,
        ShapeEmitter::Edge { .. } => 5,
    };
    let mut new_shape = shape_idx;

    egui::Grid::new(format!("shape_sel_{idx}"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Shape");
            egui::ComboBox::from_id_salt(format!("shape_mode_{idx}"))
                .selected_text(shape.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut new_shape, 0, "Point");
                    ui.selectable_value(&mut new_shape, 1, "Sphere");
                    ui.selectable_value(&mut new_shape, 2, "Box");
                    ui.selectable_value(&mut new_shape, 3, "Circle");
                    ui.selectable_value(&mut new_shape, 4, "Cone");
                    ui.selectable_value(&mut new_shape, 5, "Edge");
                });
            ui.end_row();
        });

    if new_shape != shape_idx {
        *shape = match new_shape {
            0 => ShapeEmitter::Point(Vec3::ZERO),
            1 => ShapeEmitter::Sphere { center: Vec3::ZERO, radius: ScalarRange::Constant(0.5) },
            2 => ShapeEmitter::Box { center: Vec3::ZERO, half_extents: Vec3::splat(0.5) },
            3 => ShapeEmitter::Circle { center: Vec3::ZERO, axis: Vec3::Y, radius: ScalarRange::Constant(0.5) },
            4 => ShapeEmitter::Cone { angle: 0.5, radius: 0.5, height: 1.0 },
            _ => ShapeEmitter::Edge { start: Vec3::new(-0.5, 0.0, 0.0), end: Vec3::new(0.5, 0.0, 0.0) },
        };
    }

    match shape {
        ShapeEmitter::Sphere { center, radius } => {
            draw_vec3_grid(ui, "Center", center, 0.1, idx, "sphere_c");
            draw_scalar_range(ui, "Radius", radius, 0.1, 0.0..=1000.0, idx);
        }
        ShapeEmitter::Box { center, half_extents } => {
            draw_vec3_grid(ui, "Center", center, 0.1, idx, "box_c");
            draw_vec3_grid(ui, "Half Size", half_extents, 0.1, idx, "box_hs");
        }
        ShapeEmitter::Circle { center, axis, radius } => {
            draw_vec3_grid(ui, "Center", center, 0.1, idx, "circle_c");
            draw_vec3_grid(ui, "Axis", axis, 0.01, idx, "circle_a");
            draw_scalar_range(ui, "Radius", radius, 0.1, 0.0..=1000.0, idx);
        }
        ShapeEmitter::Point(p) => {
            draw_vec3_grid(ui, "Position", p, 0.1, idx, "point_p");
        }
        ShapeEmitter::Edge { start, end } => {
            draw_vec3_grid(ui, "Start", start, 0.1, idx, "edge_s");
            draw_vec3_grid(ui, "End", end, 0.1, idx, "edge_e");
        }
        ShapeEmitter::Cone { angle, radius, height } => {
            egui::Grid::new(format!("cone_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Angle");
                ui.add(egui::DragValue::new(angle).speed(0.01).max_decimals(2));
                ui.end_row();
                grid_label(ui, "Radius");
                ui.add(egui::DragValue::new(radius).speed(0.1).max_decimals(2));
                ui.end_row();
                grid_label(ui, "Height");
                ui.add(egui::DragValue::new(height).speed(0.1).max_decimals(2));
                ui.end_row();
            });
        }
    }
}

fn draw_velocity_mode(ui: &mut egui::Ui, mode: &mut VelocityMode, idx: usize) {
    let mode_idx = match mode {
        VelocityMode::Radial { .. } => 0,
        VelocityMode::Directional { .. } => 1,
        VelocityMode::Tangent { .. } => 2,
        VelocityMode::Cone { .. } => 3,
        VelocityMode::Random { .. } => 4,
    };
    let mut new_mode = mode_idx;

    egui::Grid::new(format!("vel_sel_{idx}"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Mode");
            egui::ComboBox::from_id_salt(format!("vel_mode_{idx}"))
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut new_mode, 0, "Radial");
                    ui.selectable_value(&mut new_mode, 1, "Directional");
                    ui.selectable_value(&mut new_mode, 2, "Tangent");
                    ui.selectable_value(&mut new_mode, 3, "Cone");
                    ui.selectable_value(&mut new_mode, 4, "Random");
                });
            ui.end_row();
        });

    if new_mode != mode_idx {
        *mode = match new_mode {
            0 => VelocityMode::Radial { center: Vec3::ZERO, speed: ScalarRange::Random(1.0, 3.0) },
            1 => VelocityMode::Directional { direction: Vec3::Y, speed: ScalarRange::Random(1.0, 3.0) },
            2 => VelocityMode::Tangent { axis: Vec3::Y, speed: ScalarRange::Random(1.0, 3.0) },
            3 => VelocityMode::Cone { direction: Vec3::Y, angle: 0.3, speed: ScalarRange::Random(1.0, 3.0) },
            _ => VelocityMode::Random { speed: ScalarRange::Random(1.0, 3.0) },
        };
    }

    match mode {
        VelocityMode::Radial { center, speed } => {
            draw_vec3_grid(ui, "Center", center, 0.1, idx, "vrad_c");
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
        VelocityMode::Directional { direction, speed } => {
            draw_vec3_grid(ui, "Direction", direction, 0.01, idx, "vdir_d");
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
        VelocityMode::Tangent { axis, speed } => {
            draw_vec3_grid(ui, "Axis", axis, 0.01, idx, "vtan_a");
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
        VelocityMode::Cone { direction, angle, speed } => {
            draw_vec3_grid(ui, "Direction", direction, 0.01, idx, "vcone_d");
            egui::Grid::new(format!("vcone_a_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Angle");
                ui.add(egui::DragValue::new(angle).speed(0.01).max_decimals(2));
                ui.end_row();
            });
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
        VelocityMode::Random { speed } => {
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
    }
}

// ---------------------------------------------------------------------------
// Update section (blue)
// ---------------------------------------------------------------------------

fn draw_update_section(ui: &mut egui::Ui, modifiers: &mut Vec<UpdateModule>) {
    category_header(ui, "UPDATE", colors::ACCENT_BLUE, UpdateModule::ADD_OPTIONS, modifiers);

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("update_card_{i}"));
        if modifier_card(ui, m.label(), colors::ACCENT_BLUE, id, |ui| {
            draw_update_body(ui, m, i);
        }) {
            remove_idx = Some(i);
        }
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_update_body(ui: &mut egui::Ui, m: &mut UpdateModule, idx: usize) {
    match m {
        UpdateModule::Gravity(g) => {
            draw_vec3_grid(ui, "Gravity", g, 0.1, idx, "gravity");
        }
        UpdateModule::ConstantForce(f) => {
            draw_vec3_grid(ui, "Force", f, 0.1, idx, "const_force");
        }
        UpdateModule::Drag(d) => {
            egui::Grid::new(format!("drag_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Drag");
                ui.add(egui::DragValue::new(d).speed(0.1).range(0.0..=100.0).max_decimals(3));
                ui.end_row();
            });
        }
        UpdateModule::Noise { strength, frequency, scroll } => {
            egui::Grid::new(format!("noise_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Strength");
                ui.add(egui::DragValue::new(strength).speed(0.1).max_decimals(3));
                ui.end_row();
                grid_label(ui, "Frequency");
                ui.add(egui::DragValue::new(frequency).speed(0.1).max_decimals(3));
                ui.end_row();
            });
            draw_vec3_grid(ui, "Scroll", scroll, 0.1, idx, "noise_scroll");
        }
        UpdateModule::OrbitAround { axis, speed, radius_decay } => {
            draw_vec3_grid(ui, "Axis", axis, 0.01, idx, "orbit_a");
            egui::Grid::new(format!("orbit_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Speed");
                ui.add(egui::DragValue::new(speed).speed(0.1).max_decimals(3));
                ui.end_row();
                grid_label(ui, "Decay");
                ui.add(egui::DragValue::new(radius_decay).speed(0.01).max_decimals(3));
                ui.end_row();
            });
        }
        UpdateModule::Attract { target, strength, falloff } => {
            draw_vec3_grid(ui, "Target", target, 0.1, idx, "attract_t");
            egui::Grid::new(format!("attract_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Strength");
                ui.add(egui::DragValue::new(strength).speed(0.1).max_decimals(3));
                ui.end_row();
                grid_label(ui, "Falloff");
                ui.add(egui::DragValue::new(falloff).speed(0.1).range(0.0..=10.0).max_decimals(3));
                ui.end_row();
            });
        }
        UpdateModule::KillZone { shape, invert } => {
            match shape {
                KillShape::Sphere { center, radius } => {
                    draw_vec3_grid(ui, "Center", center, 0.1, idx, "kz_c");
                    egui::Grid::new(format!("kz_r_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                        grid_label(ui, "Radius");
                        ui.add(egui::DragValue::new(radius).speed(0.1).max_decimals(3));
                        ui.end_row();
                    });
                }
                KillShape::Box { center, half_extents } => {
                    draw_vec3_grid(ui, "Center", center, 0.1, idx, "kzb_c");
                    draw_vec3_grid(ui, "Half Size", half_extents, 0.1, idx, "kzb_hs");
                }
            }
            ui.checkbox(invert, "Invert (kill outside)");
        }
        UpdateModule::SizeByLife(curve) => {
            draw_curve_editor(ui, curve, idx, "size_life");
        }
        UpdateModule::ColorByLife(gradient) => {
            gradient_preview_bar(ui, gradient);
            ui.add_space(4.0);
            draw_gradient_keys(ui, &mut gradient.keys, idx);
        }
        UpdateModule::SizeBySpeed { min_speed, max_speed, min_size, max_size } => {
            egui::Grid::new(format!("sbs_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Min Speed");
                ui.add(egui::DragValue::new(min_speed).speed(0.1).max_decimals(3));
                ui.end_row();
                grid_label(ui, "Max Speed");
                ui.add(egui::DragValue::new(max_speed).speed(0.1).max_decimals(3));
                ui.end_row();
                grid_label(ui, "Min Size");
                ui.add(egui::DragValue::new(min_size).speed(0.01).max_decimals(3));
                ui.end_row();
                grid_label(ui, "Max Size");
                ui.add(egui::DragValue::new(max_size).speed(0.01).max_decimals(3));
                ui.end_row();
            });
        }
        UpdateModule::RotateByVelocity => {
            ui.label(egui::RichText::new("(no parameters)").color(colors::TEXT_MUTED).italics());
        }
        UpdateModule::TangentAccel { origin, axis, accel } => {
            draw_vec3_grid(ui, "Origin", origin, 0.1, idx, "tan_o");
            draw_vec3_grid(ui, "Axis", axis, 0.01, idx, "tan_a");
            egui::Grid::new(format!("tan_v_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Accel");
                ui.add(egui::DragValue::new(accel).speed(0.1).max_decimals(3));
                ui.end_row();
            });
        }
        UpdateModule::RadialAccel { origin, accel } => {
            draw_vec3_grid(ui, "Origin", origin, 0.1, idx, "rad_o");
            egui::Grid::new(format!("rad_v_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Accel");
                ui.add(egui::DragValue::new(accel).speed(0.1).max_decimals(3));
                ui.end_row();
            });
        }
        UpdateModule::Spin { axis, speed } => {
            draw_vec3_grid(ui, "Axis", axis, 0.01, idx, "spin_a");
            egui::Grid::new(format!("spin_s_{idx}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                grid_label(ui, "Speed");
                ui.add(egui::DragValue::new(speed).speed(0.1).max_decimals(3).suffix(" rad/s"));
                ui.end_row();
            });
        }
        UpdateModule::UvScroll { speed } => {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut speed[0]).speed(0.01).prefix("u:").max_decimals(3));
                ui.add(egui::DragValue::new(&mut speed[1]).speed(0.01).prefix("v:").max_decimals(3));
            });
        }
        UpdateModule::Scale3dByLife { x, y, z } => {
            ui.label(egui::RichText::new("X").color(colors::AXIS_X));
            draw_curve_editor(ui, x, idx, "s3d_x");
            ui.label(egui::RichText::new("Y").color(colors::AXIS_Y));
            draw_curve_editor(ui, y, idx, "s3d_y");
            ui.label(egui::RichText::new("Z").color(colors::AXIS_Z));
            draw_curve_editor(ui, z, idx, "s3d_z");
        }
        UpdateModule::OffsetByLife { x, y, z } => {
            ui.label(egui::RichText::new("X").color(colors::AXIS_X));
            draw_curve_editor(ui, x, idx, "off_x");
            ui.label(egui::RichText::new("Y").color(colors::AXIS_Y));
            draw_curve_editor(ui, y, idx, "off_y");
            ui.label(egui::RichText::new("Z").color(colors::AXIS_Z));
            draw_curve_editor(ui, z, idx, "off_z");
        }
        UpdateModule::EmissiveOverLife(gradient) => {
            gradient_preview_bar(ui, gradient);
            ui.add_space(4.0);
            draw_gradient_keys(ui, &mut gradient.keys, idx);
        }
    }
}

// ---------------------------------------------------------------------------
// Render section (purple)
// ---------------------------------------------------------------------------

fn draw_render_section(ui: &mut egui::Ui, render: &mut RenderModule, material_names: &[String]) -> bool {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("RENDER").strong().size(12.0).color(colors::ACCENT_PURPLE));
    });
    ui.add_space(4.0);

    // Render mode selector
    let mode_idx = match render {
        RenderModule::Billboard(_) => 0,
        RenderModule::Ribbon(_) => 1,
        RenderModule::Mesh(_) => 2,
    };
    let mut new_mode = mode_idx;

    egui::Grid::new("render_mode_grid")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Mode");
            egui::ComboBox::from_id_salt("render_mode")
                .selected_text(render.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut new_mode, 0, "Billboard");
                    ui.selectable_value(&mut new_mode, 1, "Ribbon");
                    ui.selectable_value(&mut new_mode, 2, "Mesh");
                });
            ui.end_row();
        });

    if new_mode != mode_idx {
        *render = match new_mode {
            0 => RenderModule::Billboard(BillboardConfig::default()),
            1 => RenderModule::Ribbon(RibbonConfig::default()),
            _ => RenderModule::Mesh(MeshParticleConfig::default()),
        };
    }

    match render {
        RenderModule::Billboard(config) => {
            draw_billboard_config(ui, config);
            false
        }
        RenderModule::Ribbon(config) => {
            draw_ribbon_config(ui, config);
            false
        }
        RenderModule::Mesh(config) => draw_mesh_config(ui, config, material_names),
    }
}

fn draw_billboard_config(ui: &mut egui::Ui, config: &mut BillboardConfig) {
    egui::Grid::new("billboard_config")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Orient");
            egui::ComboBox::from_id_salt("bb_orient")
                .selected_text(config.orient.label())
                .show_ui(ui, |ui| {
                    for mode in &BillboardOrient::ALL {
                        ui.selectable_value(&mut config.orient, *mode, mode.label());
                    }
                });
            ui.end_row();

            grid_label(ui, "Soft Particles");
            ui.add(
                egui::DragValue::new(&mut config.soft_particle_distance)
                    .speed(0.01)
                    .range(0.0..=10.0)
                    .max_decimals(2),
            );
            ui.end_row();
        });

    // Texture
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Texture").color(colors::TEXT_SECONDARY));
        match &config.texture {
            Some(p) => {
                ui.label(egui::RichText::new(p.as_str()).color(colors::TEXT_PRIMARY).small());
            }
            None => {
                ui.label(egui::RichText::new("None").color(colors::TEXT_MUTED).italics());
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(egui::RichText::new("Browse").color(colors::ACCENT_PURPLE)).clicked() {
            ui.memory_mut(|mem| {
                mem.data.insert_temp(egui::Id::new("vfx_texture_browse"), true);
            });
        }
        if config.texture.is_some() && ui.button(egui::RichText::new("Clear").color(colors::STATUS_ERROR)).clicked() {
            config.texture = None;
        }
    });
}

fn draw_ribbon_config(ui: &mut egui::Ui, config: &mut RibbonConfig) {
    egui::Grid::new("ribbon_config")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, "Segments");
            let mut s = config.segments_per_particle as i32;
            if ui.add(egui::DragValue::new(&mut s).range(2..=64)).changed() {
                config.segments_per_particle = s.max(2) as u32;
            }
            ui.end_row();

            grid_label(ui, "Face Camera");
            ui.checkbox(&mut config.face_camera, "");
            ui.end_row();
        });
}

fn draw_mesh_config(
    ui: &mut egui::Ui,
    config: &mut MeshParticleConfig,
    material_names: &[String],
) -> bool {
    let mut open_shape_picker = false;
    egui::Grid::new("mesh_config")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            // Shape selector — opens fuzzy palette
            grid_label(ui, "Shape");
            let current_label = config.shape.label().to_string();
            if ui
                .add(egui::Button::new(&current_label).min_size(egui::vec2(120.0, 0.0)))
                .clicked()
            {
                open_shape_picker = true;
            }
            ui.end_row();

            // Material selector
            grid_label(ui, "Material");
            let current_mat = config
                .material_path
                .as_deref()
                .unwrap_or("(base color)");
            egui::ComboBox::from_id_salt("mesh_material")
                .selected_text(current_mat)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(config.material_path.is_none(), "(base color)")
                        .clicked()
                    {
                        config.material_path = None;
                    }
                    for name in material_names {
                        if ui
                            .selectable_label(
                                config.material_path.as_deref() == Some(name.as_str()),
                                name,
                            )
                            .clicked()
                        {
                            config.material_path = Some(name.clone());
                        }
                    }
                });
            ui.end_row();

            // Base color (only shown when no library material)
            if config.material_path.is_none() {
                grid_label(ui, "Base Color");
                let mut rgba = [
                    config.base_color.red,
                    config.base_color.green,
                    config.base_color.blue,
                    config.base_color.alpha,
                ];
                if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                    config.base_color = LinearRgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
                }
                ui.end_row();
            }

            // Physics collision
            grid_label(ui, "Collide");
            ui.checkbox(&mut config.collide, "");
            ui.end_row();

            if config.collide {
                grid_label(ui, "Restitution");
                ui.add(
                    egui::DragValue::new(&mut config.restitution)
                        .speed(0.01)
                        .range(0.0..=1.0)
                        .max_decimals(2),
                );
                ui.end_row();
            }

            grid_label(ui, "Shadows");
            ui.checkbox(&mut config.cast_shadows, "");
            ui.end_row();
        });
    open_shape_picker
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn draw_vec3_grid(ui: &mut egui::Ui, label: &str, val: &mut Vec3, speed: f64, idx: usize, salt: &str) {
    egui::Grid::new(format!("v3_{salt}_{idx}"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, label);
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut val.x).speed(speed).prefix("x:").max_decimals(3));
                ui.add(egui::DragValue::new(&mut val.y).speed(speed).prefix("y:").max_decimals(3));
                ui.add(egui::DragValue::new(&mut val.z).speed(speed).prefix("z:").max_decimals(3));
            });
            ui.end_row();
        });
}

fn draw_scalar_range(
    ui: &mut egui::Ui,
    label: &str,
    range: &mut ScalarRange,
    speed: f64,
    clamp: std::ops::RangeInclusive<f32>,
    _idx: usize,
) {
    let is_random = matches!(range, ScalarRange::Random(_, _));
    let mut toggle = is_random;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
        if ui.checkbox(&mut toggle, "Random").changed() {
            if toggle {
                let val = match range {
                    ScalarRange::Constant(v) => *v,
                    ScalarRange::Random(_, _) => unreachable!(),
                };
                *range = ScalarRange::Random(val * 0.5, val * 1.5);
            } else {
                let val = match range {
                    ScalarRange::Random(a, b) => (*a + *b) / 2.0,
                    ScalarRange::Constant(_) => unreachable!(),
                };
                *range = ScalarRange::Constant(val);
            }
        }
    });

    match range {
        ScalarRange::Constant(val) => {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.add(egui::DragValue::new(val).speed(speed).range(clamp).max_decimals(3));
            });
        }
        ScalarRange::Random(min, max) => {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(egui::RichText::new("min").color(colors::TEXT_MUTED));
                ui.add(egui::DragValue::new(min).speed(speed).range(clamp.clone()).max_decimals(3));
                ui.label(egui::RichText::new("max").color(colors::TEXT_MUTED));
                ui.add(egui::DragValue::new(max).speed(speed).range(clamp).max_decimals(3));
            });
        }
    }
}

fn draw_linear_rgba_color(ui: &mut egui::Ui, label: &str, color: &mut LinearRgba) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
        let mut rgba = [color.red, color.green, color.blue, color.alpha];
        if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
            *color = LinearRgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    });
}

fn draw_gradient_keys(ui: &mut egui::Ui, keys: &mut Vec<GradientKey>, _mod_idx: usize) {
    let mut remove_key = None;
    for (i, key) in keys.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("t:").color(colors::TEXT_MUTED));
            ui.add(egui::DragValue::new(&mut key.time).range(0.0..=1.0).speed(0.01).max_decimals(2));
            let mut rgba = [key.color.red, key.color.green, key.color.blue, key.color.alpha];
            if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                key.color = LinearRgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
            if ui.add(egui::Button::new(egui::RichText::new("\u{00d7}").color(colors::STATUS_ERROR)).frame(false)).on_hover_text("Remove key").clicked() {
                remove_key = Some(i);
            }
        });
    }

    if let Some(idx) = remove_key {
        if keys.len() > 1 {
            keys.remove(idx);
        }
    }

    if ui.button(egui::RichText::new("+ Key").color(colors::ACCENT_GREEN)).clicked() {
        let time = keys.last().map(|k| (k.time + 1.0) / 2.0).unwrap_or(0.5);
        keys.push(GradientKey {
            time: time.min(1.0),
            color: LinearRgba::WHITE,
        });
    }
}

fn draw_curve_editor(ui: &mut egui::Ui, curve: &mut Curve<f32>, _idx: usize, _salt: &str) {
    // Simple key list editor
    let mut remove_key = None;
    for (i, key) in curve.keys.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("t:").color(colors::TEXT_MUTED));
            ui.add(egui::DragValue::new(&mut key.time).range(0.0..=1.0).speed(0.01).max_decimals(2));
            ui.label(egui::RichText::new("v:").color(colors::TEXT_MUTED));
            ui.add(egui::DragValue::new(&mut key.value).speed(0.01).max_decimals(3));
            if ui.add(egui::Button::new(egui::RichText::new("\u{00d7}").color(colors::STATUS_ERROR)).frame(false)).clicked() {
                remove_key = Some(i);
            }
        });
    }

    if let Some(idx) = remove_key {
        if curve.keys.len() > 1 {
            curve.keys.remove(idx);
        }
    }

    if ui.button(egui::RichText::new("+ Key").color(colors::ACCENT_GREEN)).clicked() {
        let time = curve.keys.last().map(|k| (k.time + 1.0) / 2.0).unwrap_or(0.5);
        curve.keys.push(CurveKey {
            time: time.min(1.0),
            value: 0.5,
            interp: Interp::Linear,
        });
    }
}
