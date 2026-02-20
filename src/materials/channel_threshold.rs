use bevy::prelude::*;
use bevy_channel_mat::{ChannelThresholdMaterial, ChannelThresholdUniform};
use bevy_egui::egui;
use serde::{Deserialize, Serialize};

use crate::ui::theme::{grid_label, section_header, value_slider, DRAG_VALUE_WIDTH};

use super::EditorMaterialDef;

/// Serializable properties for the channel threshold material extension.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChannelThresholdProps {
    pub channel: u32,
    pub threshold: f32,
    pub smoothing: f32,
    pub invert: bool,
}

impl Default for ChannelThresholdProps {
    fn default() -> Self {
        let def = ChannelThresholdMaterial::default();
        Self {
            channel: def.uniform.channel,
            threshold: def.uniform.threshold,
            smoothing: def.uniform.smoothing,
            invert: def.uniform.invert != 0,
        }
    }
}

/// Channel threshold material definition for the editor material registry.
pub struct ChannelThresholdDef;

impl EditorMaterialDef for ChannelThresholdDef {
    type Props = ChannelThresholdProps;
    type Extension = ChannelThresholdMaterial;

    const TYPE_NAME: &'static str = "channel_threshold";
    const DISPLAY_NAME: &'static str = "Channel Threshold";

    fn to_extension(props: &ChannelThresholdProps) -> ChannelThresholdMaterial {
        ChannelThresholdMaterial {
            uniform: ChannelThresholdUniform {
                channel: props.channel,
                threshold: props.threshold,
                smoothing: props.smoothing,
                invert: if props.invert { 1 } else { 0 },
            },
        }
    }

    fn from_extension(ext: &ChannelThresholdMaterial) -> ChannelThresholdProps {
        ChannelThresholdProps {
            channel: ext.uniform.channel,
            threshold: ext.uniform.threshold,
            smoothing: ext.uniform.smoothing,
            invert: ext.uniform.invert != 0,
        }
    }

    fn adjust_base(base: &mut StandardMaterial, _props: &ChannelThresholdProps) {
        base.alpha_mode = AlphaMode::Blend;
    }

    fn draw_ui(ui: &mut egui::Ui, props: &mut ChannelThresholdProps) -> bool {
        let mut changed = false;

        section_header(ui, "Channel Threshold", true, |ui| {
            egui::Grid::new("channel_threshold_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    grid_label(ui, "Channel");
                    let prev = props.channel;
                    egui::ComboBox::from_id_salt("channel_select")
                        .width(DRAG_VALUE_WIDTH)
                        .selected_text(match props.channel {
                            0 => "Red",
                            1 => "Green",
                            2 => "Blue",
                            _ => "Red",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut props.channel, 0, "Red");
                            ui.selectable_value(&mut props.channel, 1, "Green");
                            ui.selectable_value(&mut props.channel, 2, "Blue");
                        });
                    changed |= props.channel != prev;
                    ui.end_row();

                    grid_label(ui, "Threshold");
                    changed |= value_slider(ui, &mut props.threshold, 0.0..=1.0);
                    ui.end_row();

                    grid_label(ui, "Smoothing");
                    changed |= value_slider(ui, &mut props.smoothing, 0.0..=0.5);
                    ui.end_row();

                    grid_label(ui, "Invert");
                    changed |= ui.checkbox(&mut props.invert, "").changed();
                    ui.end_row();
                });
        });

        changed
    }
}
