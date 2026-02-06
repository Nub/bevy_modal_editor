//! Camera render settings panel for configuring post-processing effects.
//!
//! Provides UI for tonemapping, exposure, bloom, color grading, anti-aliasing,
//! SSAO, depth of field, and distance fog. Settings can be previewed on the
//! editor camera and are applied to any `GameCamera` when spawned.

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use bevy_editor_game::{
    AntiAliasingMode, BloomComposite, BloomSettingsData, CameraRenderSettings,
    ColorGradingSection, ColorGradingSettings, DofMode, DofSettings, FogFalloffMode,
    FogSettingsData, SsaoQuality, SsaoSettings, TonemappingMode,
};

use crate::editor::{EditorCamera, EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::ui::theme::{colors, draw_pin_button, grid_label, panel, panel_frame, section_header, value_slider, DRAG_VALUE_WIDTH};

/// UI state for the camera settings panel
#[derive(Resource, Default)]
pub struct CameraSettingsState {
    /// Whether render settings are currently previewed on the editor camera
    pub previewing: bool,
}

pub struct CameraSettingsPlugin;

impl Plugin for CameraSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRenderSettings>()
            .init_resource::<CameraSettingsState>()
            .add_systems(EguiPrimaryContextPass, draw_camera_settings_panel);
    }
}

/// Draw the camera settings panel (exclusive world access).
fn draw_camera_settings_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    let is_pinned = world.resource::<PinnedWindows>().0.contains(&EditorMode::Camera);
    if current_mode != EditorMode::Camera && !is_pinned {
        return;
    }

    // Clone settings for mutation during UI drawing
    let mut settings = world.resource::<CameraRenderSettings>().clone();
    let mut ui_state = world.resource::<CameraSettingsState>().previewing;

    let mut changed = false;
    let mut preview_toggled = false;
    let mut revert_requested = false;

    // Get egui context (same pattern as material_editor)
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

    // Calculate panel position (left side, offset from top)
    let available_height = ctx.input(|i: &egui::InputState| i.viewport_rect().height());
    let panel_height = available_height - panel::STATUS_BAR_HEIGHT - panel::WINDOW_PADDING * 2.0;

    // If pinned and the active mode also uses the left side, move to the right
    let displaced = is_pinned
        && current_mode != EditorMode::Camera
        && current_mode.panel_side() == Some(PanelSide::Left);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    let mut pin_toggled = false;

    egui::Window::new("Camera Settings")
        .id(egui::Id::new("camera_settings_panel"))
        .frame(panel_frame(&ctx.style()))
        .anchor(anchor_align, anchor_offset)
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .max_height(panel_height)
        .resizable(true)
        .collapsible(false)
        .title_bar(false)
        .show(&ctx, |ui| {
            // Title with pin button
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("CAMERA")
                        .strong()
                        .color(colors::ACCENT_CYAN),
                );
                ui.label(
                    egui::RichText::new("Render Settings")
                        .color(colors::TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    pin_toggled = draw_pin_button(ui, is_pinned);
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // ── Tonemapping ──
                    section_header(ui, "Tonemapping", true, |ui| {
                        egui::Grid::new("tonemapping_grid")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                grid_label(ui, "Algorithm");
                                let current_label = settings.tonemapping.label();
                                egui::ComboBox::from_id_salt("tonemap_combo")
                                    .selected_text(current_label)
                                    .show_ui(ui, |ui| {
                                        for mode in TonemappingMode::ALL {
                                            if ui
                                                .selectable_value(
                                                    &mut settings.tonemapping,
                                                    mode,
                                                    mode.label(),
                                                )
                                                .changed()
                                            {
                                                changed = true;
                                            }
                                        }
                                    });
                                ui.end_row();
                            });
                    });

                    // ── Exposure ──
                    section_header(ui, "Exposure", true, |ui| {
                        egui::Grid::new("exposure_grid")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                grid_label(ui, "EV100");
                                changed |= value_slider(ui, &mut settings.exposure, 0.0..=20.0);
                                ui.end_row();
                            });

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Presets:")
                                    .color(colors::TEXT_MUTED)
                                    .small(),
                            );
                            for (label, value) in [
                                ("Sunlight", 15.0_f32),
                                ("Overcast", 12.0),
                                ("Indoor", 7.0),
                                ("Blender", 9.7),
                            ] {
                                if ui.small_button(label).clicked() {
                                    settings.exposure = value;
                                    changed = true;
                                }
                            }
                        });
                    });

                    // ── Bloom ──
                    draw_bloom_section(ui, &mut settings, &mut changed);

                    // ── Color Grading ──
                    draw_color_grading_section(ui, &mut settings.color_grading, &mut changed);

                    // ── Anti-Aliasing ──
                    section_header(ui, "Anti-Aliasing", true, |ui| {
                        egui::Grid::new("aa_grid")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                grid_label(ui, "Mode");
                                let current_label = settings.anti_aliasing.label();
                                egui::ComboBox::from_id_salt("aa_combo")
                                    .selected_text(current_label)
                                    .show_ui(ui, |ui| {
                                        for mode in AntiAliasingMode::ALL {
                                            if ui
                                                .selectable_value(
                                                    &mut settings.anti_aliasing,
                                                    mode,
                                                    mode.label(),
                                                )
                                                .changed()
                                            {
                                                changed = true;
                                            }
                                        }
                                    });
                                ui.end_row();
                            });
                    });

                    // ── SSAO ──
                    draw_ssao_section(ui, &mut settings, &mut changed);

                    // ── Depth of Field ──
                    draw_dof_section(ui, &mut settings, &mut changed);

                    // ── Distance Fog ──
                    draw_fog_section(ui, &mut settings, &mut changed);

                    ui.add_space(12.0);
                    ui.separator();

                    // ── Bottom buttons ──
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let preview_label = if ui_state { "Stop Preview" } else { "Preview (P)" };
                        if ui.button(preview_label).clicked() {
                            ui_state = !ui_state;
                            preview_toggled = true;
                        }
                        if ui.button("Revert (R)").clicked() {
                            revert_requested = true;
                        }
                    });
                    ui.add_space(4.0);
                });
        });

    // Handle keyboard shortcuts
    if ctx.input(|i| i.key_pressed(egui::Key::P)) && !ctx.wants_keyboard_input() {
        ui_state = !ui_state;
        preview_toggled = true;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::R)) && !ctx.wants_keyboard_input() {
        revert_requested = true;
    }

    // Apply changes
    if revert_requested {
        settings = CameraRenderSettings::default();
        changed = true;
        ui_state = false;
        preview_toggled = true;
    }

    if changed {
        *world.resource_mut::<CameraRenderSettings>() = settings.clone();
    }

    if preview_toggled {
        world.resource_mut::<CameraSettingsState>().previewing = ui_state;
    }

    // Apply or revert preview on editor camera
    if changed || preview_toggled {
        let settings = world.resource::<CameraRenderSettings>().clone();
        let previewing = world.resource::<CameraSettingsState>().previewing;

        let editor_camera = {
            let mut q = world.query_filtered::<Entity, With<EditorCamera>>();
            q.iter(world).next()
        };

        if let Some(entity) = editor_camera {
            if previewing {
                apply_render_settings_to_entity(world, entity, &settings);
            } else {
                revert_render_settings_on_entity(world, entity);
            }
        }
    }

    // Toggle pin state if button was clicked
    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::Camera) {
            pinned.0.insert(EditorMode::Camera);
        }
    }
}

// ---------------------------------------------------------------------------
// Section draw helpers
// ---------------------------------------------------------------------------

fn draw_bloom_section(
    ui: &mut egui::Ui,
    settings: &mut CameraRenderSettings,
    changed: &mut bool,
) {
    let mut enabled = settings.bloom.is_some();
    let header_text = format!("Bloom ({})", if enabled { "ON" } else { "OFF" });

    section_header(ui, &header_text, false, |ui| {
        if ui
            .checkbox(&mut enabled, "Enabled")
            .changed()
        {
            if enabled && settings.bloom.is_none() {
                settings.bloom = Some(BloomSettingsData::default());
            } else if !enabled {
                settings.bloom = None;
            }
            *changed = true;
        }

        if let Some(bloom) = &mut settings.bloom {
            egui::Grid::new("bloom_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Intensity");
                    *changed |= value_slider(ui, &mut bloom.intensity, 0.0..=1.0);
                    ui.end_row();

                    grid_label(ui, "LF Boost");
                    *changed |= value_slider(ui, &mut bloom.low_frequency_boost, 0.0..=1.0);
                    ui.end_row();

                    grid_label(ui, "LF Curvature");
                    *changed |= value_slider(
                        ui,
                        &mut bloom.low_frequency_boost_curvature,
                        0.0..=1.0,
                    );
                    ui.end_row();

                    grid_label(ui, "HP Frequency");
                    *changed |= value_slider(ui, &mut bloom.high_pass_frequency, 0.0..=1.0);
                    ui.end_row();

                    grid_label(ui, "Composite");
                    let current_label = bloom.composite_mode.label();
                    egui::ComboBox::from_id_salt("bloom_composite")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for mode in BloomComposite::ALL {
                                if ui
                                    .selectable_value(
                                        &mut bloom.composite_mode,
                                        mode,
                                        mode.label(),
                                    )
                                    .changed()
                                {
                                    *changed = true;
                                }
                            }
                        });
                    ui.end_row();
                });
        }
    });
}

fn draw_color_grading_section(
    ui: &mut egui::Ui,
    cg: &mut ColorGradingSettings,
    changed: &mut bool,
) {
    section_header(ui, "Color Grading", false, |ui| {
        egui::Grid::new("cg_global_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Exposure");
                *changed |= value_slider(ui, &mut cg.exposure, -5.0..=5.0);
                ui.end_row();

                grid_label(ui, "Temperature");
                *changed |= value_slider(ui, &mut cg.temperature, -1.0..=1.0);
                ui.end_row();

                grid_label(ui, "Tint");
                *changed |= value_slider(ui, &mut cg.tint, -1.0..=1.0);
                ui.end_row();

                grid_label(ui, "Hue");
                *changed |= value_slider(ui, &mut cg.hue, -1.0..=1.0);
                ui.end_row();

                grid_label(ui, "Saturation");
                *changed |= value_slider(ui, &mut cg.post_saturation, 0.0..=2.0);
                ui.end_row();
            });

        draw_cg_subsection(ui, "Shadows", "cg_shadows", &mut cg.shadows, changed);
        draw_cg_subsection(ui, "Midtones", "cg_midtones", &mut cg.midtones, changed);
        draw_cg_subsection(ui, "Highlights", "cg_highlights", &mut cg.highlights, changed);
    });
}

fn draw_cg_subsection(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    section: &mut ColorGradingSection,
    changed: &mut bool,
) {
    egui::CollapsingHeader::new(
        egui::RichText::new(label)
            .color(colors::TEXT_SECONDARY),
    )
    .default_open(false)
    .show(ui, |ui| {
        egui::Grid::new(id)
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Saturation");
                *changed |= value_slider(ui, &mut section.saturation, 0.0..=2.0);
                ui.end_row();

                grid_label(ui, "Contrast");
                *changed |= value_slider(ui, &mut section.contrast, 0.0..=2.0);
                ui.end_row();

                grid_label(ui, "Gamma");
                *changed |= value_slider(ui, &mut section.gamma, 0.0..=3.0);
                ui.end_row();

                grid_label(ui, "Gain");
                *changed |= value_slider(ui, &mut section.gain, 0.0..=3.0);
                ui.end_row();

                grid_label(ui, "Lift");
                *changed |= value_slider(ui, &mut section.lift, -1.0..=1.0);
                ui.end_row();
            });
    });
}

fn draw_ssao_section(
    ui: &mut egui::Ui,
    settings: &mut CameraRenderSettings,
    changed: &mut bool,
) {
    let mut enabled = settings.ssao.is_some();
    let header_text = format!("SSAO ({})", if enabled { "ON" } else { "OFF" });

    section_header(ui, &header_text, false, |ui| {
        if ui.checkbox(&mut enabled, "Enabled").changed() {
            if enabled && settings.ssao.is_none() {
                settings.ssao = Some(SsaoSettings::default());
            } else if !enabled {
                settings.ssao = None;
            }
            *changed = true;
        }

        if settings.ssao.is_some() {
            ui.label(
                egui::RichText::new("Note: SSAO requires MSAA Off")
                    .color(colors::TEXT_MUTED)
                    .small(),
            );
        }

        if let Some(ssao) = &mut settings.ssao {
            egui::Grid::new("ssao_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Quality");
                    let current_label = ssao.quality.label();
                    egui::ComboBox::from_id_salt("ssao_quality")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for q in SsaoQuality::ALL {
                                if ui
                                    .selectable_value(&mut ssao.quality, q, q.label())
                                    .changed()
                                {
                                    *changed = true;
                                }
                            }
                        });
                    ui.end_row();

                    grid_label(ui, "Thickness");
                    *changed |=
                        value_slider(ui, &mut ssao.constant_object_thickness, 0.01..=5.0);
                    ui.end_row();
                });
        }
    });
}

fn draw_dof_section(
    ui: &mut egui::Ui,
    settings: &mut CameraRenderSettings,
    changed: &mut bool,
) {
    let mut enabled = settings.depth_of_field.is_some();
    let header_text = format!("Depth of Field ({})", if enabled { "ON" } else { "OFF" });

    section_header(ui, &header_text, false, |ui| {
        if ui.checkbox(&mut enabled, "Enabled").changed() {
            if enabled && settings.depth_of_field.is_none() {
                settings.depth_of_field = Some(DofSettings::default());
            } else if !enabled {
                settings.depth_of_field = None;
            }
            *changed = true;
        }

        if let Some(dof) = &mut settings.depth_of_field {
            egui::Grid::new("dof_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Mode");
                    let current_label = dof.mode.label();
                    egui::ComboBox::from_id_salt("dof_mode")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for mode in DofMode::ALL {
                                if ui
                                    .selectable_value(&mut dof.mode, mode, mode.label())
                                    .changed()
                                {
                                    *changed = true;
                                }
                            }
                        });
                    ui.end_row();

                    grid_label(ui, "Focal Dist");
                    *changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH * 2.0, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut dof.focal_distance)
                                .range(0.1..=1000.0)
                                .speed(0.5)
                                .min_decimals(1),
                        )
                        .changed();
                    ui.end_row();

                    grid_label(ui, "Aperture");
                    *changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH * 2.0, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut dof.aperture_f_stops)
                                .range(0.001..=64.0)
                                .speed(0.01)
                                .min_decimals(3)
                                .prefix("f/"),
                        )
                        .changed();
                    ui.end_row();

                    grid_label(ui, "Sensor Height");
                    *changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH * 2.0, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut dof.sensor_height)
                                .range(0.001..=0.1)
                                .speed(0.0001)
                                .min_decimals(4),
                        )
                        .changed();
                    ui.end_row();

                    grid_label(ui, "Max Depth");
                    let mut finite_depth = if dof.max_depth.is_finite() {
                        dof.max_depth
                    } else {
                        1000.0
                    };
                    let mut is_infinite = dof.max_depth.is_infinite();
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut is_infinite, "Infinite").changed() {
                            dof.max_depth = if is_infinite {
                                f32::INFINITY
                            } else {
                                finite_depth
                            };
                            *changed = true;
                        }
                        if !is_infinite {
                            if ui
                                .add_sized(
                                    [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                                    egui::DragValue::new(&mut finite_depth)
                                        .range(0.1..=10000.0)
                                        .speed(1.0),
                                )
                                .changed()
                            {
                                dof.max_depth = finite_depth;
                                *changed = true;
                            }
                        }
                    });
                    ui.end_row();
                });
        }
    });
}

fn draw_fog_section(
    ui: &mut egui::Ui,
    settings: &mut CameraRenderSettings,
    changed: &mut bool,
) {
    let mut enabled = settings.distance_fog.is_some();
    let header_text = format!("Distance Fog ({})", if enabled { "ON" } else { "OFF" });

    section_header(ui, &header_text, false, |ui| {
        if ui.checkbox(&mut enabled, "Enabled").changed() {
            if enabled && settings.distance_fog.is_none() {
                settings.distance_fog = Some(FogSettingsData::default());
            } else if !enabled {
                settings.distance_fog = None;
            }
            *changed = true;
        }

        if let Some(fog) = &mut settings.distance_fog {
            egui::Grid::new("fog_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    // Fog color
                    grid_label(ui, "Color");
                    let c = fog.color.to_srgba();
                    let mut color_arr = [c.red, c.green, c.blue, c.alpha];
                    if ui
                        .color_edit_button_rgba_unmultiplied(&mut color_arr)
                        .changed()
                    {
                        fog.color =
                            Color::srgba(color_arr[0], color_arr[1], color_arr[2], color_arr[3]);
                        *changed = true;
                    }
                    ui.end_row();

                    // Directional light color
                    grid_label(ui, "Dir Light");
                    let dc = fog.directional_light_color.to_srgba();
                    let mut dc_arr = [dc.red, dc.green, dc.blue, dc.alpha];
                    if ui
                        .color_edit_button_rgba_unmultiplied(&mut dc_arr)
                        .changed()
                    {
                        fog.directional_light_color =
                            Color::srgba(dc_arr[0], dc_arr[1], dc_arr[2], dc_arr[3]);
                        *changed = true;
                    }
                    ui.end_row();

                    grid_label(ui, "Dir Exponent");
                    *changed |=
                        value_slider(ui, &mut fog.directional_light_exponent, 1.0..=64.0);
                    ui.end_row();

                    // Falloff mode selector
                    grid_label(ui, "Falloff");
                    let falloff_label = fog.falloff.label();
                    egui::ComboBox::from_id_salt("fog_falloff")
                        .selected_text(falloff_label)
                        .show_ui(ui, |ui| {
                            let is_linear = matches!(fog.falloff, FogFalloffMode::Linear { .. });
                            let is_exp = matches!(fog.falloff, FogFalloffMode::Exponential { .. });
                            let is_exp2 =
                                matches!(fog.falloff, FogFalloffMode::ExponentialSquared { .. });

                            if ui.selectable_label(is_linear, "Linear").clicked() && !is_linear {
                                fog.falloff = FogFalloffMode::Linear {
                                    start: 0.0,
                                    end: 100.0,
                                };
                                *changed = true;
                            }
                            if ui.selectable_label(is_exp, "Exponential").clicked() && !is_exp {
                                fog.falloff =
                                    FogFalloffMode::Exponential { density: 0.02 };
                                *changed = true;
                            }
                            if ui.selectable_label(is_exp2, "Exponential²").clicked() && !is_exp2 {
                                fog.falloff =
                                    FogFalloffMode::ExponentialSquared { density: 0.02 };
                                *changed = true;
                            }
                        });
                    ui.end_row();

                    // Falloff parameters
                    match &mut fog.falloff {
                        FogFalloffMode::Linear { start, end } => {
                            grid_label(ui, "Start");
                            *changed |= value_slider(ui, start, 0.0..=500.0);
                            ui.end_row();

                            grid_label(ui, "End");
                            *changed |= value_slider(ui, end, 0.0..=1000.0);
                            ui.end_row();
                        }
                        FogFalloffMode::Exponential { density }
                        | FogFalloffMode::ExponentialSquared { density } => {
                            grid_label(ui, "Density");
                            *changed |= value_slider(ui, density, 0.001..=1.0);
                            ui.end_row();
                        }
                    }
                });
        }
    });
}

// ---------------------------------------------------------------------------
// Apply / Revert render settings to camera entities
// ---------------------------------------------------------------------------

/// Apply render settings to a camera entity by inserting/removing Bevy components.
pub fn apply_render_settings_to_entity(
    world: &mut World,
    entity: Entity,
    settings: &CameraRenderSettings,
) {
    // Tonemapping
    let tonemapping = match settings.tonemapping {
        TonemappingMode::None => bevy::core_pipeline::tonemapping::Tonemapping::None,
        TonemappingMode::Reinhard => bevy::core_pipeline::tonemapping::Tonemapping::Reinhard,
        TonemappingMode::ReinhardLuminance => {
            bevy::core_pipeline::tonemapping::Tonemapping::ReinhardLuminance
        }
        TonemappingMode::AcesFitted => bevy::core_pipeline::tonemapping::Tonemapping::AcesFitted,
        TonemappingMode::AgX => bevy::core_pipeline::tonemapping::Tonemapping::AgX,
        TonemappingMode::SomewhatBoringDisplayTransform => {
            bevy::core_pipeline::tonemapping::Tonemapping::SomewhatBoringDisplayTransform
        }
        TonemappingMode::TonyMcMapface => {
            bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface
        }
        TonemappingMode::BlenderFilmic => {
            bevy::core_pipeline::tonemapping::Tonemapping::BlenderFilmic
        }
    };

    // We need to use commands since we can't insert components directly via &mut World easily
    // without triggering borrow issues. Build up the operations then apply them.
    let mut cmds = world.commands();
    let mut entity_cmds = cmds.entity(entity);

    entity_cmds.insert(tonemapping);

    // Exposure
    entity_cmds.insert(bevy::camera::Exposure { ev100: settings.exposure });

    // Bloom
    if let Some(bloom) = &settings.bloom {
        let composite = match bloom.composite_mode {
            BloomComposite::EnergyConserving => {
                bevy::post_process::bloom::BloomCompositeMode::EnergyConserving
            }
            BloomComposite::Additive => {
                bevy::post_process::bloom::BloomCompositeMode::Additive
            }
        };
        entity_cmds.insert(bevy::post_process::bloom::Bloom {
            intensity: bloom.intensity,
            low_frequency_boost: bloom.low_frequency_boost,
            low_frequency_boost_curvature: bloom.low_frequency_boost_curvature,
            high_pass_frequency: bloom.high_pass_frequency,
            composite_mode: composite,
            ..default()
        });
    } else {
        entity_cmds.remove::<bevy::post_process::bloom::Bloom>();
    }

    // Distance Fog
    if let Some(fog) = &settings.distance_fog {
        let falloff = match &fog.falloff {
            FogFalloffMode::Linear { start, end } => {
                bevy::pbr::FogFalloff::Linear {
                    start: *start,
                    end: *end,
                }
            }
            FogFalloffMode::Exponential { density } => {
                bevy::pbr::FogFalloff::Exponential { density: *density }
            }
            FogFalloffMode::ExponentialSquared { density } => {
                bevy::pbr::FogFalloff::ExponentialSquared { density: *density }
            }
        };
        entity_cmds.insert(bevy::pbr::DistanceFog {
            color: fog.color,
            directional_light_color: fog.directional_light_color,
            directional_light_exponent: fog.directional_light_exponent,
            falloff,
        });
    } else {
        entity_cmds.remove::<bevy::pbr::DistanceFog>();
    }

    // Anti-aliasing (MSAA is a per-camera component in Bevy 0.18)
    match settings.anti_aliasing {
        AntiAliasingMode::MsaaOff => {
            entity_cmds.insert(Msaa::Off);
        }
        AntiAliasingMode::Msaa2x => {
            entity_cmds.insert(Msaa::Sample2);
        }
        AntiAliasingMode::Msaa4x => {
            entity_cmds.insert(Msaa::default());
        }
        AntiAliasingMode::Msaa8x => {
            entity_cmds.insert(Msaa::Sample8);
        }
        AntiAliasingMode::Fxaa => {
            entity_cmds.insert(Msaa::Off);
            entity_cmds.insert(bevy::anti_alias::fxaa::Fxaa::default());
        }
    }

    // Remove FXAA if not using it
    if settings.anti_aliasing != AntiAliasingMode::Fxaa {
        entity_cmds.remove::<bevy::anti_alias::fxaa::Fxaa>();
    }

    // SSAO (requires Msaa::Off)
    if let Some(ssao) = &settings.ssao {
        // SSAO is incompatible with MSAA — force it off
        entity_cmds.insert(Msaa::Off);
        entity_cmds.remove::<bevy::anti_alias::fxaa::Fxaa>();

        let quality_level = match ssao.quality {
            SsaoQuality::Low => {
                bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Low
            }
            SsaoQuality::Medium => {
                bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Medium
            }
            SsaoQuality::High => {
                bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::High
            }
            SsaoQuality::Ultra => {
                bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Ultra
            }
        };
        entity_cmds.insert(bevy::pbr::ScreenSpaceAmbientOcclusion {
            quality_level,
            constant_object_thickness: ssao.constant_object_thickness,
        });
    } else {
        entity_cmds.remove::<bevy::pbr::ScreenSpaceAmbientOcclusion>();
    }

    // Depth of Field
    if let Some(dof) = &settings.depth_of_field {
        let mode = match dof.mode {
            DofMode::Gaussian => bevy::post_process::dof::DepthOfFieldMode::Gaussian,
            DofMode::Bokeh => bevy::post_process::dof::DepthOfFieldMode::Bokeh,
        };
        entity_cmds.insert(bevy::post_process::dof::DepthOfField {
            mode,
            focal_distance: dof.focal_distance,
            aperture_f_stops: dof.aperture_f_stops,
            sensor_height: dof.sensor_height,
            max_depth: dof.max_depth,
            ..default()
        });
    } else {
        entity_cmds.remove::<bevy::post_process::dof::DepthOfField>();
    }

    // Color Grading
    let cg = &settings.color_grading;
    let convert_section =
        |s: &ColorGradingSection| bevy::render::view::ColorGradingSection {
            saturation: s.saturation,
            contrast: s.contrast,
            gamma: s.gamma,
            gain: s.gain,
            lift: s.lift,
        };
    entity_cmds.insert(bevy::render::view::ColorGrading {
        global: bevy::render::view::ColorGradingGlobal {
            exposure: cg.exposure,
            temperature: cg.temperature,
            tint: cg.tint,
            hue: cg.hue,
            post_saturation: cg.post_saturation,
            ..default()
        },
        shadows: convert_section(&cg.shadows),
        midtones: convert_section(&cg.midtones),
        highlights: convert_section(&cg.highlights),
    });

    drop(entity_cmds);
    drop(cmds);
}

/// Remove all render setting components from a camera, restoring defaults.
pub fn revert_render_settings_on_entity(world: &mut World, entity: Entity) {
    let mut cmds = world.commands();
    let mut entity_cmds = cmds.entity(entity);

    // Restore default tonemapping
    entity_cmds.insert(bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface);

    // Restore default exposure
    entity_cmds.insert(bevy::camera::Exposure { ev100: 9.7 });

    // Remove optional components
    entity_cmds.remove::<bevy::post_process::bloom::Bloom>();
    entity_cmds.remove::<bevy::pbr::DistanceFog>();
    entity_cmds.remove::<bevy::anti_alias::fxaa::Fxaa>();
    entity_cmds.remove::<bevy::pbr::ScreenSpaceAmbientOcclusion>();
    entity_cmds.remove::<bevy::post_process::dof::DepthOfField>();

    // Restore default color grading
    entity_cmds.insert(bevy::render::view::ColorGrading::default());

    // Restore default MSAA (per-camera component)
    entity_cmds.insert(Msaa::default());
}
