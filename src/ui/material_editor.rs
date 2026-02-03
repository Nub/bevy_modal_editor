//! Material editor panel for editing StandardMaterial and GridMaterial properties on selected entities.

use bevy::pbr::{ExtendedMaterial, StandardMaterial};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use bevy_grid_shader::{GridAxes, GridMaterial};

use crate::editor::{EditorMode, EditorState};
use crate::scene::MaterialType;
use crate::selection::Selected;
use crate::ui::theme::{colors, panel, panel_frame};
use crate::utils::should_process_input;

/// Type alias for the extended grid material
pub type GridMat = ExtendedMaterial<StandardMaterial, GridMaterial>;

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
    selected_std: Query<&MeshMaterial3d<StandardMaterial>, With<Selected>>,
    selected_grid: Query<&MeshMaterial3d<GridMat>, With<Selected>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    grid_materials: Res<Assets<GridMat>>,
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
        // Try StandardMaterial first
        if let Some(mat_handle) = selected_std.iter().next() {
            if let Some(material) = std_materials.get(&mat_handle.0) {
                copied_material.0 = Some(StandardMaterialData::from_material(material));
                info!("Copied material");
                return;
            }
        }
        // Try GridMaterial
        if let Some(mat_handle) = selected_grid.iter().next() {
            if let Some(material) = grid_materials.get(&mat_handle.0) {
                copied_material.0 = Some(StandardMaterialData::from_material(&material.base));
                info!("Copied material (base properties)");
            }
        }
        return;
    }

    // P to paste material to all selected entities (only base properties)
    if keyboard.just_pressed(KeyCode::KeyP) {
        if let Some(ref data) = copied_material.0 {
            let mut count = 0;
            for mat_handle in selected_std.iter() {
                if let Some(material) = std_materials.get_mut(&mat_handle.0) {
                    data.apply_to_material(material);
                    count += 1;
                }
            }
            // Note: For grid materials, we'd need mutable access which requires more complex handling
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

/// Extracted GridMaterial data for UI editing
#[derive(Clone)]
pub struct GridMaterialData {
    line_color: [f32; 4],
    major_line_color: [f32; 4],
    line_width: f32,
    major_line_width: f32,
    grid_scale: f32,
    major_line_every: u32,
    axes_xz: bool,
    axes_xy: bool,
    axes_yz: bool,
    fade_distance: f32,
    fade_strength: f32,
}

impl GridMaterialData {
    fn from_grid_material(mat: &GridMaterial) -> Self {
        let axes = mat.uniform.axes;
        Self {
            line_color: [
                mat.uniform.line_color.red,
                mat.uniform.line_color.green,
                mat.uniform.line_color.blue,
                mat.uniform.line_color.alpha,
            ],
            major_line_color: [
                mat.uniform.major_line_color.red,
                mat.uniform.major_line_color.green,
                mat.uniform.major_line_color.blue,
                mat.uniform.major_line_color.alpha,
            ],
            line_width: mat.uniform.line_width,
            major_line_width: mat.uniform.major_line_width,
            grid_scale: mat.uniform.grid_scale,
            major_line_every: mat.uniform.major_line_every,
            axes_xz: (axes & GridAxes::XZ.bits()) != 0,
            axes_xy: (axes & GridAxes::XY.bits()) != 0,
            axes_yz: (axes & GridAxes::YZ.bits()) != 0,
            fade_distance: mat.uniform.fade_distance,
            fade_strength: mat.uniform.fade_strength,
        }
    }

    fn apply_to_grid_material(&self, mat: &mut GridMaterial) {
        mat.uniform.line_color = LinearRgba::new(
            self.line_color[0],
            self.line_color[1],
            self.line_color[2],
            self.line_color[3],
        );
        mat.uniform.major_line_color = LinearRgba::new(
            self.major_line_color[0],
            self.major_line_color[1],
            self.major_line_color[2],
            self.major_line_color[3],
        );
        mat.uniform.line_width = self.line_width;
        mat.uniform.major_line_width = self.major_line_width;
        mat.uniform.grid_scale = self.grid_scale;
        mat.uniform.major_line_every = self.major_line_every;

        let mut axes = 0u32;
        if self.axes_xz { axes |= GridAxes::XZ.bits(); }
        if self.axes_xy { axes |= GridAxes::XY.bits(); }
        if self.axes_yz { axes |= GridAxes::YZ.bits(); }
        mat.uniform.axes = axes;

        mat.uniform.fade_distance = self.fade_distance;
        mat.uniform.fade_strength = self.fade_strength;
    }
}

impl Default for GridMaterialData {
    fn default() -> Self {
        Self {
            line_color: [0.3, 0.3, 0.3, 1.0],
            major_line_color: [0.5, 0.5, 0.5, 1.0],
            line_width: 1.0,
            major_line_width: 2.0,
            grid_scale: 1.0,
            major_line_every: 5,
            axes_xz: true,
            axes_xy: false,
            axes_yz: false,
            fade_distance: 50.0,
            fade_strength: 1.0,
        }
    }
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

    fn to_material(&self) -> StandardMaterial {
        let mut mat = StandardMaterial::default();
        self.apply_to_material(&mut mat);
        mat
    }
}

/// Draw the standard material properties section
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

/// Draw the grid material properties section
fn draw_grid_properties(ui: &mut egui::Ui, data: &mut GridMaterialData) -> bool {
    let mut changed = false;

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Grid Properties").color(colors::ACCENT_CYAN).strong());
    ui.add_space(4.0);

    // Line Color
    ui.label(egui::RichText::new("Line Color").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui.color_edit_button_rgba_unmultiplied(&mut data.line_color).changed();
    });

    ui.add_space(4.0);

    // Major Line Color
    ui.label(egui::RichText::new("Major Line Color").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui.color_edit_button_rgba_unmultiplied(&mut data.major_line_color).changed();
    });

    ui.add_space(4.0);

    // Line Width
    ui.label(egui::RichText::new("Line Width").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::DragValue::new(&mut data.line_width).speed(0.1).range(0.1..=10.0))
            .changed();
    });

    ui.add_space(4.0);

    // Major Line Width
    ui.label(egui::RichText::new("Major Line Width").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::DragValue::new(&mut data.major_line_width).speed(0.1).range(0.1..=10.0))
            .changed();
    });

    ui.add_space(4.0);

    // Grid Scale
    ui.label(egui::RichText::new("Grid Scale").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::DragValue::new(&mut data.grid_scale).speed(0.1).range(0.1..=100.0))
            .changed();
    });

    ui.add_space(4.0);

    // Major Line Every
    ui.label(egui::RichText::new("Major Line Every").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        let mut major = data.major_line_every as i32;
        if ui.add(egui::DragValue::new(&mut major).range(1..=100)).changed() {
            data.major_line_every = major.max(1) as u32;
            changed = true;
        }
    });

    ui.add_space(4.0);

    // Axes
    ui.label(egui::RichText::new("Axes").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut data.axes_xz, "XZ").changed();
        changed |= ui.checkbox(&mut data.axes_xy, "XY").changed();
        changed |= ui.checkbox(&mut data.axes_yz, "YZ").changed();
    });

    ui.add_space(4.0);

    // Fade Distance
    ui.label(egui::RichText::new("Fade Distance").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::DragValue::new(&mut data.fade_distance).speed(1.0).range(0.0..=1000.0))
            .changed();
    });

    ui.add_space(4.0);

    // Fade Strength
    ui.label(egui::RichText::new("Fade Strength").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::DragValue::new(&mut data.fade_strength).speed(0.1).range(0.0..=10.0))
            .changed();
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

    // Query for selected entities with standard materials
    let selected_std: Vec<(Entity, AssetId<StandardMaterial>)> = {
        let mut query = world.query_filtered::<(Entity, &MeshMaterial3d<StandardMaterial>), With<Selected>>();
        query
            .iter(world)
            .map(|(e, mat)| (e, mat.0.id()))
            .collect()
    };

    // Query for selected entities with grid materials
    let selected_grid: Vec<(Entity, AssetId<GridMat>)> = {
        let mut query = world.query_filtered::<(Entity, &MeshMaterial3d<GridMat>), With<Selected>>();
        query
            .iter(world)
            .map(|(e, mat)| (e, mat.0.id()))
            .collect()
    };

    // Determine which material type we have
    let has_std = !selected_std.is_empty();
    let has_grid = !selected_grid.is_empty();
    let selection_count = if has_std { selected_std.len() } else { selected_grid.len() };

    // Get the selected entity
    let selected_entity = if has_std {
        Some(selected_std[0].0)
    } else if has_grid {
        Some(selected_grid[0].0)
    } else {
        None
    };

    // Get entity name and material type
    let (entity_name, current_material_type) = if let Some(entity) = selected_entity {
        let name = world.get::<Name>(entity).map(|n| n.as_str().to_string());
        let mat_type = world.get::<MaterialType>(entity).copied().unwrap_or(MaterialType::Standard);
        (name, Some(mat_type))
    } else {
        (None, None)
    };

    // Extract material data for single selection
    let mut std_data = if selection_count == 1 && has_std {
        let asset_id = selected_std[0].1;
        world
            .resource::<Assets<StandardMaterial>>()
            .get(asset_id)
            .map(StandardMaterialData::from_material)
    } else if selection_count == 1 && has_grid {
        let asset_id = selected_grid[0].1;
        world
            .resource::<Assets<GridMat>>()
            .get(asset_id)
            .map(|m| StandardMaterialData::from_material(&m.base))
    } else {
        None
    };

    let mut grid_data = if selection_count == 1 && has_grid {
        let asset_id = selected_grid[0].1;
        world
            .resource::<Assets<GridMat>>()
            .get(asset_id)
            .map(|m| GridMaterialData::from_grid_material(&m.extension))
    } else {
        None
    };

    // Store original data for change detection
    let original_std_data = std_data.clone();
    let original_grid_data = grid_data.clone();

    // Track if material type should change
    let mut new_material_type: Option<MaterialType> = None;

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
            } else if !has_std && !has_grid {
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
                        egui::RichText::new("Selected entities don't have\na material.")
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
                } else if let Some(entity) = selected_entity {
                    ui.label(
                        egui::RichText::new(format!("Entity {:?}", entity))
                            .strong()
                            .size(14.0)
                            .color(colors::TEXT_PRIMARY),
                    );
                }

                ui.add_space(4.0);

                // Material type selector
                if let Some(mat_type) = current_material_type {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Type").color(colors::TEXT_SECONDARY));
                        let mut selected_type = mat_type;
                        egui::ComboBox::from_id_salt("material_type")
                            .selected_text(match selected_type {
                                MaterialType::Standard => "Standard",
                                MaterialType::Grid => "Grid",
                            })
                            .show_ui(ui, |ui| {
                                if ui.selectable_value(&mut selected_type, MaterialType::Standard, "Standard").clicked() {
                                    if selected_type != mat_type {
                                        new_material_type = Some(MaterialType::Standard);
                                    }
                                }
                                if ui.selectable_value(&mut selected_type, MaterialType::Grid, "Grid").clicked() {
                                    if selected_type != mat_type {
                                        new_material_type = Some(MaterialType::Grid);
                                    }
                                }
                            });
                    });
                }

                ui.add_space(4.0);
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0); // Left padding
                        ui.vertical(|ui| {
                            // Always show base material properties
                            if let Some(ref mut data) = std_data {
                                draw_material_properties(ui, data);
                            }

                            // Show grid properties only for grid materials
                            if has_grid {
                                if let Some(ref mut data) = grid_data {
                                    draw_grid_properties(ui, data);
                                }
                            }
                        });
                    });
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

    // Handle material type change
    if let (Some(new_type), Some(entity)) = (new_material_type, selected_entity) {
        switch_material_type(world, entity, new_type, std_data.as_ref());
        return; // Don't apply other changes when switching type
    }

    // Apply changes back to standard material
    if let (Some(original), Some(modified)) = (&original_std_data, &std_data) {
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
            if has_std {
                let asset_id = selected_std[0].1;
                if let Some(material) = world.resource_mut::<Assets<StandardMaterial>>().get_mut(asset_id) {
                    modified.apply_to_material(material);
                }
            } else if has_grid {
                let asset_id = selected_grid[0].1;
                if let Some(material) = world.resource_mut::<Assets<GridMat>>().get_mut(asset_id) {
                    modified.apply_to_material(&mut material.base);
                }
            }
        }
    }

    // Apply changes back to grid material extension
    if let (Some(original), Some(modified)) = (&original_grid_data, &grid_data) {
        let changed = original.line_color != modified.line_color
            || original.major_line_color != modified.major_line_color
            || original.line_width != modified.line_width
            || original.major_line_width != modified.major_line_width
            || original.grid_scale != modified.grid_scale
            || original.major_line_every != modified.major_line_every
            || original.axes_xz != modified.axes_xz
            || original.axes_xy != modified.axes_xy
            || original.axes_yz != modified.axes_yz
            || original.fade_distance != modified.fade_distance
            || original.fade_strength != modified.fade_strength;

        if changed && selection_count == 1 && has_grid {
            let asset_id = selected_grid[0].1;
            if let Some(material) = world.resource_mut::<Assets<GridMat>>().get_mut(asset_id) {
                modified.apply_to_grid_material(&mut material.extension);
            }
        }
    }
}

/// Switch an entity's material type
fn switch_material_type(
    world: &mut World,
    entity: Entity,
    new_type: MaterialType,
    current_std_data: Option<&StandardMaterialData>,
) {
    // Get the base material data to preserve
    let base_material = current_std_data.map(|d| d.to_material()).unwrap_or_default();

    match new_type {
        MaterialType::Standard => {
            // Remove grid material component if present
            world.entity_mut(entity).remove::<MeshMaterial3d<GridMat>>();

            // Add standard material
            let handle = world.resource_mut::<Assets<StandardMaterial>>().add(base_material);
            world.entity_mut(entity).insert(MeshMaterial3d(handle));
        }
        MaterialType::Grid => {
            // Remove standard material component if present
            world.entity_mut(entity).remove::<MeshMaterial3d<StandardMaterial>>();

            // Create extended material with grid extension
            let grid_mat = ExtendedMaterial {
                base: base_material,
                extension: GridMaterial::default(),
            };
            let handle = world.resource_mut::<Assets<GridMat>>().add(grid_mat);
            world.entity_mut(entity).insert(MeshMaterial3d(handle));
        }
    }

    // Update the material type marker
    world.entity_mut(entity).insert(new_type);

    info!("Switched material type to {:?}", new_type);
}
