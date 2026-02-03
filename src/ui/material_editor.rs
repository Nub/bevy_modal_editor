//! Material editor panel for editing StandardMaterial properties on selected entities.

use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState};
use crate::selection::Selected;
use crate::ui::theme::{colors, panel, panel_frame};
use crate::utils::should_process_input;

/// Resource storing copied material data for paste operations
#[derive(Resource, Default)]
pub struct CopiedMaterial(pub Option<StandardMaterialData>);

pub struct MaterialEditorPlugin;

impl Plugin for MaterialEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CopiedMaterial>()
            .add_systems(Update, handle_material_copy_paste)
            .add_systems(EguiPrimaryContextPass, draw_material_panel);
    }
}

/// Handle Y to copy and P to paste materials in Material mode
fn handle_material_copy_paste(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
    mut copied_material: ResMut<CopiedMaterial>,
    selected: Query<&MeshMaterial3d<StandardMaterial>, With<Selected>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Only handle Y/P in Material mode
    if *mode.get() != EditorMode::Material {
        return;
    }

    // Y to copy material from first selected entity
    if keyboard.just_pressed(KeyCode::KeyY) {
        if let Some(mat_handle) = selected.iter().next() {
            if let Some(material) = materials.get(&mat_handle.0) {
                copied_material.0 = Some(StandardMaterialData::from_material(material));
                info!("Copied material");
            }
        }
        return;
    }

    // P to paste material to all selected entities
    if keyboard.just_pressed(KeyCode::KeyP) {
        if let Some(ref data) = copied_material.0 {
            let mut count = 0;
            for mat_handle in selected.iter() {
                if let Some(material) = materials.get_mut(&mat_handle.0) {
                    data.apply_to_material(material);
                    count += 1;
                }
            }
            if count > 0 {
                info!("Pasted material to {} entities", count);
            }
        } else {
            info!("No material copied");
        }
    }
}

/// Extracted StandardMaterial data for UI editing
#[derive(Clone)]
pub struct StandardMaterialData {
    base_color: [f32; 4],
    emissive: [f32; 4],
    metallic: f32,
    perceptual_roughness: f32,
    reflectance: f32,
    alpha_mode: AlphaModeSelection,
    alpha_cutoff: f32,
    double_sided: bool,
    unlit: bool,
}

/// Simplified alpha mode for UI selection
#[derive(Clone, Copy, PartialEq, Eq)]
enum AlphaModeSelection {
    Opaque,
    Mask,
    Blend,
    AlphaToCoverage,
}

impl AlphaModeSelection {
    fn from_alpha_mode(mode: &AlphaMode) -> Self {
        match mode {
            AlphaMode::Opaque => AlphaModeSelection::Opaque,
            AlphaMode::Mask(_) => AlphaModeSelection::Mask,
            AlphaMode::Blend => AlphaModeSelection::Blend,
            AlphaMode::AlphaToCoverage => AlphaModeSelection::AlphaToCoverage,
            _ => AlphaModeSelection::Opaque,
        }
    }

    fn to_alpha_mode(self, cutoff: f32) -> AlphaMode {
        match self {
            AlphaModeSelection::Opaque => AlphaMode::Opaque,
            AlphaModeSelection::Mask => AlphaMode::Mask(cutoff),
            AlphaModeSelection::Blend => AlphaMode::Blend,
            AlphaModeSelection::AlphaToCoverage => AlphaMode::AlphaToCoverage,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            AlphaModeSelection::Opaque => "Opaque",
            AlphaModeSelection::Mask => "Mask",
            AlphaModeSelection::Blend => "Blend",
            AlphaModeSelection::AlphaToCoverage => "Alpha to Coverage",
        }
    }

    const ALL: [AlphaModeSelection; 4] = [
        AlphaModeSelection::Opaque,
        AlphaModeSelection::Mask,
        AlphaModeSelection::Blend,
        AlphaModeSelection::AlphaToCoverage,
    ];
}

impl StandardMaterialData {
    fn from_material(mat: &StandardMaterial) -> Self {
        let base = mat.base_color.to_srgba();
        let emissive = mat.emissive.to_vec4();
        let alpha_mode = AlphaModeSelection::from_alpha_mode(&mat.alpha_mode);
        let alpha_cutoff = match mat.alpha_mode {
            AlphaMode::Mask(c) => c,
            _ => 0.5,
        };

        Self {
            base_color: [base.red, base.green, base.blue, base.alpha],
            emissive: [emissive.x, emissive.y, emissive.z, emissive.w],
            metallic: mat.metallic,
            perceptual_roughness: mat.perceptual_roughness,
            reflectance: mat.reflectance,
            alpha_mode,
            alpha_cutoff,
            double_sided: mat.double_sided,
            unlit: mat.unlit,
        }
    }

    fn apply_to_material(&self, mat: &mut StandardMaterial) {
        mat.base_color = Color::srgba(
            self.base_color[0],
            self.base_color[1],
            self.base_color[2],
            self.base_color[3],
        );
        mat.emissive = LinearRgba::new(
            self.emissive[0],
            self.emissive[1],
            self.emissive[2],
            self.emissive[3],
        );
        mat.metallic = self.metallic;
        mat.perceptual_roughness = self.perceptual_roughness;
        mat.reflectance = self.reflectance;
        mat.alpha_mode = self.alpha_mode.to_alpha_mode(self.alpha_cutoff);
        mat.double_sided = self.double_sided;
        mat.unlit = self.unlit;
    }
}

/// Draw the material properties section
fn draw_material_properties(ui: &mut egui::Ui, data: &mut StandardMaterialData) -> bool {
    let mut changed = false;

    ui.add_space(4.0);

    // Base Color with alpha
    ui.label(egui::RichText::new("Base Color").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui.color_edit_button_rgba_unmultiplied(&mut data.base_color).changed();
    });

    ui.add_space(4.0);

    // Emissive color (RGB only, intensity is the W component)
    ui.label(egui::RichText::new("Emissive").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        let mut emissive_rgb = [data.emissive[0], data.emissive[1], data.emissive[2]];
        if ui.color_edit_button_rgb(&mut emissive_rgb).changed() {
            data.emissive[0] = emissive_rgb[0];
            data.emissive[1] = emissive_rgb[1];
            data.emissive[2] = emissive_rgb[2];
            changed = true;
        }
        ui.label(egui::RichText::new("Intensity").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::DragValue::new(&mut data.emissive[3]).speed(0.1).range(0.0..=100.0))
            .changed();
    });

    ui.add_space(4.0);

    // Metallic
    ui.label(egui::RichText::new("Metallic").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::Slider::new(&mut data.metallic, 0.0..=1.0).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // Roughness
    ui.label(egui::RichText::new("Roughness").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::Slider::new(&mut data.perceptual_roughness, 0.0..=1.0).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // Reflectance
    ui.label(egui::RichText::new("Reflectance").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::Slider::new(&mut data.reflectance, 0.0..=1.0).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // Alpha Mode
    ui.label(egui::RichText::new("Alpha Mode").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("alpha_mode")
            .selected_text(data.alpha_mode.label())
            .show_ui(ui, |ui| {
                for mode in AlphaModeSelection::ALL {
                    if ui.selectable_value(&mut data.alpha_mode, mode, mode.label()).changed() {
                        changed = true;
                    }
                }
            });
    });

    // Alpha Cutoff (only shown for Mask mode)
    if data.alpha_mode == AlphaModeSelection::Mask {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Alpha Cutoff").color(colors::TEXT_SECONDARY));
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::Slider::new(&mut data.alpha_cutoff, 0.0..=1.0).show_value(true))
                .changed();
        });
    }

    ui.add_space(4.0);

    // Options section
    ui.label(egui::RichText::new("Options").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut data.double_sided, "Double Sided").changed();
        changed |= ui.checkbox(&mut data.unlit, "Unlit").changed();
    });

    ui.add_space(4.0);

    changed
}

/// Draw the material editor panel
fn draw_material_panel(world: &mut World) {
    // Don't draw UI when editor is disabled
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    // Only show in Material mode
    let current_mode = world.resource::<State<EditorMode>>().get();
    if *current_mode != EditorMode::Material {
        return;
    }

    // Query for selected entities with materials
    let selected_with_materials: Vec<(Entity, AssetId<StandardMaterial>)> = {
        let mut query = world.query_filtered::<(Entity, &MeshMaterial3d<StandardMaterial>), With<Selected>>();
        query
            .iter(world)
            .map(|(e, mat)| (e, mat.0.id()))
            .collect()
    };

    let selection_count = selected_with_materials.len();

    // Get entity name for single selection
    let entity_name = if selection_count == 1 {
        world
            .get::<Name>(selected_with_materials[0].0)
            .map(|n| n.as_str().to_string())
    } else {
        None
    };

    // Extract material data for single selection
    let mut material_data = if selection_count == 1 {
        let asset_id = selected_with_materials[0].1;
        world
            .resource::<Assets<StandardMaterial>>()
            .get(asset_id)
            .map(StandardMaterialData::from_material)
    } else {
        None
    };

    // Store original data for change detection
    let original_data = material_data.clone();

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

    // Calculate available height using shared panel settings
    let available_height = ctx.content_rect().height()
        - panel::STATUS_BAR_HEIGHT
        - panel::WINDOW_PADDING * 2.0;

    egui::Window::new("Material")
        .default_size([panel::DEFAULT_WIDTH, available_height])
        .min_width(panel::MIN_WIDTH)
        .min_height(panel::MIN_HEIGHT)
        .max_height(available_height)
        .anchor(egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            // Force the window content to fill available height
            ui.set_min_height(available_height - panel::TITLE_BAR_HEIGHT - panel::BOTTOM_PADDING);

            // Check selection state
            let total_selected: usize = {
                let mut query = world.query_filtered::<Entity, With<Selected>>();
                query.iter(world).count()
            };

            if total_selected == 0 {
                // No selection at all
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No entity selected")
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Select an entity with a material\nto edit its properties.")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            } else if selection_count == 0 {
                // Selection exists but no materials
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No material on selection")
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Selected entities don't have\na StandardMaterial.")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            } else if selection_count == 1 {
                // Single selection with material
                ui.add_space(4.0);

                // Entity name
                if let Some(name) = &entity_name {
                    ui.label(
                        egui::RichText::new(name)
                            .strong()
                            .size(14.0)
                            .color(colors::TEXT_PRIMARY),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!("Entity {:?}", selected_with_materials[0].0))
                            .strong()
                            .size(14.0)
                            .color(colors::TEXT_PRIMARY),
                    );
                }

                ui.label(
                    egui::RichText::new("StandardMaterial")
                        .small()
                        .color(colors::TEXT_MUTED),
                );

                ui.add_space(4.0);
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(ref mut data) = material_data {
                        draw_material_properties(ui, data);
                    }
                });
            } else {
                // Multiple selection
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} entities selected", selection_count))
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Multi-material editing\nnot yet supported.")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            }
        });

    // Apply changes back to material if data changed
    if let (Some(original), Some(modified)) = (original_data, material_data) {
        // Simple change detection by comparing fields
        let changed = original.base_color != modified.base_color
            || original.emissive != modified.emissive
            || original.metallic != modified.metallic
            || original.perceptual_roughness != modified.perceptual_roughness
            || original.reflectance != modified.reflectance
            || original.alpha_mode != modified.alpha_mode
            || original.alpha_cutoff != modified.alpha_cutoff
            || original.double_sided != modified.double_sided
            || original.unlit != modified.unlit;

        if changed && selection_count == 1 {
            let asset_id = selected_with_materials[0].1;
            if let Some(material) = world.resource_mut::<Assets<StandardMaterial>>().get_mut(asset_id) {
                modified.apply_to_material(material);
            }
        }
    }
}
