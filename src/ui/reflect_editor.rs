//! Reflection-based component editor UI
//!
//! This module provides UI generation for components using Bevy's reflection system,
//! replacing the need for bevy_inspector_egui.

use bevy::prelude::*;
use bevy::reflect::{
    DynamicEnum, DynamicVariant, Enum, PartialReflect, ReflectMut, Struct, TypeInfo, VariantInfo,
};
use bevy_egui::egui;
use std::any::TypeId;
use std::collections::HashSet;

use super::theme::colors;

/// Width of the collapse icon for indentation alignment
const COLLAPSE_ICON_WIDTH: f32 = 18.0;

/// Resource that caches which component types are immutable (cannot be modified via reflection)
/// This prevents repeated panic checks that spam error messages
#[derive(Resource, Default)]
pub struct ImmutableComponentCache {
    /// Set of TypeIds that are known to be immutable
    pub immutable_types: HashSet<TypeId>,
}

/// Configuration for the reflect editor
#[derive(Clone)]
pub struct ReflectEditorConfig {
    /// Speed for drag values
    pub drag_speed: f32,
    /// Whether to show field types
    pub show_types: bool,
    /// Whether this is a top-level component (needs indent for non-collapsible items)
    pub is_top_level: bool,
}

impl Default for ReflectEditorConfig {
    fn default() -> Self {
        Self {
            drag_speed: 0.1,
            show_types: false,
            is_top_level: true,
        }
    }
}

impl ReflectEditorConfig {
    /// Create a config for nested fields (no extra indentation)
    pub fn nested(&self) -> Self {
        Self {
            is_top_level: false,
            ..self.clone()
        }
    }
}

/// Result of editing a value
pub enum EditResult {
    /// No changes made
    Unchanged,
    /// Value was modified
    Changed,
}

impl EditResult {
    pub fn changed(&self) -> bool {
        matches!(self, EditResult::Changed)
    }
}

/// Draw a UI editor for any reflected value
pub fn reflect_editor(
    ui: &mut egui::Ui,
    value: &mut dyn PartialReflect,
    name: &str,
    config: &ReflectEditorConfig,
) -> EditResult {
    match value.reflect_mut() {
        ReflectMut::Struct(s) => struct_editor(ui, s, name, config),
        ReflectMut::Enum(e) => enum_editor(ui, e, name, config),
        ReflectMut::List(list) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
                ui.label(
                    egui::RichText::new(format!("[{}]", list.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
            });
            EditResult::Unchanged
        }
        ReflectMut::Array(arr) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
                ui.label(
                    egui::RichText::new(format!("[{}]", arr.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
            });
            EditResult::Unchanged
        }
        ReflectMut::Map(map) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
                ui.label(
                    egui::RichText::new(format!("{{{}}}", map.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
            });
            EditResult::Unchanged
        }
        ReflectMut::Set(set) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
                ui.label(
                    egui::RichText::new(format!("{{{}}}", set.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
            });
            EditResult::Unchanged
        }
        ReflectMut::TupleStruct(ts) => {
            let mut changed = false;
            egui::CollapsingHeader::new(egui::RichText::new(name).color(colors::TEXT_SECONDARY))
                .default_open(false)
                .show(ui, |ui| {
                    let nested_config = config.nested();
                    for i in 0..ts.field_len() {
                        if let Some(field) = ts.field_mut(i) {
                            let field_name = format!("{}", i);
                            if reflect_editor(ui, field, &field_name, &nested_config).changed() {
                                changed = true;
                            }
                        }
                    }
                });
            if changed {
                EditResult::Changed
            } else {
                EditResult::Unchanged
            }
        }
        ReflectMut::Tuple(t) => {
            let mut changed = false;
            egui::CollapsingHeader::new(egui::RichText::new(name).color(colors::TEXT_SECONDARY))
                .default_open(false)
                .show(ui, |ui| {
                    let nested_config = config.nested();
                    for i in 0..t.field_len() {
                        if let Some(field) = t.field_mut(i) {
                            let field_name = format!("{}", i);
                            if reflect_editor(ui, field, &field_name, &nested_config).changed() {
                                changed = true;
                            }
                        }
                    }
                });
            if changed {
                EditResult::Changed
            } else {
                EditResult::Unchanged
            }
        }
        ReflectMut::Opaque(_) => {
            // Handle primitive/opaque types
            opaque_editor(ui, value, name, config)
        }
    }
}

/// Draw editor for a struct
fn struct_editor(
    ui: &mut egui::Ui,
    s: &mut dyn Struct,
    name: &str,
    config: &ReflectEditorConfig,
) -> EditResult {
    let mut changed = false;

    // For common types like Transform, show inline
    let type_path = s
        .get_represented_type_info()
        .map(|ti| ti.type_path())
        .unwrap_or("");

    // Special handling for Transform
    if type_path == "bevy_transform::components::transform::Transform" {
        return transform_editor(ui, s, config);
    }

    // Special handling for Vec3
    if type_path == "glam::f32::vec3::Vec3" {
        return vec3_struct_editor(ui, s, name, config);
    }

    // Special handling for Quat
    if type_path == "glam::f32::sse2::quat::Quat" || type_path == "glam::f32::scalar::quat::Quat" {
        return quat_struct_editor(ui, name);
    }

    // Generic struct handling
    let field_count = s.field_len();
    if field_count == 0 {
        // Marker component - indent to align with collapsible headers
        ui.horizontal(|ui| {
            ui.add_space(COLLAPSE_ICON_WIDTH);
            ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
            ui.label(egui::RichText::new("(marker)").small().color(colors::TEXT_MUTED));
        });
        return EditResult::Unchanged;
    }

    egui::CollapsingHeader::new(egui::RichText::new(name).color(colors::TEXT_SECONDARY))
        .default_open(false)
        .show(ui, |ui| {
            let nested_config = config.nested();
            for i in 0..field_count {
                let field_name = s.name_at(i).unwrap_or("?").to_string();
                if let Some(field) = s.field_at_mut(i) {
                    if reflect_editor(ui, field, &field_name, &nested_config).changed() {
                        changed = true;
                    }
                }
            }
        });

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

/// Special editor for Transform component
fn transform_editor(
    ui: &mut egui::Ui,
    s: &mut dyn Struct,
    config: &ReflectEditorConfig,
) -> EditResult {
    let mut changed = false;

    egui::CollapsingHeader::new(
        egui::RichText::new("Transform")
            .strong()
            .color(colors::TEXT_PRIMARY),
    )
    .default_open(false)
    .show(ui, |ui| {
        ui.add_space(4.0);

        // Translation
        if let Some(translation) = s.field_mut("translation") {
            ui.label(egui::RichText::new("Translation").color(colors::TEXT_SECONDARY));
            if let Some(v) = translation.try_downcast_mut::<Vec3>() {
                if vec3_inline_editor(ui, v, config).changed() {
                    changed = true;
                }
            }
        }

        ui.add_space(4.0);

        // Rotation (as euler angles)
        if let Some(rotation) = s.field_mut("rotation") {
            ui.label(egui::RichText::new("Rotation").color(colors::TEXT_SECONDARY));
            if let Some(q) = rotation.try_downcast_mut::<Quat>() {
                if quat_euler_editor(ui, q).changed() {
                    changed = true;
                }
            }
        }

        ui.add_space(4.0);

        // Scale
        if let Some(scale) = s.field_mut("scale") {
            ui.label(egui::RichText::new("Scale").color(colors::TEXT_SECONDARY));
            if let Some(v) = scale.try_downcast_mut::<Vec3>() {
                if vec3_inline_editor(ui, v, config).changed() {
                    changed = true;
                }
            }
        }

        ui.add_space(4.0);
    });

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

/// Inline Vec3 editor with colored axis labels
fn vec3_inline_editor(ui: &mut egui::Ui, v: &mut Vec3, config: &ReflectEditorConfig) -> EditResult {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
        if ui
            .add(egui::DragValue::new(&mut v.x).speed(config.drag_speed))
            .changed()
        {
            changed = true;
        }
        ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
        if ui
            .add(egui::DragValue::new(&mut v.y).speed(config.drag_speed))
            .changed()
        {
            changed = true;
        }
        ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
        if ui
            .add(egui::DragValue::new(&mut v.z).speed(config.drag_speed))
            .changed()
        {
            changed = true;
        }
    });

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

/// Quat editor showing euler angles in degrees
fn quat_euler_editor(ui: &mut egui::Ui, q: &mut Quat) -> EditResult {
    let (mut yaw, mut pitch, mut roll) = q.to_euler(EulerRot::YXZ);
    yaw = yaw.to_degrees();
    pitch = pitch.to_degrees();
    roll = roll.to_degrees();

    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
        let x_changed = ui
            .add(egui::DragValue::new(&mut pitch).speed(1.0).suffix("°"))
            .changed();
        ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
        let y_changed = ui
            .add(egui::DragValue::new(&mut yaw).speed(1.0).suffix("°"))
            .changed();
        ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
        let z_changed = ui
            .add(egui::DragValue::new(&mut roll).speed(1.0).suffix("°"))
            .changed();

        if x_changed || y_changed || z_changed {
            *q = Quat::from_euler(
                EulerRot::YXZ,
                yaw.to_radians(),
                pitch.to_radians(),
                roll.to_radians(),
            );
            changed = true;
        }
    });

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

/// Vec3 struct editor (when Vec3 appears as a reflected struct)
fn vec3_struct_editor(
    ui: &mut egui::Ui,
    s: &mut dyn Struct,
    name: &str,
    config: &ReflectEditorConfig,
) -> EditResult {
    let mut changed = false;

    // Try to get x, y, z fields
    let x = s.field_mut("x").and_then(|f| f.try_downcast_mut::<f32>().map(|v| *v));
    let y = s.field_mut("y").and_then(|f| f.try_downcast_mut::<f32>().map(|v| *v));
    let z = s.field_mut("z").and_then(|f| f.try_downcast_mut::<f32>().map(|v| *v));

    if let (Some(mut x_val), Some(mut y_val), Some(mut z_val)) = (x, y, z) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
            ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
            if ui
                .add(egui::DragValue::new(&mut x_val).speed(config.drag_speed))
                .changed()
            {
                if let Some(f) = s.field_mut("x").and_then(|f| f.try_downcast_mut::<f32>()) {
                    *f = x_val;
                    changed = true;
                }
            }
            ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
            if ui
                .add(egui::DragValue::new(&mut y_val).speed(config.drag_speed))
                .changed()
            {
                if let Some(f) = s.field_mut("y").and_then(|f| f.try_downcast_mut::<f32>()) {
                    *f = y_val;
                    changed = true;
                }
            }
            ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
            if ui
                .add(egui::DragValue::new(&mut z_val).speed(config.drag_speed))
                .changed()
            {
                if let Some(f) = s.field_mut("z").and_then(|f| f.try_downcast_mut::<f32>()) {
                    *f = z_val;
                    changed = true;
                }
            }
        });
    }

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

/// Quat struct editor (when Quat appears as a reflected struct)
fn quat_struct_editor(ui: &mut egui::Ui, name: &str) -> EditResult {
    // For Quat, we need to work with the raw components
    // This is tricky because Quat's internal representation varies
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        ui.label(egui::RichText::new("(quaternion)").color(colors::TEXT_MUTED).small());
    });
    EditResult::Unchanged
}

/// Draw editor for an enum
fn enum_editor(
    ui: &mut egui::Ui,
    e: &mut dyn Enum,
    name: &str,
    config: &ReflectEditorConfig,
) -> EditResult {
    let type_info = e.get_represented_type_info();

    let Some(TypeInfo::Enum(enum_info)) = type_info else {
        ui.horizontal(|ui| {
            if config.is_top_level {
                ui.add_space(COLLAPSE_ICON_WIDTH);
            }
            ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
            ui.label(egui::RichText::new("(unknown enum)").color(colors::TEXT_MUTED));
        });
        return EditResult::Unchanged;
    };

    let current_variant = e.variant_name().to_string();
    let mut changed = false;

    ui.horizontal(|ui| {
        if config.is_top_level {
            ui.add_space(COLLAPSE_ICON_WIDTH);
        }
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));

        // Collect variant names
        let variants: Vec<&str> = enum_info
            .iter()
            .map(|v| v.name())
            .collect();

        egui::ComboBox::from_id_salt(format!("enum_{}", name))
            .selected_text(&current_variant)
            .show_ui(ui, |ui| {
                for variant_name in &variants {
                    if ui
                        .selectable_label(*variant_name == current_variant.as_str(), *variant_name)
                        .clicked()
                    {
                        // Try to change the variant
                        if *variant_name != current_variant.as_str() {
                            // Get variant info
                            if let Some(variant_info) = enum_info.variant(variant_name) {
                                match variant_info {
                                    VariantInfo::Unit(_) => {
                                        // Create a unit variant
                                        let dynamic = DynamicEnum::new(
                                            *variant_name,
                                            DynamicVariant::Unit,
                                        );
                                        e.apply(&dynamic);
                                        changed = true;
                                    }
                                    VariantInfo::Struct(_) | VariantInfo::Tuple(_) => {
                                        // For now, skip complex variants
                                        // Would need to construct default values
                                    }
                                }
                            }
                        }
                    }
                }
            });
    });

    // If enum has fields, show them
    if e.field_len() > 0 {
        ui.indent(format!("enum_fields_{}", name), |ui| {
            let nested_config = config.nested();
            for i in 0..e.field_len() {
                let field_name = e.name_at(i).map(|s| s.to_string()).unwrap_or_else(|| format!("{}", i));
                if let Some(field) = e.field_at_mut(i) {
                    if reflect_editor(ui, field, &field_name, &nested_config).changed() {
                        changed = true;
                    }
                }
            }
        });
    }

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

/// Draw editor for opaque/primitive types
fn opaque_editor(
    ui: &mut egui::Ui,
    value: &mut dyn PartialReflect,
    name: &str,
    config: &ReflectEditorConfig,
) -> EditResult {
    // Add indentation for top-level components to align with collapsible headers
    if config.is_top_level {
        ui.horizontal(|ui| {
            ui.add_space(COLLAPSE_ICON_WIDTH);
            opaque_editor_inner(ui, value, name, config);
        });
        // For simplicity, we don't track changes in top-level horizontal wrapper
        return EditResult::Unchanged;
    }

    opaque_editor_inner(ui, value, name, config)
}

/// Inner implementation of opaque editor (without indentation wrapper)
fn opaque_editor_inner(
    ui: &mut egui::Ui,
    value: &mut dyn PartialReflect,
    name: &str,
    config: &ReflectEditorConfig,
) -> EditResult {
    // f32
    if let Some(v) = value.try_downcast_mut::<f32>() {
        return f32_editor(ui, v, name, config);
    }

    // f64
    if let Some(v) = value.try_downcast_mut::<f64>() {
        return f64_editor(ui, v, name, config);
    }

    // i32
    if let Some(v) = value.try_downcast_mut::<i32>() {
        return i32_editor(ui, v, name);
    }

    // i64
    if let Some(v) = value.try_downcast_mut::<i64>() {
        return i64_editor(ui, v, name);
    }

    // u32
    if let Some(v) = value.try_downcast_mut::<u32>() {
        return u32_editor(ui, v, name);
    }

    // u64
    if let Some(v) = value.try_downcast_mut::<u64>() {
        return u64_editor(ui, v, name);
    }

    // usize
    if let Some(v) = value.try_downcast_mut::<usize>() {
        return usize_editor(ui, v, name);
    }

    // bool
    if let Some(v) = value.try_downcast_mut::<bool>() {
        return bool_editor(ui, v, name);
    }

    // String
    if let Some(v) = value.try_downcast_mut::<String>() {
        return string_editor(ui, v, name);
    }

    // Vec3
    if let Some(v) = value.try_downcast_mut::<Vec3>() {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        return vec3_inline_editor(ui, v, config);
    }

    // Vec2
    if let Some(v) = value.try_downcast_mut::<Vec2>() {
        return vec2_editor(ui, v, name, config);
    }

    // Quat
    if let Some(v) = value.try_downcast_mut::<Quat>() {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        return quat_euler_editor(ui, v);
    }

    // Color (Srgba)
    if let Some(v) = value.try_downcast_mut::<bevy::color::Srgba>() {
        return srgba_editor(ui, v, name);
    }

    // Color enum
    if let Some(v) = value.try_downcast_mut::<Color>() {
        return color_editor(ui, v, name);
    }

    // Entity - show as read-only
    if let Some(v) = value.try_downcast_ref::<Entity>() {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        ui.label(
            egui::RichText::new(format!("{:?}", v))
                .color(colors::TEXT_MUTED)
                .small(),
        );
        return EditResult::Unchanged;
    }

    // Unknown type - show type name
    let type_path = value
        .get_represented_type_info()
        .map(|ti| ti.type_path())
        .unwrap_or("unknown");

    ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
    ui.label(
        egui::RichText::new(format!("({})", short_type_name(type_path)))
            .color(colors::TEXT_MUTED)
            .small(),
    );

    EditResult::Unchanged
}

fn f32_editor(ui: &mut egui::Ui, v: &mut f32, name: &str, config: &ReflectEditorConfig) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui
            .add(egui::DragValue::new(v).speed(config.drag_speed))
            .changed()
        {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn f64_editor(ui: &mut egui::Ui, v: &mut f64, name: &str, config: &ReflectEditorConfig) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui
            .add(egui::DragValue::new(v).speed(config.drag_speed as f64))
            .changed()
        {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn i32_editor(ui: &mut egui::Ui, v: &mut i32, name: &str) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.add(egui::DragValue::new(v).speed(1.0)).changed() {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn i64_editor(ui: &mut egui::Ui, v: &mut i64, name: &str) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.add(egui::DragValue::new(v).speed(1.0)).changed() {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn u32_editor(ui: &mut egui::Ui, v: &mut u32, name: &str) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.add(egui::DragValue::new(v).speed(1.0)).changed() {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn u64_editor(ui: &mut egui::Ui, v: &mut u64, name: &str) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.add(egui::DragValue::new(v).speed(1.0)).changed() {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn usize_editor(ui: &mut egui::Ui, v: &mut usize, name: &str) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.add(egui::DragValue::new(v).speed(1.0)).changed() {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn bool_editor(ui: &mut egui::Ui, v: &mut bool, name: &str) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.checkbox(v, "").changed() {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn string_editor(ui: &mut egui::Ui, v: &mut String, name: &str) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.text_edit_singleline(v).changed() {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn vec2_editor(ui: &mut egui::Ui, v: &mut Vec2, name: &str, config: &ReflectEditorConfig) -> EditResult {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
        if ui
            .add(egui::DragValue::new(&mut v.x).speed(config.drag_speed))
            .changed()
        {
            changed = true;
        }
        ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
        if ui
            .add(egui::DragValue::new(&mut v.y).speed(config.drag_speed))
            .changed()
        {
            changed = true;
        }
    });
    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn srgba_editor(ui: &mut egui::Ui, v: &mut bevy::color::Srgba, name: &str) -> EditResult {
    let mut changed = false;
    let mut color = [v.red, v.green, v.blue];

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.color_edit_button_rgb(&mut color).changed() {
            v.red = color[0];
            v.green = color[1];
            v.blue = color[2];
            changed = true;
        }
    });

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

fn color_editor(ui: &mut egui::Ui, v: &mut Color, name: &str) -> EditResult {
    let mut changed = false;
    let srgba = v.to_srgba();
    let mut color = [srgba.red, srgba.green, srgba.blue];

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).color(colors::TEXT_SECONDARY));
        if ui.color_edit_button_rgb(&mut color).changed() {
            *v = Color::srgb(color[0], color[1], color[2]);
            changed = true;
        }
    });

    if changed {
        EditResult::Changed
    } else {
        EditResult::Unchanged
    }
}

/// Get a short type name from a full type path
fn short_type_name(type_path: &str) -> &str {
    type_path.rsplit("::").next().unwrap_or(type_path)
}

/// Draw an editor for a component on an entity
/// Falls back to read-only display if the component cannot be mutably accessed
pub fn component_editor(
    world: &mut World,
    entity: Entity,
    type_id: TypeId,
    ui: &mut egui::Ui,
    config: &ReflectEditorConfig,
) -> EditResult {
    // Check if we already know this type is immutable
    let is_known_immutable = world
        .get_resource::<ImmutableComponentCache>()
        .map(|cache| cache.immutable_types.contains(&type_id))
        .unwrap_or(false);

    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();

    let Some(registration) = type_registry.get(type_id) else {
        return EditResult::Unchanged;
    };

    let Some(reflect_component) = registration.data::<ReflectComponent>() else {
        return EditResult::Unchanged;
    };

    let short_name = registration
        .type_info()
        .type_path_table()
        .short_path()
        .to_string();

    // First, get a read-only reference to check if the component exists
    let reflect_ref = reflect_component.reflect(world.entity(entity));
    if reflect_ref.is_none() {
        drop(type_registry);
        return EditResult::Unchanged;
    }

    drop(type_registry);

    // If known immutable, go straight to read-only display
    if is_known_immutable {
        return show_readonly_component(world, entity, type_id, &short_name, ui, config);
    }

    // Check if this component type supports mutable reflection
    // by checking if it would panic (immutable components will panic on reflect_mut)
    let is_mutable = is_component_mutable(world, entity, type_id);

    if !is_mutable {
        // Cache this type as immutable so we don't check again
        if let Some(mut cache) = world.get_resource_mut::<ImmutableComponentCache>() {
            cache.immutable_types.insert(type_id);
        }
        return show_readonly_component(world, entity, type_id, &short_name, ui, config);
    }

    // Safe to get mutable reference
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();
    let reflect_component = type_registry
        .get(type_id)
        .and_then(|r| r.data::<ReflectComponent>());

    if let Some(reflect_component) = reflect_component {
        if let Some(mut reflect_mut) = reflect_component.reflect_mut(world.entity_mut(entity)) {
            drop(type_registry);
            return reflect_editor(ui, &mut *reflect_mut, &short_name, config);
        }
    }

    drop(type_registry);
    EditResult::Unchanged
}

/// Show a component in read-only mode
fn show_readonly_component(
    world: &mut World,
    entity: Entity,
    type_id: TypeId,
    short_name: &str,
    ui: &mut egui::Ui,
    config: &ReflectEditorConfig,
) -> EditResult {
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();
    let reflect_component = type_registry
        .get(type_id)
        .and_then(|r| r.data::<ReflectComponent>());

    if let Some(reflect_component) = reflect_component {
        if let Some(reflect_ref) = reflect_component.reflect(world.entity(entity)) {
            drop(type_registry);
            reflect_viewer(ui, &*reflect_ref, short_name, config);
            return EditResult::Unchanged;
        }
    }

    drop(type_registry);
    EditResult::Unchanged
}

/// Check if a component supports mutable reflection
/// This uses catch_unwind to safely detect immutable components
fn is_component_mutable(world: &mut World, entity: Entity, type_id: TypeId) -> bool {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();

    let Some(registration) = type_registry.get(type_id) else {
        return false;
    };

    let Some(reflect_component) = registration.data::<ReflectComponent>().cloned() else {
        return false;
    };

    drop(type_registry);

    // We need to use AssertUnwindSafe because World doesn't implement UnwindSafe
    // This is safe because we're not going to continue using the world if a panic occurs
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Try to get mutable access - this will panic for immutable components
        let _ = reflect_component.reflect_mut(world.entity_mut(entity));
    }));

    result.is_ok()
}

/// Draw a read-only viewer for any reflected value
pub fn reflect_viewer(
    ui: &mut egui::Ui,
    value: &dyn PartialReflect,
    name: &str,
    config: &ReflectEditorConfig,
) {
    use bevy::reflect::ReflectRef;

    match value.reflect_ref() {
        ReflectRef::Struct(s) => struct_viewer(ui, s, name, config),
        ReflectRef::Enum(e) => enum_viewer(ui, e, name, config),
        ReflectRef::List(list) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_MUTED));
                ui.label(
                    egui::RichText::new(format!("[{}]", list.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
                readonly_badge(ui);
            });
        }
        ReflectRef::Array(arr) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_MUTED));
                ui.label(
                    egui::RichText::new(format!("[{}]", arr.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
                readonly_badge(ui);
            });
        }
        ReflectRef::Map(map) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_MUTED));
                ui.label(
                    egui::RichText::new(format!("{{{}}}", map.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
                readonly_badge(ui);
            });
        }
        ReflectRef::Set(set) => {
            ui.horizontal(|ui| {
                if config.is_top_level {
                    ui.add_space(COLLAPSE_ICON_WIDTH);
                }
                ui.label(egui::RichText::new(name).color(colors::TEXT_MUTED));
                ui.label(
                    egui::RichText::new(format!("{{{}}}", set.len()))
                        .color(colors::TEXT_MUTED)
                        .small(),
                );
                readonly_badge(ui);
            });
        }
        ReflectRef::TupleStruct(ts) => {
            egui::CollapsingHeader::new(egui::RichText::new(name).color(colors::TEXT_MUTED))
                .default_open(false)
                .show(ui, |ui| {
                    readonly_badge(ui);
                    let nested_config = config.nested();
                    for i in 0..ts.field_len() {
                        if let Some(field) = ts.field(i) {
                            let field_name = format!("{}", i);
                            reflect_viewer(ui, field, &field_name, &nested_config);
                        }
                    }
                });
        }
        ReflectRef::Tuple(t) => {
            egui::CollapsingHeader::new(egui::RichText::new(name).color(colors::TEXT_MUTED))
                .default_open(false)
                .show(ui, |ui| {
                    readonly_badge(ui);
                    let nested_config = config.nested();
                    for i in 0..t.field_len() {
                        if let Some(field) = t.field(i) {
                            let field_name = format!("{}", i);
                            reflect_viewer(ui, field, &field_name, &nested_config);
                        }
                    }
                });
        }
        ReflectRef::Opaque(_) => {
            opaque_viewer(ui, value, name, config);
        }
    }
}

/// Draw a read-only badge to indicate the component is not editable
fn readonly_badge(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("(read-only)")
            .small()
            .color(colors::ACCENT_ORANGE),
    );
}

/// Read-only struct viewer
fn struct_viewer(
    ui: &mut egui::Ui,
    s: &dyn Struct,
    name: &str,
    config: &ReflectEditorConfig,
) {
    let type_path = s
        .get_represented_type_info()
        .map(|ti| ti.type_path())
        .unwrap_or("");

    // Special handling for Transform (read-only)
    if type_path == "bevy_transform::components::transform::Transform" {
        transform_viewer(ui, s, config);
        return;
    }

    let field_count = s.field_len();
    if field_count == 0 {
        ui.horizontal(|ui| {
            if config.is_top_level {
                ui.add_space(COLLAPSE_ICON_WIDTH);
            }
            ui.label(egui::RichText::new(name).color(colors::TEXT_MUTED));
            ui.label(egui::RichText::new("(marker)").small().color(colors::TEXT_MUTED));
            readonly_badge(ui);
        });
        return;
    }

    egui::CollapsingHeader::new(egui::RichText::new(name).color(colors::TEXT_MUTED))
        .default_open(false)
        .show(ui, |ui| {
            readonly_badge(ui);
            let nested_config = config.nested();
            for i in 0..field_count {
                let field_name = s.name_at(i).unwrap_or("?").to_string();
                if let Some(field) = s.field_at(i) {
                    reflect_viewer(ui, field, &field_name, &nested_config);
                }
            }
        });
}

/// Read-only Transform viewer
fn transform_viewer(
    ui: &mut egui::Ui,
    s: &dyn Struct,
    _config: &ReflectEditorConfig,
) {

    egui::CollapsingHeader::new(egui::RichText::new("Transform").color(colors::TEXT_MUTED))
        .default_open(false)
        .show(ui, |ui| {
            readonly_badge(ui);
            ui.add_space(4.0);

            // Translation
            if let Some(translation) = s.field("translation") {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Translation").color(colors::TEXT_MUTED));
                    if let Some(v) = translation.try_downcast_ref::<Vec3>() {
                        ui.label(format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z));
                    }
                });
            }

            // Rotation
            if let Some(rotation) = s.field("rotation") {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Rotation").color(colors::TEXT_MUTED));
                    if let Some(q) = rotation.try_downcast_ref::<Quat>() {
                        let (yaw, pitch, roll) = q.to_euler(EulerRot::YXZ);
                        ui.label(format!(
                            "({:.1}, {:.1}, {:.1})",
                            pitch.to_degrees(),
                            yaw.to_degrees(),
                            roll.to_degrees()
                        ));
                    }
                });
            }

            // Scale
            if let Some(scale) = s.field("scale") {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Scale").color(colors::TEXT_MUTED));
                    if let Some(v) = scale.try_downcast_ref::<Vec3>() {
                        ui.label(format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z));
                    }
                });
            }

            ui.add_space(4.0);
        });
}

/// Read-only enum viewer
fn enum_viewer(ui: &mut egui::Ui, e: &dyn Enum, name: &str, config: &ReflectEditorConfig) {
    ui.horizontal(|ui| {
        if config.is_top_level {
            ui.add_space(COLLAPSE_ICON_WIDTH);
        }
        ui.label(egui::RichText::new(name).color(colors::TEXT_MUTED));
        ui.label(egui::RichText::new(e.variant_name()).color(colors::TEXT_MUTED));
        readonly_badge(ui);
    });
}

/// Read-only opaque/primitive viewer
fn opaque_viewer(ui: &mut egui::Ui, value: &dyn PartialReflect, name: &str, config: &ReflectEditorConfig) {
    ui.horizontal(|ui| {
        if config.is_top_level {
            ui.add_space(COLLAPSE_ICON_WIDTH);
        }
        ui.label(egui::RichText::new(name).color(colors::TEXT_MUTED));

        // Try to display common types
        if let Some(v) = value.try_downcast_ref::<f32>() {
            ui.label(format!("{:.3}", v));
        } else if let Some(v) = value.try_downcast_ref::<f64>() {
            ui.label(format!("{:.3}", v));
        } else if let Some(v) = value.try_downcast_ref::<i32>() {
            ui.label(format!("{}", v));
        } else if let Some(v) = value.try_downcast_ref::<i64>() {
            ui.label(format!("{}", v));
        } else if let Some(v) = value.try_downcast_ref::<u32>() {
            ui.label(format!("{}", v));
        } else if let Some(v) = value.try_downcast_ref::<u64>() {
            ui.label(format!("{}", v));
        } else if let Some(v) = value.try_downcast_ref::<usize>() {
            ui.label(format!("{}", v));
        } else if let Some(v) = value.try_downcast_ref::<bool>() {
            ui.label(if *v { "true" } else { "false" });
        } else if let Some(v) = value.try_downcast_ref::<String>() {
            ui.label(format!("\"{}\"", v));
        } else if let Some(v) = value.try_downcast_ref::<Vec3>() {
            ui.label(format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z));
        } else if let Some(v) = value.try_downcast_ref::<Vec2>() {
            ui.label(format!("({:.2}, {:.2})", v.x, v.y));
        } else if let Some(v) = value.try_downcast_ref::<Quat>() {
            let (yaw, pitch, roll) = v.to_euler(EulerRot::YXZ);
            ui.label(format!(
                "({:.1}, {:.1}, {:.1})",
                pitch.to_degrees(),
                yaw.to_degrees(),
                roll.to_degrees()
            ));
        } else if let Some(v) = value.try_downcast_ref::<Entity>() {
            ui.label(format!("{:?}", v));
        } else {
            // Unknown type
            let type_path = value
                .get_represented_type_info()
                .map(|ti| ti.type_path())
                .unwrap_or("unknown");
            ui.label(
                egui::RichText::new(format!("({})", short_type_name(type_path)))
                    .color(colors::TEXT_MUTED)
                    .small(),
            );
        }

        readonly_badge(ui);
    });
}
