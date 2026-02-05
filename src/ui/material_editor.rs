//! Material editor panel for editing materials on selected entities.
//!
//! Reads and writes the `MaterialRef` component directly, which fixes the
//! desync where UI changes were lost on save/load. The `MaterialTypeRegistry`
//! provides extension-specific UI and apply functions.

use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use bevy_editor_game::{
    AlphaModeValue, BaseMaterialProps, MaterialDefinition, MaterialExtensionData, MaterialLibrary,
    MaterialRef,
};

use crate::editor::{EditorMode, EditorState};
use crate::materials::{apply_material_def_standalone, resolve_material_ref, GridMat, MaterialTypeRegistry};
use crate::selection::Selected;
use crate::ui::file_dialog::{FileDialogState, TexturePickResult, TextureSlot};
use crate::ui::theme::{colors, panel, panel_frame};
use crate::utils::should_process_input;

/// Resource storing copied material data for paste operations
#[derive(Resource, Default)]
pub struct CopiedMaterial(pub Option<MaterialDefinition>);

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
    selected_refs: Query<&MaterialRef, With<Selected>>,
    library: Res<MaterialLibrary>,
    mut commands: Commands,
    selected_entities: Query<Entity, With<Selected>>,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    if *mode.get() != EditorMode::Material {
        return;
    }

    // Y to copy material from first selected entity
    if keyboard.just_pressed(KeyCode::KeyY) {
        if let Some(mat_ref) = selected_refs.iter().next() {
            if let Some(def) = resolve_material_ref(mat_ref, &library) {
                copied_material.0 = Some(def.clone());
                info!("Copied material");
            }
        }
        return;
    }

    // P to paste material to all selected entities
    if keyboard.just_pressed(KeyCode::KeyP) {
        if let Some(ref def) = copied_material.0 {
            let mut count = 0;
            for entity in selected_entities.iter() {
                commands
                    .entity(entity)
                    .insert(MaterialRef::Inline(def.clone()));
                count += 1;
            }
            if count > 0 {
                info!("Pasted material to {} entities", count);
            }
        } else {
            info!("No material copied");
        }
    }
}

/// Draw the base PBR material properties
fn draw_base_properties(ui: &mut egui::Ui, base: &mut BaseMaterialProps) -> bool {
    let mut changed = false;

    ui.add_space(4.0);

    // Base Color with alpha
    let mut color_arr = {
        let c = base.base_color.to_srgba();
        [c.red, c.green, c.blue, c.alpha]
    };
    ui.label(egui::RichText::new("Base Color").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        if ui
            .color_edit_button_rgba_unmultiplied(&mut color_arr)
            .changed()
        {
            base.base_color =
                Color::srgba(color_arr[0], color_arr[1], color_arr[2], color_arr[3]);
            changed = true;
        }
    });

    ui.add_space(4.0);

    // Emissive color (RGB) + intensity (W)
    let mut emissive_rgb = [base.emissive.red, base.emissive.green, base.emissive.blue];
    let mut emissive_intensity = base.emissive.alpha;
    ui.label(egui::RichText::new("Emissive").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        if ui.color_edit_button_rgb(&mut emissive_rgb).changed() {
            base.emissive = LinearRgba::new(
                emissive_rgb[0],
                emissive_rgb[1],
                emissive_rgb[2],
                emissive_intensity,
            );
            changed = true;
        }
        ui.label(egui::RichText::new("Intensity").color(colors::TEXT_MUTED));
        if ui
            .add(
                egui::DragValue::new(&mut emissive_intensity)
                    .speed(0.1)
                    .range(0.0..=100.0),
            )
            .changed()
        {
            base.emissive = LinearRgba::new(
                emissive_rgb[0],
                emissive_rgb[1],
                emissive_rgb[2],
                emissive_intensity,
            );
            changed = true;
        }
    });

    ui.add_space(4.0);

    // Metallic
    ui.label(egui::RichText::new("Metallic").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::Slider::new(&mut base.metallic, 0.0..=1.0).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // Roughness
    ui.label(egui::RichText::new("Roughness").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::Slider::new(&mut base.perceptual_roughness, 0.0..=1.0).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // Reflectance
    ui.label(egui::RichText::new("Reflectance").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui
            .add(egui::Slider::new(&mut base.reflectance, 0.0..=1.0).show_value(true))
            .changed();
    });

    ui.add_space(8.0);

    // Transmission
    ui.label(egui::RichText::new("Transmission").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("IOR").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.ior, 1.0..=3.0).show_value(true))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Specular").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.specular_transmission, 0.0..=1.0).show_value(true))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Diffuse").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.diffuse_transmission, 0.0..=1.0).show_value(true))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Thickness").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.thickness, 0.0..=10.0).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // Specular Tint
    let mut tint_arr = {
        let c = base.specular_tint.to_srgba();
        [c.red, c.green, c.blue, c.alpha]
    };
    ui.label(egui::RichText::new("Specular Tint").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        if ui
            .color_edit_button_rgba_unmultiplied(&mut tint_arr)
            .changed()
        {
            base.specular_tint =
                Color::srgba(tint_arr[0], tint_arr[1], tint_arr[2], tint_arr[3]);
            changed = true;
        }
    });

    ui.add_space(4.0);

    // Clearcoat
    ui.label(egui::RichText::new("Clearcoat").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Strength").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.clearcoat, 0.0..=1.0).show_value(true))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Roughness").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.clearcoat_perceptual_roughness, 0.0..=1.0).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // Anisotropy
    ui.label(egui::RichText::new("Anisotropy").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Strength").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.anisotropy_strength, 0.0..=1.0).show_value(true))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Rotation").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::Slider::new(&mut base.anisotropy_rotation, 0.0..=std::f32::consts::TAU).show_value(true))
            .changed();
    });

    ui.add_space(4.0);

    // UV Scale
    ui.label(egui::RichText::new("UV Scale").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("U").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::DragValue::new(&mut base.uv_scale[0]).speed(0.1).range(0.01..=100.0))
            .changed();
        ui.label(egui::RichText::new("V").color(colors::TEXT_MUTED));
        changed |= ui
            .add(egui::DragValue::new(&mut base.uv_scale[1]).speed(0.1).range(0.01..=100.0))
            .changed();
    });

    ui.add_space(4.0);

    // Alpha Mode
    ui.label(egui::RichText::new("Alpha Mode").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("alpha_mode")
            .selected_text(base.alpha_mode.label())
            .show_ui(ui, |ui| {
                for mode in AlphaModeValue::ALL {
                    if ui
                        .selectable_value(&mut base.alpha_mode, mode, mode.label())
                        .changed()
                    {
                        changed = true;
                    }
                }
            });
    });

    // Alpha Cutoff (only shown for Mask mode)
    if base.alpha_mode == AlphaModeValue::Mask {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Alpha Cutoff").color(colors::TEXT_SECONDARY));
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::Slider::new(&mut base.alpha_cutoff, 0.0..=1.0).show_value(true))
                .changed();
        });
    }

    ui.add_space(4.0);

    // Options section
    ui.label(egui::RichText::new("Options").color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut base.double_sided, "Double Sided").changed();
        changed |= ui.checkbox(&mut base.unlit, "Unlit").changed();
    });

    ui.add_space(4.0);

    changed
}

/// Result from drawing texture slot UI
struct TextureSlotResult {
    changed: bool,
    browse_requested: Option<TextureSlot>,
}

/// Draw a single texture slot row
fn draw_texture_row(
    ui: &mut egui::Ui,
    label: &str,
    slot: TextureSlot,
    path: &mut Option<String>,
    result: &mut TextureSlotResult,
) {
    ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
    ui.horizontal(|ui| {
        // Display current path or "None"
        let display = path
            .as_ref()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or("None");
        ui.label(
            egui::RichText::new(display)
                .color(if path.is_some() {
                    colors::TEXT_PRIMARY
                } else {
                    colors::TEXT_MUTED
                })
                .small(),
        );

        if ui
            .small_button("Browse")
            .on_hover_text("Pick an image file")
            .clicked()
        {
            result.browse_requested = Some(slot);
        }

        if path.is_some()
            && ui
                .small_button("X")
                .on_hover_text("Clear texture")
                .clicked()
        {
            *path = None;
            result.changed = true;
        }
    });
    ui.add_space(2.0);
}

/// Draw texture slot UI for all 5 PBR texture maps
fn draw_texture_slots(ui: &mut egui::Ui, base: &mut BaseMaterialProps) -> TextureSlotResult {
    let mut result = TextureSlotResult {
        changed: false,
        browse_requested: None,
    };

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Textures")
            .strong()
            .color(colors::TEXT_PRIMARY),
    );
    ui.add_space(4.0);

    draw_texture_row(ui, "Base Color", TextureSlot::BaseColor, &mut base.base_color_texture, &mut result);
    draw_texture_row(ui, "Normal Map", TextureSlot::NormalMap, &mut base.normal_map_texture, &mut result);
    draw_texture_row(ui, "Metallic/Roughness", TextureSlot::MetallicRoughness, &mut base.metallic_roughness_texture, &mut result);
    draw_texture_row(ui, "Emissive", TextureSlot::Emissive, &mut base.emissive_texture, &mut result);
    draw_texture_row(ui, "Occlusion", TextureSlot::Occlusion, &mut base.occlusion_texture, &mut result);

    result
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

    // Collect selected entity info
    let selected_entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).collect()
    };

    let total_selected = selected_entities.len();
    let first_entity = selected_entities.first().copied();

    // Get MaterialRef + Name for first selected entity
    let (entity_name, current_mat_ref) = if let Some(entity) = first_entity {
        let name = world.get::<Name>(entity).map(|n| n.as_str().to_string());
        let mat_ref = world.get::<MaterialRef>(entity).cloned();
        (name, mat_ref)
    } else {
        (None, None)
    };

    // Resolve the definition (we need a clone because we'll mutate it)
    let library = world
        .get_resource::<MaterialLibrary>()
        .cloned()
        .unwrap_or_default();

    let mut working_def = current_mat_ref
        .as_ref()
        .and_then(|r| resolve_material_ref(r, &library))
        .cloned();

    // Check for texture pick result and apply it
    if let Some(entity) = first_entity {
        let pick_data = world.resource_mut::<TexturePickResult>().0.take();
        if let Some(pick) = pick_data {
            if pick.entity == entity {
                if let Some(def) = &mut working_def {
                    match pick.slot {
                        TextureSlot::BaseColor => def.base.base_color_texture = Some(pick.path),
                        TextureSlot::NormalMap => def.base.normal_map_texture = Some(pick.path),
                        TextureSlot::MetallicRoughness => def.base.metallic_roughness_texture = Some(pick.path),
                        TextureSlot::Emissive => def.base.emissive_texture = Some(pick.path),
                        TextureSlot::Occlusion => def.base.occlusion_texture = Some(pick.path),
                    }
                    // Apply immediately
                    let def_clone = def.clone();
                    apply_and_update_entity(world, entity, def_clone);
                }
            }
        }
    }

    let original_def = working_def.clone();

    // Collect available material type names from registry
    let type_names: Vec<(&'static str, &'static str)> = world
        .get_resource::<MaterialTypeRegistry>()
        .map(|reg| {
            reg.types
                .iter()
                .map(|e| (e.type_name, e.display_name))
                .collect()
        })
        .unwrap_or_default();

    // Determine current extension type name (owned to avoid borrow conflict)
    let current_type_name: String = working_def
        .as_ref()
        .and_then(|d| d.extension.as_ref())
        .map(|e| e.type_name.clone())
        .unwrap_or_else(|| "standard".to_string());

    // Extract the draw_extension_ui function pointer before entering the UI closure
    let ext_draw_fn: Option<fn(&mut egui::Ui, &str) -> (bool, String)> = working_def
        .as_ref()
        .and_then(|d| d.extension.as_ref())
        .and_then(|ext| {
            world
                .get_resource::<MaterialTypeRegistry>()
                .and_then(|r| r.find(&ext.type_name))
                .map(|e| e.draw_extension_ui)
        });

    // Track if material type should change
    let mut new_type_name: Option<String> = None;

    // Track extension UI changes
    let mut ext_changed = false;
    let mut new_ext_data: Option<String> = None;

    // Track texture slot UI changes
    let mut browse_texture_slot: Option<TextureSlot> = None;
    let mut texture_changed = false;

    // Has material at all?
    let has_material = current_mat_ref.is_some();

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
    let available_height =
        ctx.content_rect().height() - panel::STATUS_BAR_HEIGHT - panel::WINDOW_PADDING * 2.0;

    egui::Window::new("Material")
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
            ui.set_min_height(
                available_height - panel::TITLE_BAR_HEIGHT - panel::BOTTOM_PADDING,
            );

            if total_selected == 0 {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No entity selected")
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "Select an entity with a material\nto edit its properties.",
                        )
                        .small()
                        .color(colors::TEXT_MUTED),
                    );
                });
            } else if !has_material {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No material on selection")
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Selected entities don't have\na material component.")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            } else if total_selected == 1 {
                ui.add_space(4.0);

                // Entity name
                if let Some(name) = &entity_name {
                    ui.label(
                        egui::RichText::new(name)
                            .strong()
                            .size(14.0)
                            .color(colors::TEXT_PRIMARY),
                    );
                } else if let Some(entity) = first_entity {
                    ui.label(
                        egui::RichText::new(format!("Entity {:?}", entity))
                            .strong()
                            .size(14.0)
                            .color(colors::TEXT_PRIMARY),
                    );
                }

                // Library/Inline indicator
                if let Some(ref mat_ref) = current_mat_ref {
                    match mat_ref {
                        MaterialRef::Library(name) => {
                            ui.label(
                                egui::RichText::new(format!("Library: {}", name))
                                    .small()
                                    .color(colors::ACCENT_BLUE),
                            );
                        }
                        MaterialRef::Inline(_) => {
                            ui.label(
                                egui::RichText::new("Inline material")
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                        }
                    }
                }

                ui.add_space(4.0);

                // Material type selector
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Type").color(colors::TEXT_SECONDARY));
                    let current_display = type_names
                        .iter()
                        .find(|(tn, _)| *tn == current_type_name)
                        .map(|(_, dn)| *dn)
                        .unwrap_or("Standard");

                    egui::ComboBox::from_id_salt("material_type")
                        .selected_text(current_display)
                        .show_ui(ui, |ui| {
                            for &(type_name, display_name) in &type_names {
                                if ui
                                    .selectable_label(
                                        type_name == current_type_name,
                                        display_name,
                                    )
                                    .clicked()
                                    && type_name != current_type_name
                                {
                                    new_type_name = Some(type_name.to_string());
                                }
                            }
                        });
                });

                ui.add_space(4.0);
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            if let Some(def) = &mut working_def {
                                // Base material properties
                                draw_base_properties(ui, &mut def.base);

                                // Texture slots
                                let tex_result = draw_texture_slots(ui, &mut def.base);
                                if tex_result.changed {
                                    texture_changed = true;
                                }
                                if tex_result.browse_requested.is_some() {
                                    browse_texture_slot = tex_result.browse_requested;
                                }

                                // Extension-specific UI (from pre-extracted fn pointer)
                                if let (Some(ext), Some(draw_fn)) =
                                    (&def.extension, ext_draw_fn)
                                {
                                    let (ch, nd) = draw_fn(ui, &ext.data);
                                    if ch {
                                        ext_changed = true;
                                        new_ext_data = Some(nd);
                                    }
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
                        egui::RichText::new(format!("{} entities selected", total_selected))
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

    let Some(entity) = first_entity else { return };
    if total_selected != 1 { return; }

    // Handle material type change
    if let Some(new_type) = new_type_name {
        if let Some(mut def) = working_def.take() {
            if new_type == "standard" {
                def.extension = None;
            } else {
                // Get default extension data from registry
                let default_data = world
                    .get_resource::<MaterialTypeRegistry>()
                    .and_then(|r| r.find(&new_type))
                    .and_then(|e| e.default_extension_data.clone())
                    .unwrap_or_default();
                def.extension = Some(MaterialExtensionData {
                    type_name: new_type,
                    data: default_data,
                });
            }

            apply_and_update_entity(world, entity, def);
        }
        return;
    }

    // Apply extension data changes
    if ext_changed {
        if let Some(mut def) = working_def.take() {
            if let Some(new_data) = new_ext_data {
                if let Some(ext) = &mut def.extension {
                    ext.data = new_data;
                }
            }

            apply_and_update_entity(world, entity, def);
        }
        return;
    }

    // Open file dialog for texture browsing
    if let Some(slot) = browse_texture_slot {
        world
            .resource_mut::<FileDialogState>()
            .open_pick_texture(slot, entity);
        return;
    }

    // Apply texture clear changes
    if texture_changed {
        if let Some(modified) = &working_def {
            apply_and_update_entity(world, entity, modified.clone());
        }
        return;
    }

    // Apply base property changes
    if let (Some(original), Some(modified)) = (&original_def, &working_def) {
        let base_changed = original.base.base_color != modified.base.base_color
            || original.base.metallic != modified.base.metallic
            || original.base.perceptual_roughness != modified.base.perceptual_roughness
            || original.base.reflectance != modified.base.reflectance
            || original.base.alpha_mode != modified.base.alpha_mode
            || original.base.alpha_cutoff != modified.base.alpha_cutoff
            || original.base.double_sided != modified.base.double_sided
            || original.base.unlit != modified.base.unlit
            || original.base.emissive != modified.base.emissive
            || original.base.ior != modified.base.ior
            || original.base.specular_transmission != modified.base.specular_transmission
            || original.base.specular_tint != modified.base.specular_tint
            || original.base.clearcoat != modified.base.clearcoat
            || original.base.clearcoat_perceptual_roughness != modified.base.clearcoat_perceptual_roughness
            || original.base.anisotropy_strength != modified.base.anisotropy_strength
            || original.base.anisotropy_rotation != modified.base.anisotropy_rotation
            || original.base.diffuse_transmission != modified.base.diffuse_transmission
            || original.base.thickness != modified.base.thickness
            || original.base.uv_scale != modified.base.uv_scale;

        if base_changed {
            apply_and_update_entity(world, entity, modified.clone());
        }
    }
}

/// Remove old material, insert new MaterialRef, and apply the definition to the entity.
fn apply_and_update_entity(world: &mut World, entity: Entity, def: MaterialDefinition) {
    // Remove old material components
    remove_all_material_components(world, entity);

    // Write new MaterialRef
    let mat_ref = MaterialRef::Inline(def.clone());
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(mat_ref);
    }

    // Apply material (registry lookup + world mutation in one step)
    apply_material_def_standalone(world, entity, &def);
}

/// Remove all possible material components from an entity.
fn remove_all_material_components(world: &mut World, entity: Entity) {
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.remove::<MeshMaterial3d<StandardMaterial>>();
        e.remove::<MeshMaterial3d<GridMat>>();
    }
}
