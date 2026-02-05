use bevy_egui::egui;
use bevy_grid_shader::{GridAxes, GridMaterial, GridMaterialUniform};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ui::theme::{grid_label, section_header, value_slider, DRAG_VALUE_WIDTH};

use super::EditorMaterialDef;

/// Serializable properties for the grid material extension.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GridMaterialProps {
    pub line_color: [f32; 4],
    pub major_line_color: [f32; 4],
    pub line_width: f32,
    pub major_line_width: f32,
    pub grid_scale: f32,
    pub major_line_every: u32,
    pub axes_xz: bool,
    pub axes_xy: bool,
    pub axes_yz: bool,
    pub fade_distance: f32,
    pub fade_strength: f32,
}

impl Default for GridMaterialProps {
    fn default() -> Self {
        let def = GridMaterial::default();
        Self {
            line_color: [
                def.uniform.line_color.red,
                def.uniform.line_color.green,
                def.uniform.line_color.blue,
                def.uniform.line_color.alpha,
            ],
            major_line_color: [
                def.uniform.major_line_color.red,
                def.uniform.major_line_color.green,
                def.uniform.major_line_color.blue,
                def.uniform.major_line_color.alpha,
            ],
            line_width: def.uniform.line_width,
            major_line_width: def.uniform.major_line_width,
            grid_scale: def.uniform.grid_scale,
            major_line_every: def.uniform.major_line_every,
            axes_xz: (def.uniform.axes & GridAxes::XZ.bits()) != 0,
            axes_xy: (def.uniform.axes & GridAxes::XY.bits()) != 0,
            axes_yz: (def.uniform.axes & GridAxes::YZ.bits()) != 0,
            fade_distance: def.uniform.fade_distance,
            fade_strength: def.uniform.fade_strength,
        }
    }
}

/// Grid material definition for the editor material registry.
pub struct GridMaterialDef;

impl EditorMaterialDef for GridMaterialDef {
    type Props = GridMaterialProps;
    type Extension = GridMaterial;

    const TYPE_NAME: &'static str = "grid";
    const DISPLAY_NAME: &'static str = "Grid";

    fn to_extension(props: &GridMaterialProps) -> GridMaterial {
        let mut axes = 0u32;
        if props.axes_xz {
            axes |= GridAxes::XZ.bits();
        }
        if props.axes_xy {
            axes |= GridAxes::XY.bits();
        }
        if props.axes_yz {
            axes |= GridAxes::YZ.bits();
        }

        GridMaterial {
            uniform: GridMaterialUniform {
                line_color: LinearRgba::new(
                    props.line_color[0],
                    props.line_color[1],
                    props.line_color[2],
                    props.line_color[3],
                ),
                major_line_color: LinearRgba::new(
                    props.major_line_color[0],
                    props.major_line_color[1],
                    props.major_line_color[2],
                    props.major_line_color[3],
                ),
                line_width: props.line_width,
                major_line_width: props.major_line_width,
                grid_scale: props.grid_scale,
                major_line_every: props.major_line_every,
                axes,
                fade_distance: props.fade_distance,
                fade_strength: props.fade_strength,
                _padding: 0.0,
            },
        }
    }

    fn from_extension(ext: &GridMaterial) -> GridMaterialProps {
        GridMaterialProps {
            line_color: [
                ext.uniform.line_color.red,
                ext.uniform.line_color.green,
                ext.uniform.line_color.blue,
                ext.uniform.line_color.alpha,
            ],
            major_line_color: [
                ext.uniform.major_line_color.red,
                ext.uniform.major_line_color.green,
                ext.uniform.major_line_color.blue,
                ext.uniform.major_line_color.alpha,
            ],
            line_width: ext.uniform.line_width,
            major_line_width: ext.uniform.major_line_width,
            grid_scale: ext.uniform.grid_scale,
            major_line_every: ext.uniform.major_line_every,
            axes_xz: (ext.uniform.axes & GridAxes::XZ.bits()) != 0,
            axes_xy: (ext.uniform.axes & GridAxes::XY.bits()) != 0,
            axes_yz: (ext.uniform.axes & GridAxes::YZ.bits()) != 0,
            fade_distance: ext.uniform.fade_distance,
            fade_strength: ext.uniform.fade_strength,
        }
    }

    fn draw_ui(ui: &mut egui::Ui, props: &mut GridMaterialProps) -> bool {
        let mut changed = false;

        section_header(ui, "Grid Properties", true, |ui| {
            egui::Grid::new("grid_props_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Line Color");
                    changed |= ui
                        .color_edit_button_rgba_unmultiplied(&mut props.line_color)
                        .changed();
                    ui.end_row();

                    grid_label(ui, "Major Color");
                    changed |= ui
                        .color_edit_button_rgba_unmultiplied(&mut props.major_line_color)
                        .changed();
                    ui.end_row();

                    grid_label(ui, "Line Width");
                    changed |= value_slider(ui, &mut props.line_width, 0.1..=10.0);
                    ui.end_row();

                    grid_label(ui, "Major Width");
                    changed |= value_slider(ui, &mut props.major_line_width, 0.1..=10.0);
                    ui.end_row();

                    grid_label(ui, "Scale");
                    changed |= value_slider(ui, &mut props.grid_scale, 0.1..=100.0);
                    ui.end_row();

                    grid_label(ui, "Major Every");
                    let mut major = props.major_line_every as i32;
                    if ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut major).range(1..=100),
                        )
                        .changed()
                    {
                        props.major_line_every = major.max(1) as u32;
                        changed = true;
                    }
                    ui.end_row();

                    grid_label(ui, "Axes");
                    ui.horizontal(|ui| {
                        changed |= ui.checkbox(&mut props.axes_xz, "XZ").changed();
                        changed |= ui.checkbox(&mut props.axes_xy, "XY").changed();
                        changed |= ui.checkbox(&mut props.axes_yz, "YZ").changed();
                    });
                    ui.end_row();

                    grid_label(ui, "Fade Dist");
                    changed |= value_slider(ui, &mut props.fade_distance, 0.0..=1000.0);
                    ui.end_row();

                    grid_label(ui, "Fade Strength");
                    changed |= value_slider(ui, &mut props.fade_strength, 0.0..=10.0);
                    ui.end_row();
                });
        });

        changed
    }
}
