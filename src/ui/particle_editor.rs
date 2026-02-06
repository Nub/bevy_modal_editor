//! Particle editor panel for editing bevy_hanabi effects on selected entities.

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState};
use crate::particles::data::*;
use crate::selection::Selected;
use crate::ui::theme::{colors, panel, panel_frame};

/// Accent red color for remove buttons (not in the shared palette).
const ACCENT_RED: egui::Color32 = egui::Color32::from_rgb(220, 80, 80);

pub struct ParticleEditorPlugin;

impl Plugin for ParticleEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_particle_panel);
    }
}

/// Draw the particle editor panel (exclusive world access).
fn draw_particle_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    if current_mode != EditorMode::Particle {
        return;
    }

    // Get the single selected entity with a ParticleEffectMarker
    let entity = {
        let mut q = world.query_filtered::<Entity, (With<Selected>, With<ParticleEffectMarker>)>();
        match q.iter(world).next() {
            Some(e) => e,
            None => {
                // No selected particle entity — show hint
                draw_empty_panel(world);
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

    // Draw the panel
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

    egui::Window::new("Particle Effect")
        .default_size([panel::DEFAULT_WIDTH, available_height])
        .min_width(panel::MIN_WIDTH)
        .min_height(panel::MIN_HEIGHT)
        .max_height(available_height)
        .anchor(
            egui::Align2::RIGHT_TOP,
            [-panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            ui.set_min_height(available_height - panel::TITLE_BAR_HEIGHT - panel::BOTTOM_PADDING);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(280.0);

                    // Top-level settings
                    draw_top_level_settings(ui, &mut marker);
                    ui.add_space(8.0);
                    ui.separator();

                    // Spawner
                    ui.add_space(4.0);
                    draw_spawner_section(ui, &mut marker.spawner);
                    ui.add_space(4.0);
                    ui.separator();

                    // Init modifiers
                    ui.add_space(4.0);
                    draw_init_modifiers(ui, &mut marker.init_modifiers);
                    ui.add_space(4.0);
                    ui.separator();

                    // Update modifiers
                    ui.add_space(4.0);
                    draw_update_modifiers(ui, &mut marker.update_modifiers);
                    ui.add_space(4.0);
                    ui.separator();

                    // Render modifiers
                    ui.add_space(4.0);
                    draw_render_modifiers(ui, &mut marker.render_modifiers);
                });
        });

    // Write back if changed (compare serialized for deep equality)
    let changed = ron::to_string(&marker).ok() != ron::to_string(&original).ok();
    if changed {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert(marker);
        }
    }
}

fn draw_empty_panel(world: &mut World) {
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

    egui::Window::new("Particle Effect")
        .default_size([panel::DEFAULT_WIDTH, available_height])
        .min_width(panel::MIN_WIDTH)
        .min_height(panel::MIN_HEIGHT)
        .max_height(available_height)
        .anchor(
            egui::Align2::RIGHT_TOP,
            [-panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            ui.set_min_height(available_height - panel::TITLE_BAR_HEIGHT - panel::BOTTOM_PADDING);
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Select a particle effect entity")
                        .color(colors::TEXT_MUTED)
                        .italics(),
                );
            });
        });
}

// ---------------------------------------------------------------------------
// Top-level settings
// ---------------------------------------------------------------------------

fn draw_top_level_settings(ui: &mut egui::Ui, marker: &mut ParticleEffectMarker) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Capacity").color(colors::TEXT_SECONDARY));
        let mut cap = marker.capacity as i32;
        if ui
            .add(egui::DragValue::new(&mut cap).range(1..=1_000_000).speed(100))
            .changed()
        {
            marker.capacity = cap.max(1) as u32;
        }
    });

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Sim Space").color(colors::TEXT_SECONDARY));
        egui::ComboBox::from_id_salt("sim_space")
            .selected_text(match marker.simulation_space {
                ParticleSimSpace::Global => "Global",
                ParticleSimSpace::Local => "Local",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut marker.simulation_space, ParticleSimSpace::Global, "Global");
                ui.selectable_value(&mut marker.simulation_space, ParticleSimSpace::Local, "Local");
            });
    });

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Sim Condition").color(colors::TEXT_SECONDARY));
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
    });

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Integration").color(colors::TEXT_SECONDARY));
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
    });

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Alpha Mode").color(colors::TEXT_SECONDARY));
        egui::ComboBox::from_id_salt("alpha_mode")
            .selected_text(marker.alpha_mode.label())
            .show_ui(ui, |ui| {
                for mode in &ParticleAlphaMode::ALL {
                    ui.selectable_value(&mut marker.alpha_mode, *mode, mode.label());
                }
            });
    });
}

// ---------------------------------------------------------------------------
// Spawner section
// ---------------------------------------------------------------------------

fn draw_spawner_section(ui: &mut egui::Ui, spawner: &mut SpawnerConfig) {
    ui.label(
        egui::RichText::new("Spawner")
            .strong()
            .color(colors::TEXT_PRIMARY),
    );
    ui.add_space(4.0);

    // Mode selector
    let mode_idx = match spawner {
        SpawnerConfig::Rate { .. } => 0,
        SpawnerConfig::Once { .. } => 1,
        SpawnerConfig::Burst { .. } => 2,
    };

    let mut new_mode = mode_idx;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Mode").color(colors::TEXT_SECONDARY));
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

    match spawner {
        SpawnerConfig::Rate { rate } => {
            draw_drag_row(ui, "Rate", rate, 1.0, 0.1..=100000.0);
        }
        SpawnerConfig::Once { count } => {
            draw_drag_row(ui, "Count", count, 1.0, 1.0..=100000.0);
        }
        SpawnerConfig::Burst { count, period } => {
            draw_drag_row(ui, "Count", count, 1.0, 1.0..=100000.0);
            draw_drag_row(ui, "Period", period, 0.1, 0.01..=60.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Init modifiers
// ---------------------------------------------------------------------------

fn draw_init_modifiers(ui: &mut egui::Ui, modifiers: &mut Vec<InitModifierData>) {
    draw_modifier_list_header(ui, "Init Modifiers", InitModifierData::ADD_OPTIONS, modifiers);

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("init_{}", i));
        egui::CollapsingHeader::new(
            egui::RichText::new(m.label()).color(colors::TEXT_PRIMARY),
        )
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            draw_init_modifier_fields(ui, m, i);
            if ui
                .button(egui::RichText::new("Remove").color(ACCENT_RED))
                .clicked()
            {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_init_modifier_fields(ui: &mut egui::Ui, m: &mut InitModifierData, idx: usize) {
    match m {
        InitModifierData::SetLifetime(range) => {
            draw_scalar_range(ui, "Lifetime", range, 0.1, 0.01..=120.0, idx);
        }
        InitModifierData::SetColor(color) => {
            draw_vec4_color(ui, "Color", color);
        }
        InitModifierData::SetSize(range) => {
            draw_scalar_range(ui, "Size", range, 0.01, 0.001..=100.0, idx);
        }
        InitModifierData::SetPositionSphere {
            center,
            radius,
            volume,
        } => {
            draw_vec3_row(ui, "Center", center, 0.1);
            draw_scalar_range(ui, "Radius", radius, 0.1, 0.0..=1000.0, idx);
            ui.checkbox(volume, "Volume");
        }
        InitModifierData::SetPositionCircle {
            center,
            axis,
            radius,
            volume,
        } => {
            draw_vec3_row(ui, "Center", center, 0.1);
            draw_vec3_row(ui, "Axis", axis, 0.01);
            draw_scalar_range(ui, "Radius", radius, 0.1, 0.0..=1000.0, idx);
            ui.checkbox(volume, "Volume");
        }
        InitModifierData::SetVelocitySphere { center, speed } => {
            draw_vec3_row(ui, "Center", center, 0.1);
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
        InitModifierData::SetVelocityTangent {
            origin,
            axis,
            speed,
        } => {
            draw_vec3_row(ui, "Origin", origin, 0.1);
            draw_vec3_row(ui, "Axis", axis, 0.01);
            draw_scalar_range(ui, "Speed", speed, 0.1, 0.0..=1000.0, idx);
        }
    }
}

// ---------------------------------------------------------------------------
// Update modifiers
// ---------------------------------------------------------------------------

fn draw_update_modifiers(ui: &mut egui::Ui, modifiers: &mut Vec<UpdateModifierData>) {
    draw_modifier_list_header(ui, "Update Modifiers", UpdateModifierData::ADD_OPTIONS, modifiers);

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("update_{}", i));
        egui::CollapsingHeader::new(
            egui::RichText::new(m.label()).color(colors::TEXT_PRIMARY),
        )
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            draw_update_modifier_fields(ui, m);
            if ui
                .button(egui::RichText::new("Remove").color(ACCENT_RED))
                .clicked()
            {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_update_modifier_fields(ui: &mut egui::Ui, m: &mut UpdateModifierData) {
    match m {
        UpdateModifierData::Accel(d) => {
            draw_vec3_row(ui, "Accel", &mut d.accel, 0.1);
        }
        UpdateModifierData::RadialAccel(d) => {
            draw_vec3_row(ui, "Origin", &mut d.origin, 0.1);
            draw_drag_row(ui, "Accel", &mut d.accel, 0.1, -1000.0..=1000.0);
        }
        UpdateModifierData::LinearDrag(d) => {
            draw_drag_row(ui, "Drag", &mut d.drag, 0.1, 0.0..=100.0);
        }
        UpdateModifierData::KillAabb(d) => {
            draw_vec3_row(ui, "Center", &mut d.center, 0.1);
            draw_vec3_row(ui, "Half Size", &mut d.half_size, 0.1);
            ui.checkbox(&mut d.kill_inside, "Kill Inside");
        }
        UpdateModifierData::KillSphere(d) => {
            draw_vec3_row(ui, "Center", &mut d.center, 0.1);
            draw_drag_row(ui, "Radius", &mut d.radius, 0.1, 0.0..=10000.0);
            ui.checkbox(&mut d.kill_inside, "Kill Inside");
        }
    }
}

// ---------------------------------------------------------------------------
// Render modifiers
// ---------------------------------------------------------------------------

fn draw_render_modifiers(ui: &mut egui::Ui, modifiers: &mut Vec<RenderModifierData>) {
    draw_modifier_list_header(ui, "Render Modifiers", RenderModifierData::ADD_OPTIONS, modifiers);

    let mut remove_idx = None;
    for (i, m) in modifiers.iter_mut().enumerate() {
        let id = ui.make_persistent_id(format!("render_{}", i));
        egui::CollapsingHeader::new(
            egui::RichText::new(m.label()).color(colors::TEXT_PRIMARY),
        )
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            draw_render_modifier_fields(ui, m, i);
            if ui
                .button(egui::RichText::new("Remove").color(ACCENT_RED))
                .clicked()
            {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(idx) = remove_idx {
        modifiers.remove(idx);
    }
}

fn draw_render_modifier_fields(ui: &mut egui::Ui, m: &mut RenderModifierData, idx: usize) {
    match m {
        RenderModifierData::ColorOverLifetime { keys } => {
            draw_gradient_editor(ui, keys, "color_grad", idx, true);
        }
        RenderModifierData::SizeOverLifetime { keys } => {
            draw_gradient_editor(ui, keys, "size_grad", idx, false);
        }
        RenderModifierData::SetColor { color } => {
            draw_vec4_color(ui, "Color", color);
        }
        RenderModifierData::SetSize { size } => {
            draw_vec3_row(ui, "Size", size, 0.01);
        }
        RenderModifierData::Orient { mode } => {
            egui::ComboBox::from_id_salt(format!("orient_{}", idx))
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for m_opt in &ParticleOrientMode::ALL {
                        ui.selectable_value(mode, *m_opt, m_opt.label());
                    }
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
// Gradient editor
// ---------------------------------------------------------------------------

fn draw_gradient_editor(
    ui: &mut egui::Ui,
    keys: &mut Vec<GradientKeyData>,
    id_prefix: &str,
    mod_idx: usize,
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
                if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                    key.value = Vec4::new(rgba[0], rgba[1], rgba[2], rgba[3]);
                }
            } else {
                // Size: just XYZ
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
            if ui
                .small_button(egui::RichText::new("x").color(ACCENT_RED))
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
        let ratio = keys.last().map(|k| (k.ratio + 1.0) / 2.0).unwrap_or(0.5);
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
// Modifier list header with Add dropdown
// ---------------------------------------------------------------------------

fn draw_modifier_list_header<T>(
    ui: &mut egui::Ui,
    title: &str,
    options: &[(&str, fn() -> T)],
    list: &mut Vec<T>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .strong()
                .color(colors::TEXT_PRIMARY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(
                egui::RichText::new("+").color(colors::ACCENT_GREEN),
                |ui| {
                    for (name, factory) in options {
                        if ui.button(*name).clicked() {
                            list.push(factory());
                            ui.close_menu();
                        }
                    }
                },
            );
        });
    });
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Shared drawing helpers
// ---------------------------------------------------------------------------

fn draw_scalar_range(
    ui: &mut egui::Ui,
    label: &str,
    range: &mut ScalarRange,
    speed: f64,
    clamp: std::ops::RangeInclusive<f32>,
    _idx: usize,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));

        let is_random = matches!(range, ScalarRange::Random(_, _));
        let mut toggle = is_random;
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

fn draw_vec3_row(ui: &mut egui::Ui, label: &str, val: &mut Vec3, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
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
}

fn draw_vec4_color(ui: &mut egui::Ui, label: &str, val: &mut Vec4) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
        let mut rgba = [val.x, val.y, val.z, val.w];
        if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
            *val = Vec4::new(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    });
}

fn draw_drag_row(
    ui: &mut egui::Ui,
    label: &str,
    val: &mut f32,
    speed: f64,
    range: std::ops::RangeInclusive<f32>,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
        ui.add(
            egui::DragValue::new(val)
                .speed(speed)
                .range(range)
                .max_decimals(3),
        );
    });
}
