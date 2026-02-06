//! Particle editor panel for editing bevy_hanabi effects on selected entities.
//!
//! Niagara-inspired card-based layout with color-coded modifier categories:
//! - **Spawner**: orange accent
//! - **Spawn (init)**: green accent
//! - **Update**: blue accent
//! - **Render**: purple accent

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::particles::data::*;
use crate::selection::Selected;
use crate::ui::theme::{colors, draw_pin_button, grid_label, panel, panel_frame};

pub struct ParticleEditorPlugin;

impl Plugin for ParticleEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_particle_panel);
    }
}

// ---------------------------------------------------------------------------
// Card / section drawing helpers
// ---------------------------------------------------------------------------

/// Corner radius used on module cards.
const CARD_ROUNDING: u8 = 4;
/// Width of the colored left-border accent stripe.
const ACCENT_STRIPE_WIDTH: f32 = 3.0;

/// Draw a color-coded module card.
///
/// Returns `true` when the [×] remove button in the header is clicked.
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
        // Header row: label (bold) left, [×] right
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).strong().color(colors::TEXT_PRIMARY));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{00d7}") // ×
                            .color(colors::STATUS_ERROR),
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

    // Paint the accent stripe over the left edge of the card.
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

/// Draw a category section header (e.g. "SPAWN", "UPDATE", "RENDER") with a
/// colored label on the left and a [+] dropdown on the right.
fn category_header<T>(
    ui: &mut egui::Ui,
    label: &str,
    accent: egui::Color32,
    options: &[(&str, fn() -> T)],
    list: &mut Vec<T>,
) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .strong()
                .size(12.0)
                .color(accent),
        );
        // Separator line filling the middle
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

/// Draw a horizontal gradient preview bar for Color/Size Over Lifetime.
fn gradient_preview_bar(ui: &mut egui::Ui, keys: &[GradientKeyData], is_color: bool) {
    let desired = egui::vec2(ui.available_width(), 16.0);
    let (rect, _resp) = ui.allocate_exact_size(desired, egui::Sense::hover());

    if keys.is_empty() {
        return;
    }

    let painter = ui.painter();
    let n_segments = 64;

    // Sort keys by ratio for correct interpolation.
    let mut sorted: Vec<_> = keys.iter().collect();
    sorted.sort_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap_or(std::cmp::Ordering::Equal));

    for seg in 0..n_segments {
        let t0 = seg as f32 / n_segments as f32;
        let t1 = (seg + 1) as f32 / n_segments as f32;
        let t_mid = (t0 + t1) * 0.5;

        let value = sample_gradient(&sorted, t_mid);

        let color = if is_color {
            egui::Color32::from_rgba_unmultiplied(
                (value.x * 255.0).clamp(0.0, 255.0) as u8,
                (value.y * 255.0).clamp(0.0, 255.0) as u8,
                (value.z * 255.0).clamp(0.0, 255.0) as u8,
                (value.w * 255.0).clamp(0.0, 255.0) as u8,
            )
        } else {
            // Size: grayscale from magnitude of XYZ
            let brightness = (value.x + value.y + value.z) / 3.0;
            let v = (brightness.clamp(0.0, 1.0) * 255.0) as u8;
            egui::Color32::from_rgb(v, v, v)
        };

        let x0 = rect.left() + t0 * rect.width();
        let x1 = rect.left() + t1 * rect.width();
        let seg_rect = egui::Rect::from_min_max(
            egui::pos2(x0, rect.top()),
            egui::pos2(x1, rect.bottom()),
        );
        painter.rect_filled(seg_rect, 0.0, color);
    }

    // Thin border around the bar.
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(2),
        egui::Stroke::new(1.0, colors::WIDGET_BORDER),
        egui::StrokeKind::Inside,
    );
}

/// Linearly sample a sorted gradient at position `t` (0..1).
fn sample_gradient(sorted: &[&GradientKeyData], t: f32) -> Vec4 {
    if sorted.is_empty() {
        return Vec4::ZERO;
    }
    if sorted.len() == 1 || t <= sorted[0].ratio {
        return sorted[0].value;
    }
    if t >= sorted.last().unwrap().ratio {
        return sorted.last().unwrap().value;
    }
    for window in sorted.windows(2) {
        let (a, b) = (window[0], window[1]);
        if t >= a.ratio && t <= b.ratio {
            let frac = if (b.ratio - a.ratio).abs() < 1e-6 {
                0.0
            } else {
                (t - a.ratio) / (b.ratio - a.ratio)
            };
            return a.value.lerp(b.value, frac);
        }
    }
    sorted.last().unwrap().value
}

/// Draw a spawner card (orange accent, no remove button).
fn draw_spawner_card(ui: &mut egui::Ui, spawner: &mut SpawnerConfig) {
    let accent = colors::ACCENT_ORANGE;

    let frame = egui::Frame::new()
        .fill(colors::BG_MEDIUM)
        .corner_radius(egui::CornerRadius::same(CARD_ROUNDING))
        .inner_margin(egui::Margin::same(6));

    let resp = frame.show(ui, |ui| {
        // Header
        ui.label(
            egui::RichText::new("Spawner")
                .strong()
                .color(colors::TEXT_PRIMARY),
        );
        ui.add_space(2.0);

        // Mode selector
        let mode_idx = match spawner {
            SpawnerConfig::Rate { .. } => 0,
            SpawnerConfig::Once { .. } => 1,
            SpawnerConfig::Burst { .. } => 2,
        };

        let mut new_mode = mode_idx;

        egui::Grid::new("spawner_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Mode");
                egui::ComboBox::from_id_salt("spawner_mode")
                    .selected_text(match mode_idx {
                        0 => "Rate",
                        1 => "Once",
                        _ => "Burst",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut new_mode, 0, "Rate");
                        ui.selectable_value(&mut new_mode, 1, "Once");
                        ui.selectable_value(&mut new_mode, 2, "Burst");
                    });
                ui.end_row();
            });

        if new_mode != mode_idx {
            *spawner = match new_mode {
                0 => SpawnerConfig::Rate { rate: 50.0 },
                1 => SpawnerConfig::Once { count: 100.0 },
                _ => SpawnerConfig::Burst {
                    count: 100.0,
                    period: 2.0,
                },
            };
        }

        egui::Grid::new("spawner_values_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| match spawner {
                SpawnerConfig::Rate { rate } => {
                    grid_label(ui, "Rate");
                    ui.add(
                        egui::DragValue::new(rate)
                            .speed(1.0)
                            .range(0.1..=100000.0)
                            .max_decimals(1),
                    );
                    ui.end_row();
                }
                SpawnerConfig::Once { count } => {
                    grid_label(ui, "Count");
                    ui.add(
                        egui::DragValue::new(count)
                            .speed(1.0)
                            .range(1.0..=100000.0)
                            .max_decimals(0),
                    );
                    ui.end_row();
                }
                SpawnerConfig::Burst { count, period } => {
                    grid_label(ui, "Count");
                    ui.add(
                        egui::DragValue::new(count)
                            .speed(1.0)
                            .range(1.0..=100000.0)
                            .max_decimals(0),
                    );
                    ui.end_row();

                    grid_label(ui, "Period");
                    ui.add(
                        egui::DragValue::new(period)
                            .speed(0.1)
                            .range(0.01..=60.0)
                            .max_decimals(2)
                            .suffix(" s"),
                    );
                    ui.end_row();
                }
            });
    });

    // Accent stripe
    let card_rect = resp.response.rect;
    let stripe = egui::Rect::from_min_max(
        card_rect.left_top(),
        egui::pos2(card_rect.left() + ACCENT_STRIPE_WIDTH, card_rect.bottom()),
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
}

// ---------------------------------------------------------------------------
// Main panel entry point
// ---------------------------------------------------------------------------

/// Draw the particle editor panel (exclusive world access).
fn draw_particle_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    let is_pinned = world.resource::<PinnedWindows>().0.contains(&EditorMode::Particle);
    if current_mode != EditorMode::Particle && !is_pinned {
        return;
    }

    // Get the single selected entity with a ParticleEffectMarker
    let entity = {
        let mut q = world.query_filtered::<Entity, (With<Selected>, With<ParticleEffectMarker>)>();
        match q.iter(world).next() {
            Some(e) => e,
            None => {
                draw_empty_panel(world, is_pinned, current_mode);
                return;
            }
        }
    };

    // Clone the marker data for editing
    let mut marker = world
        .get::<ParticleEffectMarker>(entity)
        .unwrap()
        .clone();
    let original = marker.clone();

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

    let available_height =
        ctx.content_rect().height() - panel::STATUS_BAR_HEIGHT - panel::WINDOW_PADDING * 2.0;

    // If pinned and the active mode also uses the right side, move to the left
    let displaced = is_pinned
        && current_mode != EditorMode::Particle
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    let mut pin_toggled = false;

    egui::Window::new("Particle Effect")
        .default_size([panel::DEFAULT_WIDTH, available_height])
        .min_width(panel::MIN_WIDTH)
        .min_height(panel::MIN_HEIGHT)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            ui.set_min_height(available_height - panel::TITLE_BAR_HEIGHT - panel::BOTTOM_PADDING);

            // Pin button (right-aligned)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                pin_toggled = draw_pin_button(ui, is_pinned);
            });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(280.0);

                    // -- System settings (compact grid) --
                    draw_system_settings(ui, &mut marker);
                    ui.add_space(4.0);

                    // -- Spawner card (orange) --
                    draw_spawner_card(ui, &mut marker.spawner);

                    // -- Spawn / Init section (green) --
                    draw_init_section(ui, &mut marker.init_modifiers);

                    // -- Update section (blue) --
                    draw_update_section(ui, &mut marker.update_modifiers);

                    // -- Render section (purple) --
                    draw_render_section(ui, &mut marker.render_modifiers);

                    ui.add_space(8.0);
                });
        });

    // Write back if changed
    let changed = ron::to_string(&marker).ok() != ron::to_string(&original).ok();
    if changed {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert(marker);
        }
    }

    // Toggle pin state if button was clicked
    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::Particle) {
            pinned.0.insert(EditorMode::Particle);
        }
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

    let available_height =
        ctx.content_rect().height() - panel::STATUS_BAR_HEIGHT - panel::WINDOW_PADDING * 2.0;

    let displaced = is_pinned
        && current_mode != EditorMode::Particle
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    let mut pin_toggled = false;

    egui::Window::new("Particle Effect")
        .default_size([panel::DEFAULT_WIDTH, available_height])
        .min_width(panel::MIN_WIDTH)
        .min_height(panel::MIN_HEIGHT)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            ui.set_min_height(available_height - panel::TITLE_BAR_HEIGHT - panel::BOTTOM_PADDING);

            // Pin button (right-aligned)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                pin_toggled = draw_pin_button(ui, is_pinned);
            });

            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Select a particle effect entity")
                        .color(colors::TEXT_MUTED)
                        .italics(),
                );
            });
        });

    // Toggle pin state if button was clicked
    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::Particle) {
            pinned.0.insert(EditorMode::Particle);
        }
    }
}

// ---------------------------------------------------------------------------
// System settings (compact 2-column grid)
// ---------------------------------------------------------------------------

fn draw_system_settings(ui: &mut egui::Ui, marker: &mut ParticleEffectMarker) {
    egui::Grid::new("system_settings_grid")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            // Capacity
            grid_label(ui, "Capacity");
            let mut cap = marker.capacity as i32;
            if ui
                .add(egui::DragValue::new(&mut cap).range(1..=1_000_000).speed(100))
                .changed()
            {
                marker.capacity = cap.max(1) as u32;
            }
            ui.end_row();

            // Sim Space
            grid_label(ui, "Sim Space");
            egui::ComboBox::from_id_salt("sim_space")
                .selected_text(match marker.simulation_space {
                    ParticleSimSpace::Global => "Global",
                    ParticleSimSpace::Local => "Local",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut marker.simulation_space,
                        ParticleSimSpace::Global,
                        "Global",
                    );
                    ui.selectable_value(
                        &mut marker.simulation_space,
                        ParticleSimSpace::Local,
                        "Local",
                    );
                });
            ui.end_row();

            // Condition
            grid_label(ui, "Condition");
            egui::ComboBox::from_id_salt("sim_condition")
                .selected_text(match marker.simulation_condition {
                    ParticleSimCondition::WhenVisible => "When Visible",
                    ParticleSimCondition::Always => "Always",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut marker.simulation_condition,
                        ParticleSimCondition::WhenVisible,
                        "When Visible",
                    );
                    ui.selectable_value(
                        &mut marker.simulation_condition,
                        ParticleSimCondition::Always,
                        "Always",
                    );
                });
            ui.end_row();

            // Integration
            grid_label(ui, "Integration");
            egui::ComboBox::from_id_salt("motion_int")
                .selected_text(match marker.motion_integration {
                    ParticleMotionIntegration::None => "None",
                    ParticleMotionIntegration::PreUpdate => "Pre-Update",
                    ParticleMotionIntegration::PostUpdate => "Post-Update",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut marker.motion_integration,
                        ParticleMotionIntegration::None,
                        "None",
                    );
                    ui.selectable_value(
                        &mut marker.motion_integration,
                        ParticleMotionIntegration::PreUpdate,
                        "Pre-Update",
                    );
                    ui.selectable_value(
                        &mut marker.motion_integration,
                        ParticleMotionIntegration::PostUpdate,
                        "Post-Update",
                    );
                });
            ui.end_row();

            // Alpha mode
            grid_label(ui, "Alpha");
            egui::ComboBox::from_id_salt("alpha_mode")
                .selected_text(marker.alpha_mode.label())
                .show_ui(ui, |ui| {
                    for mode in &ParticleAlphaMode::ALL {
                        ui.selectable_value(&mut marker.alpha_mode, *mode, mode.label());
                    }
                });
            ui.end_row();
        });
}

// ---------------------------------------------------------------------------
// Init (Spawn) section — green accent
// ---------------------------------------------------------------------------

fn draw_init_section(ui: &mut egui::Ui, modifiers: &mut Vec<InitModifierData>) {
    category_header(
        ui,
        "SPAWN",
        colors::ACCENT_GREEN,
        InitModifierData::ADD_OPTIONS,
        modifiers,
    );

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("init_card_{i}"));
        if modifier_card(ui, m.label(), colors::ACCENT_GREEN, id, |ui| {
            draw_init_modifier_body(ui, m, i);
        }) {
            remove_idx = Some(i);
        }
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_init_modifier_body(ui: &mut egui::Ui, m: &mut InitModifierData, idx: usize) {
    match m {
        InitModifierData::SetLifetime(range) => {
            draw_scalar_range(ui, "Lifetime", range, 0.1, 0.01..=120.0, idx);
        }
        InitModifierData::SetColor(color) => {
            draw_vec4_color_row(ui, "Color", color);
        }
        InitModifierData::SetSize(range) => {
            draw_scalar_range(ui, "Size", range, 0.01, 0.001..=100.0, idx);
        }
        InitModifierData::SetPositionSphere {
            center,
            radius,
            volume,
        } => {
            draw_vec3_grid(ui, "Center", center, 0.1, idx, "pos_sphere");
            draw_scalar_range(ui, "Radius", radius, 0.1, 0.0..=1000.0, idx);
            ui.checkbox(volume, "Volume");
        }
        InitModifierData::SetPositionCircle {
            center,
            axis,
            radius,
            volume,
        } => {
            draw_vec3_grid(ui, "Center", center, 0.1, idx, "pos_circle_c");
            draw_vec3_grid(ui, "Axis", axis, 0.01, idx, "pos_circle_a");
            draw_scalar_range(ui, "Radius", radius, 0.1, 0.0..=1000.0, idx);
            ui.checkbox(volume, "Volume");
        }
        InitModifierData::SetVelocitySphere { center, speed } => {
            draw_vec3_grid(ui, "Center", center, 0.1, idx, "vel_sphere");
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
        InitModifierData::SetVelocityTangent {
            origin,
            axis,
            speed,
        } => {
            draw_vec3_grid(ui, "Origin", origin, 0.1, idx, "vel_tan_o");
            draw_vec3_grid(ui, "Axis", axis, 0.01, idx, "vel_tan_a");
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
    }
}

// ---------------------------------------------------------------------------
// Update section — blue accent
// ---------------------------------------------------------------------------

fn draw_update_section(ui: &mut egui::Ui, modifiers: &mut Vec<UpdateModifierData>) {
    category_header(
        ui,
        "UPDATE",
        colors::ACCENT_BLUE,
        UpdateModifierData::ADD_OPTIONS,
        modifiers,
    );

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("update_card_{i}"));
        if modifier_card(ui, m.label(), colors::ACCENT_BLUE, id, |ui| {
            draw_update_modifier_body(ui, m, i);
        }) {
            remove_idx = Some(i);
        }
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_update_modifier_body(ui: &mut egui::Ui, m: &mut UpdateModifierData, idx: usize) {
    match m {
        UpdateModifierData::Accel(d) => {
            draw_vec3_grid(ui, "Accel", &mut d.accel, 0.1, idx, "accel");
        }
        UpdateModifierData::RadialAccel(d) => {
            draw_vec3_grid(ui, "Origin", &mut d.origin, 0.1, idx, "radial_o");
            egui::Grid::new(format!("radial_val_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Accel");
                    ui.add(
                        egui::DragValue::new(&mut d.accel)
                            .speed(0.1)
                            .range(-1000.0..=1000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();
                });
        }
        UpdateModifierData::LinearDrag(d) => {
            egui::Grid::new(format!("drag_grid_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Drag");
                    ui.add(
                        egui::DragValue::new(&mut d.drag)
                            .speed(0.1)
                            .range(0.0..=100.0)
                            .max_decimals(3),
                    );
                    ui.end_row();
                });
        }
        UpdateModifierData::KillAabb(d) => {
            draw_vec3_grid(ui, "Center", &mut d.center, 0.1, idx, "kaabb_c");
            draw_vec3_grid(ui, "Half Size", &mut d.half_size, 0.1, idx, "kaabb_hs");
            ui.checkbox(&mut d.kill_inside, "Kill Inside");
        }
        UpdateModifierData::KillSphere(d) => {
            draw_vec3_grid(ui, "Center", &mut d.center, 0.1, idx, "ksphere_c");
            egui::Grid::new(format!("ksphere_r_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Radius");
                    ui.add(
                        egui::DragValue::new(&mut d.radius)
                            .speed(0.1)
                            .range(0.0..=10000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();
                });
            ui.checkbox(&mut d.kill_inside, "Kill Inside");
        }
        UpdateModifierData::TangentAccel(d) => {
            draw_vec3_grid(ui, "Origin", &mut d.origin, 0.1, idx, "tan_o");
            draw_vec3_grid(ui, "Axis", &mut d.axis, 0.01, idx, "tan_a");
            egui::Grid::new(format!("tan_accel_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Accel");
                    ui.add(
                        egui::DragValue::new(&mut d.accel)
                            .speed(0.1)
                            .range(-1000.0..=1000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();
                });
        }
        UpdateModifierData::ConformToSphere(d) => {
            draw_vec3_grid(ui, "Origin", &mut d.origin, 0.1, idx, "conform_o");
            egui::Grid::new(format!("conform_vals_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Radius");
                    ui.add(
                        egui::DragValue::new(&mut d.radius)
                            .speed(0.1)
                            .range(0.0..=10000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();

                    grid_label(ui, "Influence");
                    ui.add(
                        egui::DragValue::new(&mut d.influence_dist)
                            .speed(0.1)
                            .range(0.0..=10000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();

                    grid_label(ui, "Accel");
                    ui.add(
                        egui::DragValue::new(&mut d.attraction_accel)
                            .speed(0.1)
                            .range(0.0..=10000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();

                    grid_label(ui, "Max Speed");
                    ui.add(
                        egui::DragValue::new(&mut d.max_speed)
                            .speed(0.1)
                            .range(0.0..=10000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();
                });
        }
        UpdateModifierData::SetPositionCone3d(d) => {
            egui::Grid::new(format!("cone_vals_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Height");
                    ui.add(
                        egui::DragValue::new(&mut d.height)
                            .speed(0.1)
                            .range(0.0..=1000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();

                    grid_label(ui, "Base Radius");
                    ui.add(
                        egui::DragValue::new(&mut d.base_radius)
                            .speed(0.1)
                            .range(0.0..=1000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();

                    grid_label(ui, "Top Radius");
                    ui.add(
                        egui::DragValue::new(&mut d.top_radius)
                            .speed(0.1)
                            .range(0.0..=1000.0)
                            .max_decimals(3),
                    );
                    ui.end_row();
                });
            ui.checkbox(&mut d.volume, "Volume");
        }
    }
}

// ---------------------------------------------------------------------------
// Render section — purple accent
// ---------------------------------------------------------------------------

fn draw_render_section(ui: &mut egui::Ui, modifiers: &mut Vec<RenderModifierData>) {
    category_header(
        ui,
        "RENDER",
        colors::ACCENT_PURPLE,
        RenderModifierData::ADD_OPTIONS,
        modifiers,
    );

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("render_card_{i}"));
        if modifier_card(ui, m.label(), colors::ACCENT_PURPLE, id, |ui| {
            draw_render_modifier_body(ui, m, i);
        }) {
            remove_idx = Some(i);
        }
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_render_modifier_body(ui: &mut egui::Ui, m: &mut RenderModifierData, idx: usize) {
    match m {
        RenderModifierData::ColorOverLifetime { keys } => {
            gradient_preview_bar(ui, keys, true);
            ui.add_space(4.0);
            draw_gradient_keys(ui, keys, idx, true);
        }
        RenderModifierData::SizeOverLifetime { keys } => {
            gradient_preview_bar(ui, keys, false);
            ui.add_space(4.0);
            draw_gradient_keys(ui, keys, idx, false);
        }
        RenderModifierData::SetColor { color } => {
            draw_vec4_color_row(ui, "Color", color);
        }
        RenderModifierData::SetSize { size } => {
            draw_vec3_grid(ui, "Size", size, 0.01, idx, "rend_size");
        }
        RenderModifierData::Orient { mode } => {
            egui::Grid::new(format!("orient_grid_{idx}"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Mode");
                    egui::ComboBox::from_id_salt(format!("orient_{idx}"))
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for m_opt in &ParticleOrientMode::ALL {
                                ui.selectable_value(mode, *m_opt, m_opt.label());
                            }
                        });
                    ui.end_row();
                });
        }
        RenderModifierData::ScreenSpaceSize => {
            ui.label(
                egui::RichText::new("(no parameters)")
                    .color(colors::TEXT_MUTED)
                    .italics(),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Gradient key editor (used inside render cards)
// ---------------------------------------------------------------------------

fn draw_gradient_keys(
    ui: &mut egui::Ui,
    keys: &mut Vec<GradientKeyData>,
    _mod_idx: usize,
    is_color: bool,
) {
    let mut remove_key = None;
    for (i, key) in keys.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("t:").color(colors::TEXT_MUTED));
            ui.add(
                egui::DragValue::new(&mut key.ratio)
                    .range(0.0..=1.0)
                    .speed(0.01)
                    .max_decimals(2),
            );
            if is_color {
                let mut rgba = [key.value.x, key.value.y, key.value.z, key.value.w];
                if ui
                    .color_edit_button_rgba_unmultiplied(&mut rgba)
                    .changed()
                {
                    key.value = Vec4::new(rgba[0], rgba[1], rgba[2], rgba[3]);
                }
            } else {
                ui.add(
                    egui::DragValue::new(&mut key.value.x)
                        .speed(0.01)
                        .prefix("x:")
                        .max_decimals(3),
                );
                ui.add(
                    egui::DragValue::new(&mut key.value.y)
                        .speed(0.01)
                        .prefix("y:")
                        .max_decimals(3),
                );
                ui.add(
                    egui::DragValue::new(&mut key.value.z)
                        .speed(0.01)
                        .prefix("z:")
                        .max_decimals(3),
                );
            }
            // Per-key remove button
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("\u{00d7}").color(colors::STATUS_ERROR),
                    )
                    .frame(false),
                )
                .on_hover_text("Remove key")
                .clicked()
            {
                remove_key = Some(i);
            }
        });
    }

    if let Some(idx) = remove_key {
        if keys.len() > 1 {
            keys.remove(idx);
        }
    }

    if ui
        .button(egui::RichText::new("+ Key").color(colors::ACCENT_GREEN))
        .clicked()
    {
        let ratio = keys
            .last()
            .map(|k| (k.ratio + 1.0) / 2.0)
            .unwrap_or(0.5);
        keys.push(GradientKeyData {
            ratio: ratio.min(1.0),
            value: if is_color {
                Vec4::ONE
            } else {
                Vec4::new(0.05, 0.05, 0.05, 0.0)
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Shared property drawing helpers
// ---------------------------------------------------------------------------

/// Draw a Vec3 value in a 2-column grid with axis-colored X/Y/Z prefixes.
fn draw_vec3_grid(
    ui: &mut egui::Ui,
    label: &str,
    val: &mut Vec3,
    speed: f64,
    idx: usize,
    salt: &str,
) {
    egui::Grid::new(format!("v3_{salt}_{idx}"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            grid_label(ui, label);
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut val.x)
                        .speed(speed)
                        .prefix("x:")
                        .max_decimals(3),
                );
                ui.add(
                    egui::DragValue::new(&mut val.y)
                        .speed(speed)
                        .prefix("y:")
                        .max_decimals(3),
                );
                ui.add(
                    egui::DragValue::new(&mut val.z)
                        .speed(speed)
                        .prefix("z:")
                        .max_decimals(3),
                );
            });
            ui.end_row();
        });
}

/// Draw a ScalarRange (Constant or Random) with a toggle checkbox.
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
                ui.add(
                    egui::DragValue::new(val)
                        .speed(speed)
                        .range(clamp)
                        .max_decimals(3),
                );
            });
        }
        ScalarRange::Random(min, max) => {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(egui::RichText::new("min").color(colors::TEXT_MUTED));
                ui.add(
                    egui::DragValue::new(min)
                        .speed(speed)
                        .range(clamp.clone())
                        .max_decimals(3),
                );
                ui.label(egui::RichText::new("max").color(colors::TEXT_MUTED));
                ui.add(
                    egui::DragValue::new(max)
                        .speed(speed)
                        .range(clamp)
                        .max_decimals(3),
                );
            });
        }
    }
}

/// Draw a Vec4 as an RGBA color picker in a row.
fn draw_vec4_color_row(ui: &mut egui::Ui, label: &str, val: &mut Vec4) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
        let mut rgba = [val.x, val.y, val.z, val.w];
        if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
            *val = Vec4::new(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    });
}
