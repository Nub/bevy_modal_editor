use bevy::{
    asset::embedded_asset,
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin, StandardMaterial},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use bevy_egui::egui;
use bevy_modal_editor::materials::EditorMaterialDef;
use bevy_modal_editor::ui::theme::colors;
use serde::{Deserialize, Serialize};

/// Uniform data sent to the GPU for the checkerboard shader.
#[derive(Clone, Copy, ShaderType, Debug)]
pub struct CheckerboardUniform {
    pub color_a: LinearRgba,
    pub color_b: LinearRgba,
    pub scale: f32,
    pub _padding: Vec3,
}

/// A material extension that renders a world-space checkerboard pattern.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct CheckerboardMaterial {
    #[uniform(100)]
    pub uniform: CheckerboardUniform,
}

impl Default for CheckerboardMaterial {
    fn default() -> Self {
        Self {
            uniform: CheckerboardUniform {
                color_a: LinearRgba::new(0.9, 0.9, 0.9, 1.0),
                color_b: LinearRgba::new(0.3, 0.3, 0.3, 1.0),
                scale: 2.0,
                _padding: Vec3::ZERO,
            },
        }
    }
}

impl MaterialExtension for CheckerboardMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://marble_demo/checkerboard.wgsl".into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        "embedded://marble_demo/checkerboard.wgsl".into()
    }
}

/// Plugin that registers the checkerboard material with Bevy's rendering system.
pub struct CheckerboardMaterialPlugin;

impl Plugin for CheckerboardMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "checkerboard.wgsl");
        app.add_plugins(
            MaterialPlugin::<ExtendedMaterial<StandardMaterial, CheckerboardMaterial>>::default(),
        );
    }
}

// ---------------------------------------------------------------------------
// Editor integration via EditorMaterialDef
// ---------------------------------------------------------------------------

/// Serializable properties for the checkerboard material extension.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CheckerboardMaterialProps {
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    pub scale: f32,
}

impl Default for CheckerboardMaterialProps {
    fn default() -> Self {
        let def = CheckerboardMaterial::default();
        Self {
            color_a: [
                def.uniform.color_a.red,
                def.uniform.color_a.green,
                def.uniform.color_a.blue,
                def.uniform.color_a.alpha,
            ],
            color_b: [
                def.uniform.color_b.red,
                def.uniform.color_b.green,
                def.uniform.color_b.blue,
                def.uniform.color_b.alpha,
            ],
            scale: def.uniform.scale,
        }
    }
}

/// Checkerboard material definition for the editor material registry.
pub struct CheckerboardMaterialDef;

impl EditorMaterialDef for CheckerboardMaterialDef {
    type Props = CheckerboardMaterialProps;
    type Extension = CheckerboardMaterial;

    const TYPE_NAME: &'static str = "checkerboard";
    const DISPLAY_NAME: &'static str = "Checkerboard";

    fn to_extension(props: &CheckerboardMaterialProps) -> CheckerboardMaterial {
        CheckerboardMaterial {
            uniform: CheckerboardUniform {
                color_a: LinearRgba::new(
                    props.color_a[0],
                    props.color_a[1],
                    props.color_a[2],
                    props.color_a[3],
                ),
                color_b: LinearRgba::new(
                    props.color_b[0],
                    props.color_b[1],
                    props.color_b[2],
                    props.color_b[3],
                ),
                scale: props.scale,
                _padding: Vec3::ZERO,
            },
        }
    }

    fn from_extension(ext: &CheckerboardMaterial) -> CheckerboardMaterialProps {
        CheckerboardMaterialProps {
            color_a: [
                ext.uniform.color_a.red,
                ext.uniform.color_a.green,
                ext.uniform.color_a.blue,
                ext.uniform.color_a.alpha,
            ],
            color_b: [
                ext.uniform.color_b.red,
                ext.uniform.color_b.green,
                ext.uniform.color_b.blue,
                ext.uniform.color_b.alpha,
            ],
            scale: ext.uniform.scale,
        }
    }

    fn draw_ui(ui: &mut egui::Ui, props: &mut CheckerboardMaterialProps) -> bool {
        let mut changed = false;

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Checkerboard Properties")
                .color(colors::ACCENT_CYAN)
                .strong(),
        );
        ui.add_space(4.0);

        // Color A
        ui.label(egui::RichText::new("Color A").color(colors::TEXT_SECONDARY));
        ui.horizontal(|ui| {
            changed |= ui
                .color_edit_button_rgba_unmultiplied(&mut props.color_a)
                .changed();
        });
        ui.add_space(4.0);

        // Color B
        ui.label(egui::RichText::new("Color B").color(colors::TEXT_SECONDARY));
        ui.horizontal(|ui| {
            changed |= ui
                .color_edit_button_rgba_unmultiplied(&mut props.color_b)
                .changed();
        });
        ui.add_space(4.0);

        // Scale
        ui.label(egui::RichText::new("Scale").color(colors::TEXT_SECONDARY));
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut props.scale).speed(0.1).range(0.1..=100.0))
                .changed();
        });
        ui.add_space(4.0);

        changed
    }
}
