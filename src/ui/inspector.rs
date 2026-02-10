use avian3d::prelude::RigidBody;
use bevy::light::VolumetricLight;
use bevy::prelude::*;
use bevy::reflect::TypeInfo;
use bevy_egui::{egui, EguiPrimaryContextPass};
use bevy_procedural::{PlacementOrientation, ProceduralPlacer, SamplingMode};
use bevy_spline_3d::path_follow::{FollowerState, LoopMode, SplineFollower};
use std::any::TypeId;

use bevy_editor_game::{CustomEntityRegistry, InspectorWidgetFn, SceneComponentRegistry};

use super::command_palette::{open_add_component_palette, CommandPaletteState, TexturePickResult, TextureSlot, draw_entity_field, make_callback_id, PendingEntitySelection};
use super::reflect_editor::{clear_focus_state, component_editor, ReflectEditorConfig};
use super::InspectorPanelState;
use crate::commands::TakeSnapshotCommand;
use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::scene::{
    blockout::{ArchMarker, LShapeMarker, RampMarker, StairsMarker},
    DecalMarker, DecalType, DirectionalLightMarker, FogVolumeMarker, Locked, SceneLightMarker,
};
use crate::selection::Selected;
use crate::ui::theme::{colors, draw_pin_button, grid_label, panel, panel_frame, section_header, value_slider, DRAG_VALUE_WIDTH};

/// Represents the RigidBody type for UI selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RigidBodyType {
    Static,
    Dynamic,
    Kinematic,
}

impl RigidBodyType {
    fn from_rigid_body(rb: &RigidBody) -> Self {
        match rb {
            RigidBody::Static => RigidBodyType::Static,
            RigidBody::Dynamic => RigidBodyType::Dynamic,
            RigidBody::Kinematic => RigidBodyType::Kinematic,
        }
    }

    fn to_rigid_body(self) -> RigidBody {
        match self {
            RigidBodyType::Static => RigidBody::Static,
            RigidBodyType::Dynamic => RigidBody::Dynamic,
            RigidBodyType::Kinematic => RigidBody::Kinematic,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            RigidBodyType::Static => "Static",
            RigidBodyType::Dynamic => "Dynamic",
            RigidBodyType::Kinematic => "Kinematic",
        }
    }

    const ALL: [RigidBodyType; 3] = [
        RigidBodyType::Static,
        RigidBodyType::Dynamic,
        RigidBodyType::Kinematic,
    ];
}

/// State for the component editor popup
#[derive(Resource, Default)]
pub struct ComponentEditorState {
    /// The component type ID being edited (if any)
    pub editing_component: Option<(std::any::TypeId, String)>,
    /// Whether the popup was just opened (to focus first field)
    pub just_opened: bool,
}

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentEditorState>()
            .add_systems(EguiPrimaryContextPass, (draw_inspector_panel, draw_component_editor_popup));
    }
}

/// Result of drawing a removable component section
enum ComponentAction<T> {
    None,
    Update(T),
    Remove,
}

/// Data for point light editing
#[derive(Clone)]
struct PointLightData {
    color: [f32; 3],
    intensity: f32,
    range: f32,
    radius: f32,
    shadows_enabled: bool,
    volumetric: bool,
}

impl From<&SceneLightMarker> for PointLightData {
    fn from(marker: &SceneLightMarker) -> Self {
        let color = marker.color.to_srgba();
        Self {
            color: [color.red, color.green, color.blue],
            intensity: marker.intensity,
            range: marker.range,
            radius: marker.radius,
            shadows_enabled: marker.shadows_enabled,
            volumetric: false, // Set separately based on VolumetricLight component
        }
    }
}

/// Data for directional light editing
#[derive(Clone)]
struct DirectionalLightData {
    color: [f32; 3],
    illuminance: f32,
    shadows_enabled: bool,
    volumetric: bool,
}

impl From<&DirectionalLightMarker> for DirectionalLightData {
    fn from(marker: &DirectionalLightMarker) -> Self {
        let color = marker.color.to_srgba();
        Self {
            color: [color.red, color.green, color.blue],
            illuminance: marker.illuminance,
            shadows_enabled: marker.shadows_enabled,
            volumetric: false, // Set separately based on VolumetricLight component
        }
    }
}

/// Data for fog volume editing
#[derive(Clone)]
struct FogVolumeData {
    fog_color: [f32; 3],
    density_factor: f32,
    absorption: f32,
    scattering: f32,
    scattering_asymmetry: f32,
    light_tint: [f32; 3],
    light_intensity: f32,
}

impl From<&FogVolumeMarker> for FogVolumeData {
    fn from(marker: &FogVolumeMarker) -> Self {
        let fog_color = marker.fog_color.to_srgba();
        let light_tint = marker.light_tint.to_srgba();
        Self {
            fog_color: [fog_color.red, fog_color.green, fog_color.blue],
            density_factor: marker.density_factor,
            absorption: marker.absorption,
            scattering: marker.scattering,
            scattering_asymmetry: marker.scattering_asymmetry,
            light_tint: [light_tint.red, light_tint.green, light_tint.blue],
            light_intensity: marker.light_intensity,
        }
    }
}

/// Data for stairs editing
struct StairsData {
    step_count: u32,
    height: f32,
    depth: f32,
    width: f32,
}

impl From<&StairsMarker> for StairsData {
    fn from(marker: &StairsMarker) -> Self {
        Self {
            step_count: marker.step_count,
            height: marker.height,
            depth: marker.depth,
            width: marker.width,
        }
    }
}

/// Data for ramp editing
struct RampData {
    height: f32,
    length: f32,
    width: f32,
}

impl From<&RampMarker> for RampData {
    fn from(marker: &RampMarker) -> Self {
        Self {
            height: marker.height,
            length: marker.length,
            width: marker.width,
        }
    }
}

/// Data for arch editing
struct ArchData {
    opening_width: f32,
    opening_height: f32,
    thickness: f32,
    wall_width: f32,
    wall_height: f32,
    arch_segments: u32,
}

impl From<&ArchMarker> for ArchData {
    fn from(marker: &ArchMarker) -> Self {
        Self {
            opening_width: marker.opening_width,
            opening_height: marker.opening_height,
            thickness: marker.thickness,
            wall_width: marker.wall_width,
            wall_height: marker.wall_height,
            arch_segments: marker.arch_segments,
        }
    }
}

/// Data for L-shape editing
struct LShapeData {
    arm1_length: f32,
    arm2_length: f32,
    arm_width: f32,
    height: f32,
}

impl From<&LShapeMarker> for LShapeData {
    fn from(marker: &LShapeMarker) -> Self {
        Self {
            arm1_length: marker.arm1_length,
            arm2_length: marker.arm2_length,
            arm_width: marker.arm_width,
            height: marker.height,
        }
    }
}

/// Data for decal editing
struct DecalData {
    base_color_path: Option<String>,
    normal_map_path: Option<String>,
    emissive_path: Option<String>,
    decal_type: DecalType,
    depth_fade_factor: f32,
}

impl From<&DecalMarker> for DecalData {
    fn from(marker: &DecalMarker) -> Self {
        Self {
            base_color_path: marker.base_color_path.clone(),
            normal_map_path: marker.normal_map_path.clone(),
            emissive_path: marker.emissive_path.clone(),
            decal_type: marker.decal_type,
            depth_fade_factor: marker.depth_fade_factor,
        }
    }
}

/// Result from drawing decal texture slot UI
struct DecalTextureResult {
    changed: bool,
    browse_requested: Option<TextureSlot>,
}

/// Data for SplineFollower editing
#[derive(Clone)]
struct SplineFollowerData {
    spline: Entity,
    speed: f32,
    t: f32,
    loop_mode: LoopMode,
    state: FollowerState,
    align_to_tangent: bool,
    up_vector: [f32; 3],
    direction: f32,
    offset: [f32; 3],
    constant_speed: bool,
}

impl From<&SplineFollower> for SplineFollowerData {
    fn from(follower: &SplineFollower) -> Self {
        Self {
            spline: follower.spline,
            speed: follower.speed,
            t: follower.t,
            loop_mode: follower.loop_mode,
            state: follower.state,
            align_to_tangent: follower.align_to_tangent,
            up_vector: [follower.up_vector.x, follower.up_vector.y, follower.up_vector.z],
            direction: follower.direction,
            offset: [follower.offset.x, follower.offset.y, follower.offset.z],
            constant_speed: follower.constant_speed,
        }
    }
}

/// Template data for UI editing
#[derive(Clone)]
struct TemplateData {
    entity: Entity,
    weight: f32,
    name: Option<String>,
}

/// Data for ProceduralPlacer editing
#[derive(Clone)]
struct ProceduralPlacerData {
    // Templates
    templates: Vec<TemplateData>,
    // Sampling
    mode: usize, // 0=Uniform, 1=Random
    count: usize,
    seed: Option<u64>,
    // Placement
    orientation: usize, // 0=Identity, 1=AlignToTangent, 2=AlignToSurface, 3=RandomYaw, 4=RandomFull
    up_vector: [f32; 3],
    offset: [f32; 3],
    // Projection config
    projection_enabled: bool,
    projection_direction: [f32; 3],
    projection_local_space: bool,
    projection_ray_offset: f32,
    projection_max_distance: f32,
    use_bounds_offset: bool,
    enabled: bool,
}

impl ProceduralPlacerData {
    fn from_placer(p: &ProceduralPlacer, world: &World) -> Self {
        let (mode, seed) = match &p.mode {
            SamplingMode::Uniform => (0, None),
            SamplingMode::Random { seed } => (1, *seed),
        };
        let (orientation, up_vector) = match &p.orientation {
            PlacementOrientation::Identity => (0, [0.0, 1.0, 0.0]),
            PlacementOrientation::AlignToTangent { up } => (1, [up.x, up.y, up.z]),
            PlacementOrientation::AlignToSurface => (2, [0.0, 1.0, 0.0]),
            PlacementOrientation::RandomYaw => (3, [0.0, 1.0, 0.0]),
            PlacementOrientation::RandomFull => (4, [0.0, 1.0, 0.0]),
        };
        let templates = p.templates.iter().map(|t| {
            TemplateData {
                entity: t.entity,
                weight: t.weight,
                name: world.get::<Name>(t.entity).map(|n| n.as_str().to_string()),
            }
        }).collect();
        Self {
            templates,
            mode,
            count: p.count,
            seed,
            orientation,
            up_vector,
            offset: [p.offset.x, p.offset.y, p.offset.z],
            projection_enabled: p.projection.enabled,
            projection_direction: [p.projection.direction.x, p.projection.direction.y, p.projection.direction.z],
            projection_local_space: p.projection.local_space,
            projection_ray_offset: p.projection.ray_origin_offset,
            projection_max_distance: p.projection.max_distance,
            use_bounds_offset: p.use_bounds_offset,
            enabled: p.enabled,
        }
    }
}

/// Helper: draw an X/Y/Z inline row of DragValues.
fn xyz_row(ui: &mut egui::Ui, values: &mut [f32; 3], speed: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
        changed |= ui
            .add_sized(
                [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut values[0]).speed(speed).min_decimals(2),
            )
            .changed();
        ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
        changed |= ui
            .add_sized(
                [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut values[1]).speed(speed).min_decimals(2),
            )
            .changed();
        ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
        changed |= ui
            .add_sized(
                [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut values[2]).speed(speed).min_decimals(2),
            )
            .changed();
    });
    changed
}

/// Draw a transform section with colored X/Y/Z labels
fn draw_transform_section(ui: &mut egui::Ui, transform: &mut Transform) -> bool {
    let mut changed = false;

    section_header(ui, "Transform", true, |ui| {
        egui::Grid::new("transform_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                // Translation
                grid_label(ui, "Translation");
                let mut pos = transform.translation.to_array();
                if xyz_row(ui, &mut pos, 0.1) {
                    transform.translation = Vec3::from(pos);
                    changed = true;
                }
                ui.end_row();

                // Rotation (as euler angles in degrees)
                let (mut yaw, mut pitch, mut roll) = transform.rotation.to_euler(EulerRot::YXZ);
                yaw = yaw.to_degrees();
                pitch = pitch.to_degrees();
                roll = roll.to_degrees();

                grid_label(ui, "Rotation");
                let mut rot_changed = false;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
                    rot_changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut pitch).speed(1.0).suffix("°").min_decimals(1),
                        )
                        .changed();
                    ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
                    rot_changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut yaw).speed(1.0).suffix("°").min_decimals(1),
                        )
                        .changed();
                    ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
                    rot_changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut roll).speed(1.0).suffix("°").min_decimals(1),
                        )
                        .changed();
                });
                if rot_changed {
                    transform.rotation = Quat::from_euler(
                        EulerRot::YXZ,
                        yaw.to_radians(),
                        pitch.to_radians(),
                        roll.to_radians(),
                    );
                    changed = true;
                }
                ui.end_row();

                // Scale
                grid_label(ui, "Scale");
                let mut scl = transform.scale.to_array();
                if xyz_row(ui, &mut scl, 0.01) {
                    transform.scale = Vec3::from(scl);
                    changed = true;
                }
                ui.end_row();
            });
    });

    changed
}

/// Draw point light properties section
fn draw_point_light_section(ui: &mut egui::Ui, data: &mut PointLightData) -> bool {
    let mut changed = false;

    section_header(ui, "Point Light", true, |ui| {
        egui::Grid::new("point_light_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Color");
                changed |= ui.color_edit_button_rgb(&mut data.color).changed();
                ui.end_row();

                grid_label(ui, "Intensity");
                changed |= value_slider(ui, &mut data.intensity, 0.0..=1000000.0);
                ui.end_row();

                grid_label(ui, "Range");
                changed |= value_slider(ui, &mut data.range, 0.0..=1000.0);
                ui.end_row();

                grid_label(ui, "Radius");
                changed |= value_slider(ui, &mut data.radius, 0.0..=10.0);
                ui.end_row();

                grid_label(ui, "Shadows");
                changed |= ui.checkbox(&mut data.shadows_enabled, "").changed();
                ui.end_row();

                grid_label(ui, "Volumetric");
                changed |= ui.checkbox(&mut data.volumetric, "").changed();
                ui.end_row();
            });
    });

    changed
}

/// Draw directional light properties section
fn draw_directional_light_section(ui: &mut egui::Ui, data: &mut DirectionalLightData) -> bool {
    let mut changed = false;

    section_header(ui, "Directional Light", true, |ui| {
        egui::Grid::new("directional_light_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Color");
                changed |= ui.color_edit_button_rgb(&mut data.color).changed();
                ui.end_row();

                grid_label(ui, "Illuminance");
                changed |= value_slider(ui, &mut data.illuminance, 0.0..=200000.0);
                ui.end_row();

                grid_label(ui, "Shadows");
                changed |= ui.checkbox(&mut data.shadows_enabled, "").changed();
                ui.end_row();

                grid_label(ui, "Volumetric");
                changed |= ui.checkbox(&mut data.volumetric, "").changed();
                ui.end_row();
            });
    });

    changed
}

/// Draw fog volume properties section
fn draw_fog_volume_section(ui: &mut egui::Ui, data: &mut FogVolumeData) -> bool {
    let mut changed = false;

    section_header(ui, "Fog Volume", true, |ui| {
        egui::Grid::new("fog_volume_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Fog Color");
                changed |= ui.color_edit_button_rgb(&mut data.fog_color).changed();
                ui.end_row();

                grid_label(ui, "Density");
                changed |= value_slider(ui, &mut data.density_factor, 0.0..=1.0);
                ui.end_row();

                grid_label(ui, "Absorption");
                changed |= value_slider(ui, &mut data.absorption, 0.0..=1.0);
                ui.end_row();

                grid_label(ui, "Scattering");
                changed |= value_slider(ui, &mut data.scattering, 0.0..=1.0);
                ui.end_row();

                grid_label(ui, "Asymmetry");
                changed |= value_slider(ui, &mut data.scattering_asymmetry, -1.0..=1.0);
                ui.end_row();

                grid_label(ui, "Light Tint");
                changed |= ui.color_edit_button_rgb(&mut data.light_tint).changed();
                ui.end_row();

                grid_label(ui, "Light Intensity");
                changed |= value_slider(ui, &mut data.light_intensity, 0.0..=10.0);
                ui.end_row();
            });
    });

    changed
}

/// Draw stairs properties section
fn draw_stairs_section(ui: &mut egui::Ui, data: &mut StairsData) -> bool {
    let mut changed = false;

    section_header(ui, "Stairs", true, |ui| {
        egui::Grid::new("stairs_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Steps");
                let mut steps = data.step_count as i32;
                if ui
                    .add_sized(
                        [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut steps).range(1..=50),
                    )
                    .changed()
                {
                    data.step_count = steps.max(1) as u32;
                    changed = true;
                }
                ui.end_row();

                grid_label(ui, "Height");
                changed |= value_slider(ui, &mut data.height, 0.1..=50.0);
                ui.end_row();

                grid_label(ui, "Depth");
                changed |= value_slider(ui, &mut data.depth, 0.1..=50.0);
                ui.end_row();

                grid_label(ui, "Width");
                changed |= value_slider(ui, &mut data.width, 0.1..=50.0);
                ui.end_row();
            });
    });

    changed
}

/// Draw ramp properties section
fn draw_ramp_section(ui: &mut egui::Ui, data: &mut RampData) -> bool {
    let mut changed = false;

    section_header(ui, "Ramp", true, |ui| {
        egui::Grid::new("ramp_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Height");
                changed |= value_slider(ui, &mut data.height, 0.1..=50.0);
                ui.end_row();

                grid_label(ui, "Length");
                changed |= value_slider(ui, &mut data.length, 0.1..=50.0);
                ui.end_row();

                grid_label(ui, "Width");
                changed |= value_slider(ui, &mut data.width, 0.1..=50.0);
                ui.end_row();
            });
    });

    changed
}

/// Draw arch properties section
fn draw_arch_section(ui: &mut egui::Ui, data: &mut ArchData) -> bool {
    let mut changed = false;

    section_header(ui, "Arch", true, |ui| {
        egui::Grid::new("arch_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Opening W");
                changed |= value_slider(ui, &mut data.opening_width, 0.1..=20.0);
                ui.end_row();

                grid_label(ui, "Opening H");
                changed |= value_slider(ui, &mut data.opening_height, 0.1..=20.0);
                ui.end_row();

                grid_label(ui, "Thickness");
                changed |= value_slider(ui, &mut data.thickness, 0.1..=10.0);
                ui.end_row();

                grid_label(ui, "Wall Width");
                changed |= value_slider(ui, &mut data.wall_width, 0.1..=20.0);
                ui.end_row();

                grid_label(ui, "Wall Height");
                changed |= value_slider(ui, &mut data.wall_height, 0.1..=20.0);
                ui.end_row();

                grid_label(ui, "Segments");
                let mut segments = data.arch_segments as i32;
                if ui
                    .add_sized(
                        [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut segments).range(4..=32),
                    )
                    .changed()
                {
                    data.arch_segments = segments.max(4) as u32;
                    changed = true;
                }
                ui.end_row();
            });
    });

    changed
}

/// Draw L-shape properties section
fn draw_lshape_section(ui: &mut egui::Ui, data: &mut LShapeData) -> bool {
    let mut changed = false;

    section_header(ui, "L-Shape", true, |ui| {
        egui::Grid::new("lshape_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                grid_label(ui, "Arm 1 Len");
                changed |= value_slider(ui, &mut data.arm1_length, 0.1..=50.0);
                ui.end_row();

                grid_label(ui, "Arm 2 Len");
                changed |= value_slider(ui, &mut data.arm2_length, 0.1..=50.0);
                ui.end_row();

                grid_label(ui, "Arm Width");
                changed |= value_slider(ui, &mut data.arm_width, 0.1..=20.0);
                ui.end_row();

                grid_label(ui, "Height");
                changed |= value_slider(ui, &mut data.height, 0.1..=50.0);
                ui.end_row();
            });
    });

    changed
}

/// Draw a single decal texture row (label + filename + Browse + Clear)
fn draw_decal_texture_row(
    ui: &mut egui::Ui,
    label: &str,
    slot: TextureSlot,
    path: &mut Option<String>,
    result: &mut DecalTextureResult,
) {
    grid_label(ui, label);
    ui.horizontal(|ui| {
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
    ui.end_row();
}

/// Draw decal properties section with texture selectors
fn draw_decal_section(ui: &mut egui::Ui, data: &mut DecalData) -> DecalTextureResult {
    let mut result = DecalTextureResult {
        changed: false,
        browse_requested: None,
    };

    section_header(ui, "Decal", true, |ui| {
        egui::Grid::new("decal_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                // Decal type selector
                grid_label(ui, "Type");
                let mut type_idx: usize = match data.decal_type {
                    DecalType::Clustered => 0,
                    DecalType::Forward => 1,
                };
                let prev_idx = type_idx;
                egui::ComboBox::from_id_salt("decal_type")
                    .selected_text(match data.decal_type {
                        DecalType::Clustered => "Clustered",
                        DecalType::Forward => "Forward",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut type_idx, 0, "Clustered");
                        ui.selectable_value(&mut type_idx, 1, "Forward");
                    });
                if type_idx != prev_idx {
                    data.decal_type = match type_idx {
                        1 => DecalType::Forward,
                        _ => DecalType::Clustered,
                    };
                    result.changed = true;
                }
                ui.end_row();

                // Depth fade factor (Forward only)
                if data.decal_type == DecalType::Forward {
                    grid_label(ui, "Depth Fade");
                    let prev = data.depth_fade_factor;
                    ui.add(
                        egui::DragValue::new(&mut data.depth_fade_factor)
                            .speed(0.1)
                            .range(0.01..=100.0)
                            .suffix(" m"),
                    );
                    if data.depth_fade_factor != prev {
                        result.changed = true;
                    }
                    ui.end_row();
                }

                draw_decal_texture_row(ui, "Base Color", TextureSlot::DecalBaseColor, &mut data.base_color_path, &mut result);
                draw_decal_texture_row(ui, "Normal Map", TextureSlot::DecalNormalMap, &mut data.normal_map_path, &mut result);
                draw_decal_texture_row(ui, "Emissive", TextureSlot::DecalEmissive, &mut data.emissive_path, &mut result);
            });
    });

    result
}

/// Draw SplineFollower properties section
/// Result from drawing spline follower section
struct SplineFollowerResult {
    changed: bool,
    open_spline_picker: bool,
}

fn draw_spline_follower_section(
    ui: &mut egui::Ui,
    data: &mut SplineFollowerData,
    spline_name: Option<&str>,
) -> SplineFollowerResult {
    let mut result = SplineFollowerResult {
        changed: false,
        open_spline_picker: false,
    };

    section_header(ui, "Spline Follower", true, |ui| {
        egui::Grid::new("spline_follower_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                // Spline entity reference
                grid_label(ui, "Spline");
                result.open_spline_picker = draw_entity_field(ui, "", data.spline, spline_name);
                ui.end_row();

                // Speed
                grid_label(ui, "Speed");
                result.changed |= value_slider(ui, &mut data.speed, 0.0..=100.0);
                ui.end_row();

                // Position on spline (t)
                grid_label(ui, "Position (t)");
                result.changed |= value_slider(ui, &mut data.t, 0.0..=1.0);
                ui.end_row();

                // Loop mode
                grid_label(ui, "Loop Mode");
                egui::ComboBox::from_id_salt("loop_mode")
                    .selected_text(match data.loop_mode {
                        LoopMode::Once => "Once",
                        LoopMode::Loop => "Loop",
                        LoopMode::PingPong => "Ping Pong",
                    })
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut data.loop_mode, LoopMode::Once, "Once").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.loop_mode, LoopMode::Loop, "Loop").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.loop_mode, LoopMode::PingPong, "Ping Pong").clicked() {
                            result.changed = true;
                        }
                    });
                ui.end_row();

                // State
                grid_label(ui, "State");
                egui::ComboBox::from_id_salt("follower_state")
                    .selected_text(match data.state {
                        FollowerState::Playing => "Playing",
                        FollowerState::Paused => "Paused",
                        FollowerState::Finished => "Finished",
                    })
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut data.state, FollowerState::Playing, "Playing").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.state, FollowerState::Paused, "Paused").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.state, FollowerState::Finished, "Finished").clicked() {
                            result.changed = true;
                        }
                    });
                ui.end_row();

                // Direction
                grid_label(ui, "Direction");
                ui.horizontal(|ui| {
                    if ui.selectable_label(data.direction >= 0.0, "Forward").clicked() {
                        data.direction = 1.0;
                        result.changed = true;
                    }
                    if ui.selectable_label(data.direction < 0.0, "Backward").clicked() {
                        data.direction = -1.0;
                        result.changed = true;
                    }
                });
                ui.end_row();

                // Align to tangent
                grid_label(ui, "Align");
                result.changed |= ui.checkbox(&mut data.align_to_tangent, "To tangent").changed();
                ui.end_row();

                // Constant speed
                grid_label(ui, "Const Speed");
                result.changed |= ui.checkbox(&mut data.constant_speed, "").changed();
                ui.end_row();

                // Up vector (only show if align_to_tangent is true)
                if data.align_to_tangent {
                    grid_label(ui, "Up Vector");
                    result.changed |= xyz_row(ui, &mut data.up_vector, 0.01);
                    ui.end_row();
                }

                // Offset
                grid_label(ui, "Offset");
                result.changed |= xyz_row(ui, &mut data.offset, 0.1);
                ui.end_row();
            });
    });

    result
}

/// Result from drawing the ProceduralPlacer section
struct ProceduralPlacerResult {
    changed: bool,
    open_template_picker: bool,
    remove_template_index: Option<usize>,
}

/// Draw a ProceduralPlacer section
fn draw_procedural_placer_section(ui: &mut egui::Ui, data: &mut ProceduralPlacerData) -> ProceduralPlacerResult {
    let mut result = ProceduralPlacerResult {
        changed: false,
        open_template_picker: false,
        remove_template_index: None,
    };

    section_header(ui, "Procedural Placer", true, |ui| {
        // Templates section
        ui.label(egui::RichText::new("Templates").color(colors::TEXT_SECONDARY).small());
        ui.add_space(4.0);

        if data.templates.is_empty() {
            ui.label(egui::RichText::new("No templates configured").color(colors::TEXT_MUTED).italics());
        } else {
            for (i, template) in data.templates.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    // Template name/entity
                    let name = template.name.as_deref().unwrap_or("Unnamed");
                    ui.label(egui::RichText::new(name).color(colors::TEXT_PRIMARY));

                    // Weight slider
                    ui.add_sized(
                        [60.0, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut template.weight)
                            .speed(0.1)
                            .range(0.0..=100.0)
                            .prefix("w: "),
                    );
                    if ui.small_button("×").clicked() {
                        result.remove_template_index = Some(i);
                        result.changed = true;
                    }
                });
            }
        }

        if ui.small_button("+ Add Template").clicked() {
            result.open_template_picker = true;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        egui::Grid::new("procedural_placer_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                // Sampling mode
                grid_label(ui, "Mode");
                egui::ComboBox::from_id_salt("placer_mode")
                    .selected_text(match data.mode {
                        0 => "Uniform",
                        _ => "Random",
                    })
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut data.mode, 0, "Uniform").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.mode, 1, "Random").clicked() {
                            result.changed = true;
                        }
                    });
                ui.end_row();

                // Count
                grid_label(ui, "Count");
                let mut count_i32 = data.count as i32;
                if ui
                    .add_sized(
                        [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut count_i32).speed(1).range(1..=10000),
                    )
                    .changed()
                {
                    data.count = count_i32.max(1) as usize;
                    result.changed = true;
                }
                ui.end_row();

                // Seed (only for random mode)
                if data.mode == 1 {
                    grid_label(ui, "Seed");
                    let mut has_seed = data.seed.is_some();
                    let mut seed_val = data.seed.unwrap_or(0) as i64;
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut has_seed, "").changed() {
                            data.seed = if has_seed { Some(seed_val as u64) } else { None };
                            result.changed = true;
                        }
                        if has_seed {
                            if ui
                                .add_sized(
                                    [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                                    egui::DragValue::new(&mut seed_val).speed(1),
                                )
                                .changed()
                            {
                                data.seed = Some(seed_val as u64);
                                result.changed = true;
                            }
                        }
                    });
                    ui.end_row();
                }

                ui.separator();
                ui.end_row();

                // Orientation
                grid_label(ui, "Orientation");
                egui::ComboBox::from_id_salt("placer_orientation")
                    .selected_text(match data.orientation {
                        0 => "Identity",
                        1 => "Align to Tangent",
                        2 => "Align to Surface",
                        3 => "Random Yaw",
                        _ => "Random Full",
                    })
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut data.orientation, 0, "Identity").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.orientation, 1, "Align to Tangent").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.orientation, 2, "Align to Surface").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.orientation, 3, "Random Yaw").clicked() {
                            result.changed = true;
                        }
                        if ui.selectable_value(&mut data.orientation, 4, "Random Full").clicked() {
                            result.changed = true;
                        }
                    });
                ui.end_row();

                // Up vector (only for AlignToTangent)
                if data.orientation == 1 {
                    grid_label(ui, "Up Vector");
                    result.changed |= xyz_row(ui, &mut data.up_vector, 0.01);
                    ui.end_row();
                }

                // Offset
                grid_label(ui, "Offset");
                result.changed |= xyz_row(ui, &mut data.offset, 0.1);
                ui.end_row();

                // Projection
                grid_label(ui, "Projection");
                result.changed |= ui.checkbox(&mut data.projection_enabled, "").changed();
                ui.end_row();

                // Projection settings (only if projection enabled)
                if data.projection_enabled {
                    grid_label(ui, "Direction");
                    result.changed |= xyz_row(ui, &mut data.projection_direction, 0.1);
                    ui.end_row();

                    grid_label(ui, "Local Space");
                    result.changed |= ui.checkbox(&mut data.projection_local_space, "").changed();
                    ui.end_row();

                    grid_label(ui, "Ray Offset");
                    result.changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut data.projection_ray_offset)
                                .speed(0.1)
                                .range(0.0..=1000.0),
                        )
                        .changed();
                    ui.end_row();

                    grid_label(ui, "Max Distance");
                    result.changed |= ui
                        .add_sized(
                            [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut data.projection_max_distance)
                                .speed(1.0)
                                .range(0.0..=10000.0),
                        )
                        .changed();
                    ui.end_row();

                    grid_label(ui, "Bounds Offset");
                    result.changed |= ui.checkbox(&mut data.use_bounds_offset, "").changed();
                    ui.end_row();
                }

                // Enabled
                grid_label(ui, "Enabled");
                result.changed |= ui.checkbox(&mut data.enabled, "").changed();
                ui.end_row();
            });
    });

    result
}

/// Draw a RigidBody type selector with remove button
/// current_type is None if entities have mixed types
fn draw_rigidbody_section(ui: &mut egui::Ui, current_type: Option<RigidBodyType>) -> ComponentAction<RigidBodyType> {
    let mut action = ComponentAction::None;

    ui.horizontal(|ui| {
        ui.collapsing(
            egui::RichText::new("Physics").strong().color(colors::TEXT_PRIMARY),
            |ui| {
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Body Type").color(colors::TEXT_SECONDARY));
                });

                let display_text = current_type.map(|t| t.label()).unwrap_or("Mixed");

                let mut new_type = None;
                egui::ComboBox::from_id_salt("rigidbody_type")
                    .selected_text(display_text)
                    .show_ui(ui, |ui| {
                        for rb_type in RigidBodyType::ALL {
                            if ui.selectable_value(&mut new_type, Some(rb_type), rb_type.label()).clicked() {
                                // Only set if different from current
                                if current_type != Some(rb_type) {
                                    new_type = Some(rb_type);
                                } else {
                                    new_type = None;
                                }
                            }
                        }
                    });

                if let Some(t) = new_type {
                    action = ComponentAction::Update(t);
                }

                ui.add_space(4.0);
            },
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("X")
                .on_hover_text("Remove Physics component")
                .clicked()
            {
                action = ComponentAction::Remove;
            }
        });
    });

    action
}

/// Draw the component inspector panel
fn draw_inspector_panel(world: &mut World) {
    // Don't draw UI when editor is disabled
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    // Show inspector in ObjectInspector mode, or when pinned
    let current_mode = *world.resource::<State<EditorMode>>().get();
    let is_pinned = world.resource::<PinnedWindows>().0.contains(&EditorMode::ObjectInspector);
    if current_mode != EditorMode::ObjectInspector && !is_pinned {
        return;
    }

    // Query for all selected entities
    let selected_entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).collect()
    };

    let selection_count = selected_entities.len();
    let single_entity = if selection_count == 1 { Some(selected_entities[0]) } else { None };

    // Get entity name and transform for single selection
    let mut entity_name = single_entity.and_then(|e| {
        world.get::<Name>(e).map(|n| n.as_str().to_string())
    });
    let mut transform_copy = single_entity.and_then(|e| world.get::<Transform>(e).copied());
    let original_name = entity_name.clone();

    // Get locked state for single selection
    let mut is_locked = single_entity.map(|e| world.get::<Locked>(e).is_some()).unwrap_or(false);
    let original_locked = is_locked;

    // Get RigidBody types for all selected entities that have one
    let rigidbody_types: Vec<(Entity, RigidBodyType)> = selected_entities
        .iter()
        .filter_map(|&e| {
            world.get::<RigidBody>(e).map(|rb| (e, RigidBodyType::from_rigid_body(rb)))
        })
        .collect();

    // Determine if all have same type or mixed
    let common_rigidbody_type: Option<RigidBodyType> = if rigidbody_types.is_empty() {
        None
    } else {
        let first_type = rigidbody_types[0].1;
        if rigidbody_types.iter().all(|(_, t)| *t == first_type) {
            Some(first_type)
        } else {
            None // Mixed types
        }
    };
    let has_rigidbodies = !rigidbody_types.is_empty();

    // Get point light data for single selection
    let mut point_light_data = single_entity.and_then(|e| {
        world.get::<SceneLightMarker>(e).map(|m| {
            let mut data = PointLightData::from(m);
            data.volumetric = world.get::<VolumetricLight>(e).is_some();
            data
        })
    });

    // Get directional light data for single selection
    let mut directional_light_data = single_entity.and_then(|e| {
        world.get::<DirectionalLightMarker>(e).map(|m| {
            let mut data = DirectionalLightData::from(m);
            data.volumetric = world.get::<VolumetricLight>(e).is_some();
            data
        })
    });

    // Get fog volume data for single selection
    let mut fog_volume_data = single_entity.and_then(|e| {
        world.get::<FogVolumeMarker>(e).map(|m| FogVolumeData::from(m))
    });

    // Get decal data for single selection
    let mut decal_data = single_entity.and_then(|e| {
        world.get::<DecalMarker>(e).map(|m| DecalData::from(m))
    });

    // Check for texture pick result targeting decal slots
    if let (Some(entity), Some(data)) = (single_entity, &mut decal_data) {
        let pick_data = world.resource_mut::<TexturePickResult>().0.take();
        if let Some(pick) = pick_data {
            if pick.entity == Some(entity) {
                let applied = match pick.slot {
                    TextureSlot::DecalBaseColor => {
                        data.base_color_path = Some(pick.path.clone());
                        true
                    }
                    TextureSlot::DecalNormalMap => {
                        data.normal_map_path = Some(pick.path.clone());
                        true
                    }
                    TextureSlot::DecalEmissive => {
                        data.emissive_path = Some(pick.path.clone());
                        true
                    }
                    _ => false,
                };
                if applied {
                    // Apply immediately to marker so sync_decal_markers picks it up
                    if let Some(mut marker) = world.get_mut::<DecalMarker>(entity) {
                        marker.base_color_path = data.base_color_path.clone();
                        marker.normal_map_path = data.normal_map_path.clone();
                        marker.emissive_path = data.emissive_path.clone();
                    }
                } else {
                    // Not a decal slot — put it back for other editors
                    world.resource_mut::<TexturePickResult>().0 = Some(pick);
                }
            } else {
                // Different entity — put it back
                world.resource_mut::<TexturePickResult>().0 = Some(pick);
            }
        }
    }

    // Get blockout shape data for single selection
    let mut stairs_data = single_entity.and_then(|e| {
        world.get::<StairsMarker>(e).map(|m| StairsData::from(m))
    });
    let mut ramp_data = single_entity.and_then(|e| {
        world.get::<RampMarker>(e).map(|m| RampData::from(m))
    });
    let mut arch_data = single_entity.and_then(|e| {
        world.get::<ArchMarker>(e).map(|m| ArchData::from(m))
    });
    let mut lshape_data = single_entity.and_then(|e| {
        world.get::<LShapeMarker>(e).map(|m| LShapeData::from(m))
    });

    // Get spline follower data for single selection
    let mut spline_follower_data = single_entity.and_then(|e| {
        world.get::<SplineFollower>(e).map(|f| SplineFollowerData::from(f))
    });

    // Get the spline entity's name (for display in the picker)
    let spline_name: Option<String> = spline_follower_data.as_ref().and_then(|data| {
        world.get::<Name>(data.spline).map(|n| n.as_str().to_string())
    });

    // Get procedural placer component data for single selection
    let mut procedural_placer_data = single_entity.and_then(|e| {
        world.get::<ProceduralPlacer>(e).map(|p| ProceduralPlacerData::from_placer(p, world))
    });

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

    let mut transform_changed = false;
    let mut rigidbody_action: ComponentAction<RigidBodyType> = ComponentAction::None;
    let mut point_light_changed = false;
    let mut directional_light_changed = false;
    let mut fog_volume_changed = false;
    let mut decal_changed = false;
    let mut decal_browse_requested: Option<TextureSlot> = None;
    let mut stairs_changed = false;
    let mut ramp_changed = false;
    let mut arch_changed = false;
    let mut lshape_changed = false;
    let mut spline_follower_changed = false;
    let mut open_spline_picker = false;
    let mut custom_inspector_changed = false;

    // Procedural placer change tracking
    let mut procedural_placer_changed = false;
    let mut open_placer_template_picker = false;
    let mut remove_placer_template_index: Option<usize> = None;

    // Check for "N" key to focus name field (only for single selection)
    let focus_name_field = selection_count == 1
        && !ctx.wants_keyboard_input()
        && ctx.input(|i| i.key_pressed(egui::Key::N));
    let name_field_id = egui::Id::new("inspector_name_field");

    // Calculate available height using shared panel settings
    let available_height = panel::available_height(&ctx);

    // If pinned and the active mode also uses the right side, move to the left
    let displaced = is_pinned
        && current_mode != EditorMode::ObjectInspector
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    let mut pin_toggled = false;

    let panel_response = egui::Window::new("Inspector")
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .min_height(available_height)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            // Pin button (right-aligned)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                pin_toggled = draw_pin_button(ui, is_pinned);
            });

            match selection_count {
                0 => {
                    // No selection
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("No entity selected")
                                .color(colors::TEXT_MUTED)
                                .italics(),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Click an entity in the viewport\nor hierarchy to select it.")
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                    });
                }
                1 => {
                    // Single selection - full inspector
                    let entity = single_entity.unwrap();
                    ui.add_space(4.0);

                    // Editable entity name
                    if let Some(ref mut name) = entity_name {
                        let response = ui.add(
                            egui::TextEdit::singleline(name)
                                .id(name_field_id)
                                .font(egui::FontId::proportional(16.0))
                                .text_color(colors::TEXT_PRIMARY)
                                .margin(egui::vec2(8.0, 6.0)),
                        );
                        if focus_name_field {
                            response.request_focus();
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(format!("Entity {:?}", entity))
                                .strong()
                                .size(14.0)
                                .color(colors::TEXT_PRIMARY),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("ID: {:?}", entity))
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.checkbox(&mut is_locked, "Locked");
                        });
                    });

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // Show locked message if entity is locked
                        if is_locked {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Entity is locked")
                                        .color(colors::TEXT_MUTED)
                                        .italics(),
                                );
                            });
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Unlock to edit properties")
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }

                        // Custom Transform section with colored labels (disabled if locked)
                        if !is_locked {
                            if let Some(ref mut transform) = transform_copy {
                                transform_changed = draw_transform_section(ui, transform);
                            }

                            ui.add_space(4.0);

                            // RigidBody type selector (only if entity has RigidBody)
                            if has_rigidbodies {
                                rigidbody_action = draw_rigidbody_section(ui, common_rigidbody_type);
                                ui.add_space(4.0);
                            }

                            // Point light properties
                            if let Some(ref mut data) = point_light_data {
                                point_light_changed = draw_point_light_section(ui, data);
                                ui.add_space(4.0);
                            }

                            // Directional light properties
                            if let Some(ref mut data) = directional_light_data {
                                directional_light_changed = draw_directional_light_section(ui, data);
                                ui.add_space(4.0);
                            }

                            // Fog volume properties
                            if let Some(ref mut data) = fog_volume_data {
                                fog_volume_changed = draw_fog_volume_section(ui, data);
                                ui.add_space(4.0);
                            }

                            // Decal properties
                            if let Some(ref mut data) = decal_data {
                                let result = draw_decal_section(ui, data);
                                decal_changed |= result.changed;
                                decal_browse_requested = result.browse_requested;
                                ui.add_space(4.0);
                            }

                            // Blockout shape properties
                            if let Some(ref mut data) = stairs_data {
                                stairs_changed = draw_stairs_section(ui, data);
                                ui.add_space(4.0);
                            }
                            if let Some(ref mut data) = ramp_data {
                                ramp_changed = draw_ramp_section(ui, data);
                                ui.add_space(4.0);
                            }
                            if let Some(ref mut data) = arch_data {
                                arch_changed = draw_arch_section(ui, data);
                                ui.add_space(4.0);
                            }
                            if let Some(ref mut data) = lshape_data {
                                lshape_changed = draw_lshape_section(ui, data);
                                ui.add_space(4.0);
                            }

                            // Spline follower properties
                            if let Some(ref mut data) = spline_follower_data {
                                let result = draw_spline_follower_section(ui, data, spline_name.as_deref());
                                spline_follower_changed = result.changed;
                                open_spline_picker = result.open_spline_picker;
                                ui.add_space(4.0);
                            }

                            // Procedural placer properties
                            if let Some(ref mut data) = procedural_placer_data {
                                let result = draw_procedural_placer_section(ui, data);
                                procedural_placer_changed = result.changed;
                                open_placer_template_picker = result.open_template_picker;
                                remove_placer_template_index = result.remove_template_index;
                                ui.add_space(4.0);
                            }

                            // Custom entity inspector widgets (filtered by has_component)
                            // Skip zero-field markers here — they appear in the Markers section
                            {
                                let custom_entries: Vec<(
                                    &'static str,
                                    Option<InspectorWidgetFn>,
                                    TypeId,
                                )> = {
                                    let type_registry = world.resource::<AppTypeRegistry>().clone();
                                    let type_registry = type_registry.read();
                                    world
                                        .resource::<CustomEntityRegistry>()
                                        .entries
                                        .iter()
                                        .filter(|e| (e.has_component)(world, entity))
                                        .filter(|e| {
                                            // Keep entries with a custom inspector always
                                            if e.entity_type.draw_inspector.is_some() {
                                                return true;
                                            }
                                            // Skip zero-field (marker) components
                                            if let Some(reg) = type_registry.get(e.component_type_id) {
                                                !matches!(reg.type_info(), TypeInfo::Struct(s) if s.field_len() == 0)
                                            } else {
                                                true
                                            }
                                        })
                                        .map(|e| {
                                            (
                                                e.entity_type.name,
                                                e.entity_type.draw_inspector,
                                                e.component_type_id,
                                            )
                                        })
                                        .collect()
                                };
                                let config = ReflectEditorConfig::default();
                                for (name, draw_inspector, type_id) in custom_entries {
                                    if let Some(draw_fn) = draw_inspector {
                                        if draw_fn(world, entity, ui) {
                                            custom_inspector_changed = true;
                                        }
                                    } else {
                                        egui::CollapsingHeader::new(
                                            egui::RichText::new(name)
                                                .color(colors::TEXT_PRIMARY),
                                        )
                                        .default_open(true)
                                        .show(ui, |ui| {
                                            component_editor(
                                                world, entity, type_id, ui, &config,
                                            );
                                        });
                                    }
                                    ui.add_space(4.0);
                                }
                            }

                            ui.separator();
                            ui.add_space(4.0);
                        }

                        // Game components (registered via register_scene_component)
                        {
                            let game_comp_entries: Vec<(TypeId, String)> = {
                                let scene_reg = world.resource::<SceneComponentRegistry>();
                                let custom_reg = world.resource::<CustomEntityRegistry>();
                                let type_registry = world.resource::<AppTypeRegistry>().clone();
                                let type_registry = type_registry.read();

                                // Collect TypeIds already handled by custom entity registry
                                let custom_type_ids: Vec<TypeId> = custom_reg
                                    .entries
                                    .iter()
                                    .map(|e| e.component_type_id)
                                    .collect();

                                let entity_ref = world.entity(entity);
                                let archetype = entity_ref.archetype();
                                let component_type_ids: Vec<TypeId> = archetype
                                    .components()
                                    .iter()
                                    .filter_map(|&cid| {
                                        world.components().get_info(cid)?.type_id()
                                    })
                                    .collect();

                                scene_reg
                                    .type_ids
                                    .iter()
                                    .filter(|tid| component_type_ids.contains(tid))
                                    .filter(|tid| !custom_type_ids.contains(tid))
                                    .filter_map(|&tid| {
                                        let reg = type_registry.get(tid)?;
                                        // Skip zero-field markers
                                        if matches!(reg.type_info(), TypeInfo::Struct(s) if s.field_len() == 0) {
                                            return None;
                                        }
                                        let name = reg
                                            .type_info()
                                            .type_path_table()
                                            .short_path()
                                            .to_string();
                                        Some((tid, name))
                                    })
                                    .collect()
                            };

                            if !game_comp_entries.is_empty() {
                                ui.label(
                                    egui::RichText::new("Game Components")
                                        .strong()
                                        .color(colors::TEXT_SECONDARY),
                                );
                                ui.add_space(4.0);
                                let config = ReflectEditorConfig::default();
                                for (type_id, name) in &game_comp_entries {
                                    egui::CollapsingHeader::new(
                                        egui::RichText::new(name.as_str())
                                            .color(colors::TEXT_PRIMARY),
                                    )
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        component_editor(
                                            world, entity, *type_id, ui, &config,
                                        );
                                    });
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                                ui.add_space(4.0);
                            }
                        }

                        // Add Component button
                        ui.add_space(4.0);
                        if ui
                            .button(egui::RichText::new("+ Add Component").color(colors::ACCENT_GREEN))
                            .clicked()
                        {
                            let mut palette_state = world.resource_mut::<CommandPaletteState>();
                            open_add_component_palette(&mut palette_state, entity);
                        }
                        ui.add_space(4.0);

                        ui.separator();
                        ui.add_space(4.0);

                        // Show all components via reflection
                        egui::CollapsingHeader::new(
                            egui::RichText::new("All Components").color(colors::TEXT_SECONDARY),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            draw_all_components(world, entity, ui);
                        });

                        // Markers section (zero-field components)
                        {
                            let marker_names = collect_marker_names(world, entity);
                            if !marker_names.is_empty() {
                                egui::CollapsingHeader::new(
                                    egui::RichText::new("Markers")
                                        .color(colors::TEXT_SECONDARY),
                                )
                                .default_open(false)
                                .show(ui, |ui| {
                                    for name in &marker_names {
                                        ui.label(
                                            egui::RichText::new(name)
                                                .color(colors::TEXT_PRIMARY),
                                        );
                                    }
                                });
                            }
                        }
                    });
                }
                _ => {
                    // Multiple selection
                    ui.add_space(4.0);

                    ui.label(
                        egui::RichText::new(format!("{} entities selected", selection_count))
                            .strong()
                            .size(14.0)
                            .color(colors::TEXT_PRIMARY),
                    );

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // RigidBody type selector for multi-selection
                        if has_rigidbodies {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} of {} have physics",
                                    rigidbody_types.len(),
                                    selection_count
                                ))
                                .small()
                                .color(colors::TEXT_MUTED),
                            );
                            ui.add_space(4.0);
                            rigidbody_action = draw_rigidbody_section(ui, common_rigidbody_type);
                        } else {
                            ui.label(
                                egui::RichText::new("No shared properties to edit")
                                    .color(colors::TEXT_MUTED)
                                    .italics(),
                            );
                        }
                    });
                }
            }
        });

    // Determine if any property was changed this frame
    let any_change = transform_changed
        || entity_name != original_name
        || is_locked != original_locked
        || !matches!(rigidbody_action, ComponentAction::None)
        || point_light_changed
        || directional_light_changed
        || fog_volume_changed
        || decal_changed
        || stairs_changed
        || ramp_changed
        || arch_changed
        || lshape_changed
        || spline_follower_changed
        || procedural_placer_changed
        || custom_inspector_changed;

    // Take a snapshot before the first change in an editing session
    if any_change {
        let needs = world
            .get_resource::<InspectorPanelState>()
            .map(|s| s.needs_snapshot)
            .unwrap_or(true);
        if needs {
            TakeSnapshotCommand {
                description: "Inspector edit".to_string(),
            }
            .apply(world);
            if let Some(mut state) = world.get_resource_mut::<InspectorPanelState>() {
                state.needs_snapshot = false;
            }
        }
    } else if let Some(mut state) = world.get_resource_mut::<InspectorPanelState>() {
        // No changes this frame — reset so the next edit gets a fresh snapshot
        state.needs_snapshot = true;
    }

    // Apply transform changes back to the entity (single selection only)
    if transform_changed {
        if let (Some(entity), Some(new_transform)) = (single_entity, transform_copy) {
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                *transform = new_transform;
            }
        }
    }

    // Apply name changes back to the entity (single selection only)
    if entity_name != original_name {
        if let (Some(entity), Some(new_name)) = (single_entity, entity_name) {
            if let Some(mut name) = world.get_mut::<Name>(entity) {
                name.set(new_name);
            }
        }
    }

    // Apply locked state changes
    if is_locked != original_locked {
        if let Some(entity) = single_entity {
            if is_locked {
                world.entity_mut(entity).insert(Locked);
            } else {
                world.entity_mut(entity).remove::<Locked>();
            }
        }
    }

    // Apply RigidBody changes to all selected entities with RigidBody
    match rigidbody_action {
        ComponentAction::Update(new_type) => {
            for (entity, _) in &rigidbody_types {
                world.entity_mut(*entity).remove::<RigidBody>();
                world.entity_mut(*entity).insert(new_type.to_rigid_body());
            }
        }
        ComponentAction::Remove => {
            for (entity, _) in &rigidbody_types {
                world.entity_mut(*entity).remove::<RigidBody>();
            }
        }
        ComponentAction::None => {}
    }

    // Apply point light changes
    if point_light_changed {
        if let (Some(entity), Some(data)) = (single_entity, point_light_data) {
            let new_color = Color::srgb(data.color[0], data.color[1], data.color[2]);

            // Update the marker component
            if let Some(mut marker) = world.get_mut::<SceneLightMarker>(entity) {
                marker.color = new_color;
                marker.intensity = data.intensity;
                marker.range = data.range;
                marker.radius = data.radius;
                marker.shadows_enabled = data.shadows_enabled;
            }

            // Update the actual PointLight component
            if let Some(mut light) = world.get_mut::<PointLight>(entity) {
                light.color = new_color;
                light.intensity = data.intensity;
                light.range = data.range;
                light.radius = data.radius;
                light.shadows_enabled = data.shadows_enabled;
            }

            // Toggle VolumetricLight component
            let has_volumetric = world.get::<VolumetricLight>(entity).is_some();
            if data.volumetric && !has_volumetric {
                world.entity_mut(entity).insert(VolumetricLight);
            } else if !data.volumetric && has_volumetric {
                world.entity_mut(entity).remove::<VolumetricLight>();
            }
        }
    }

    // Apply directional light changes
    if directional_light_changed {
        if let (Some(entity), Some(data)) = (single_entity, directional_light_data) {
            let new_color = Color::srgb(data.color[0], data.color[1], data.color[2]);

            // Update the marker component
            if let Some(mut marker) = world.get_mut::<DirectionalLightMarker>(entity) {
                marker.color = new_color;
                marker.illuminance = data.illuminance;
                marker.shadows_enabled = data.shadows_enabled;
            }

            // Update the actual DirectionalLight component
            if let Some(mut light) = world.get_mut::<DirectionalLight>(entity) {
                light.color = new_color;
                light.illuminance = data.illuminance;
                light.shadows_enabled = data.shadows_enabled;
            }

            // Toggle VolumetricLight component
            let has_volumetric = world.get::<VolumetricLight>(entity).is_some();
            if data.volumetric && !has_volumetric {
                world.entity_mut(entity).insert(VolumetricLight);
            } else if !data.volumetric && has_volumetric {
                world.entity_mut(entity).remove::<VolumetricLight>();
            }
        }
    }

    // Apply fog volume changes
    if fog_volume_changed {
        if let (Some(entity), Some(data)) = (single_entity, fog_volume_data) {
            let new_fog_color = Color::srgb(data.fog_color[0], data.fog_color[1], data.fog_color[2]);
            let new_light_tint = Color::srgb(data.light_tint[0], data.light_tint[1], data.light_tint[2]);

            // Update the marker component
            if let Some(mut marker) = world.get_mut::<FogVolumeMarker>(entity) {
                marker.fog_color = new_fog_color;
                marker.density_factor = data.density_factor;
                marker.absorption = data.absorption;
                marker.scattering = data.scattering;
                marker.scattering_asymmetry = data.scattering_asymmetry;
                marker.light_tint = new_light_tint;
                marker.light_intensity = data.light_intensity;
            }

            // Update the actual FogVolume component
            if let Some(mut fog) = world.get_mut::<bevy::light::FogVolume>(entity) {
                fog.fog_color = new_fog_color;
                fog.density_factor = data.density_factor;
                fog.absorption = data.absorption;
                fog.scattering = data.scattering;
                fog.scattering_asymmetry = data.scattering_asymmetry;
                fog.light_tint = new_light_tint;
                fog.light_intensity = data.light_intensity;
            }
        }
    }

    // Apply decal changes (sync_decal_markers handles runtime component rebuild)
    if decal_changed {
        if let (Some(entity), Some(data)) = (single_entity, decal_data) {
            if let Some(mut marker) = world.get_mut::<DecalMarker>(entity) {
                marker.base_color_path = data.base_color_path;
                marker.normal_map_path = data.normal_map_path;
                marker.emissive_path = data.emissive_path;
                marker.decal_type = data.decal_type;
                marker.depth_fade_factor = data.depth_fade_factor;
            }
        }
    }

    // Open decal texture picker if requested
    if let Some(slot) = decal_browse_requested {
        world
            .resource_mut::<CommandPaletteState>()
            .open_pick_texture(slot, single_entity);
    }

    // Apply blockout shape changes (mesh regeneration is handled by Changed<T> systems)
    if stairs_changed {
        if let (Some(entity), Some(data)) = (single_entity, stairs_data) {
            if let Some(mut marker) = world.get_mut::<StairsMarker>(entity) {
                marker.step_count = data.step_count;
                marker.height = data.height;
                marker.depth = data.depth;
                marker.width = data.width;
            }
        }
    }

    if ramp_changed {
        if let (Some(entity), Some(data)) = (single_entity, ramp_data) {
            if let Some(mut marker) = world.get_mut::<RampMarker>(entity) {
                marker.height = data.height;
                marker.length = data.length;
                marker.width = data.width;
            }
        }
    }

    if arch_changed {
        if let (Some(entity), Some(data)) = (single_entity, arch_data) {
            if let Some(mut marker) = world.get_mut::<ArchMarker>(entity) {
                marker.opening_width = data.opening_width;
                marker.opening_height = data.opening_height;
                marker.thickness = data.thickness;
                marker.wall_width = data.wall_width;
                marker.wall_height = data.wall_height;
                marker.arch_segments = data.arch_segments;
            }
        }
    }

    if lshape_changed {
        if let (Some(entity), Some(data)) = (single_entity, lshape_data) {
            if let Some(mut marker) = world.get_mut::<LShapeMarker>(entity) {
                marker.arm1_length = data.arm1_length;
                marker.arm2_length = data.arm2_length;
                marker.arm_width = data.arm_width;
                marker.height = data.height;
            }
        }
    }

    // Apply spline follower changes
    if spline_follower_changed {
        if let (Some(entity), Some(data)) = (single_entity, spline_follower_data.clone()) {
            if let Some(mut follower) = world.get_mut::<SplineFollower>(entity) {
                follower.spline = data.spline;
                follower.speed = data.speed;
                follower.t = data.t;
                follower.loop_mode = data.loop_mode;
                follower.state = data.state;
                follower.align_to_tangent = data.align_to_tangent;
                follower.up_vector = Vec3::new(data.up_vector[0], data.up_vector[1], data.up_vector[2]);
                follower.direction = data.direction;
                follower.offset = Vec3::new(data.offset[0], data.offset[1], data.offset[2]);
                follower.constant_speed = data.constant_speed;
            }
        }
    }

    // Remove template from procedural placer
    if let Some(remove_index) = remove_placer_template_index {
        if let Some(entity) = single_entity {
            if let Some(mut placer) = world.get_mut::<ProceduralPlacer>(entity) {
                if remove_index < placer.templates.len() {
                    placer.templates.remove(remove_index);
                }
            }
        }
    }

    // Apply procedural placer changes
    if procedural_placer_changed {
        if let (Some(entity), Some(data)) = (single_entity, procedural_placer_data.clone()) {
            if let Some(mut placer) = world.get_mut::<ProceduralPlacer>(entity) {
                placer.mode = match data.mode {
                    0 => SamplingMode::Uniform,
                    _ => SamplingMode::Random { seed: data.seed },
                };
                placer.count = data.count;
                placer.orientation = match data.orientation {
                    0 => PlacementOrientation::Identity,
                    1 => PlacementOrientation::AlignToTangent {
                        up: Vec3::new(data.up_vector[0], data.up_vector[1], data.up_vector[2]),
                    },
                    2 => PlacementOrientation::AlignToSurface,
                    3 => PlacementOrientation::RandomYaw,
                    _ => PlacementOrientation::RandomFull,
                };
                placer.offset = Vec3::new(data.offset[0], data.offset[1], data.offset[2]);
                placer.projection.enabled = data.projection_enabled;
                placer.projection.direction = Vec3::new(
                    data.projection_direction[0],
                    data.projection_direction[1],
                    data.projection_direction[2],
                );
                placer.projection.local_space = data.projection_local_space;
                placer.projection.ray_origin_offset = data.projection_ray_offset;
                placer.projection.max_distance = data.projection_max_distance;
                placer.use_bounds_offset = data.use_bounds_offset;
                placer.enabled = data.enabled;
                // Update template weights
                for (i, template_data) in data.templates.iter().enumerate() {
                    if i < placer.templates.len() {
                        placer.templates[i].weight = template_data.weight;
                    }
                }
            }
        }
    }

    // Open entity picker for spline field (SplineFollower)
    if open_spline_picker {
        if let Some(entity) = single_entity {
            let callback_id = make_callback_id(entity, "spline");
            let mut palette_state = world.resource_mut::<CommandPaletteState>();
            palette_state.open_entity_picker(entity, "Spline", callback_id);
        }
    }

    // Open entity picker for placer template field
    if open_placer_template_picker {
        if let Some(entity) = single_entity {
            let callback_id = make_callback_id(entity, "placer_template");
            let mut palette_state = world.resource_mut::<CommandPaletteState>();
            palette_state.open_entity_picker(entity, "Placer Template", callback_id);
        }
    }

    // Handle pending entity selection from picker
    {
        let pending = world.resource::<PendingEntitySelection>().0;
        if let Some(selection) = pending {
            if let Some(entity) = single_entity {
                // Check if this is for the SplineFollower spline field
                let spline_callback = make_callback_id(entity, "spline");
                if selection.callback_id == spline_callback {
                    if let Some(mut follower) = world.get_mut::<SplineFollower>(entity) {
                        follower.spline = selection.selected_entity;
                    }
                }

                // Check if this is for adding a placer template
                let placer_template_callback = make_callback_id(entity, "placer_template");
                if selection.callback_id == placer_template_callback {
                    if let Some(mut placer) = world.get_mut::<ProceduralPlacer>(entity) {
                        placer.templates.push(bevy_procedural::WeightedTemplate::new(
                            selection.selected_entity,
                            1.0,
                        ));
                    }
                }

            }
            // Clear the pending selection
            world.resource_mut::<PendingEntitySelection>().0 = None;
        }
    }

    // Check for entity picker request from reflection editor
    {
        let field_name: Option<String> = ctx.memory(|mem| {
            mem.data.get_temp::<String>(egui::Id::new("entity_picker_request"))
        });
        if let Some(field) = field_name {
            // Clear the request
            ctx.memory_mut(|mem| {
                mem.data.remove::<String>(egui::Id::new("entity_picker_request"));
            });
            // Open the entity picker
            if let Some(entity) = single_entity {
                let callback_id = make_callback_id(entity, &field);
                let mut palette_state = world.resource_mut::<CommandPaletteState>();
                palette_state.open_entity_picker(entity, &field, callback_id);
            }
        }
    }

    // Update the panel state resource with the actual panel width
    if let Some(response) = &panel_response {
        if let Some(mut panel_state) = world.get_resource_mut::<InspectorPanelState>() {
            panel_state.width = response.response.rect.width();
        }
    }

    // Toggle pin state if button was clicked
    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::ObjectInspector) {
            pinned.0.insert(EditorMode::ObjectInspector);
        }
    }
}

/// Draw all components on an entity using reflection
fn draw_all_components(world: &mut World, entity: Entity, ui: &mut egui::Ui) {
    // Collect custom entity and scene-registered component TypeIds to exclude
    // (these are already shown in the main inspector area)
    let custom_type_ids: Vec<TypeId> = {
        let mut ids: Vec<TypeId> = world
            .resource::<CustomEntityRegistry>()
            .entries
            .iter()
            .map(|e| e.component_type_id)
            .collect();
        ids.extend_from_slice(&world.resource::<SceneComponentRegistry>().type_ids);
        ids
    };

    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry_guard = type_registry.read();

    // Collect component type IDs for this entity
    let mut component_ids: Vec<(TypeId, String)> = {
        let entity_ref = world.entity(entity);
        let archetype = entity_ref.archetype();

        archetype
            .components()
            .iter()
            .filter_map(|&component_id| {
                let component_info = world.components().get_info(component_id)?;
                let type_id = component_info.type_id()?;

                // Check if this type is registered for reflection
                let registration = type_registry_guard.get(type_id)?;

                // Check if it has ReflectComponent
                if registration.data::<ReflectComponent>().is_none() {
                    return None;
                }

                // Skip zero-field (marker) components — shown in Markers section
                if matches!(registration.type_info(), TypeInfo::Struct(s) if s.field_len() == 0) {
                    return None;
                }

                let short_name = registration
                    .type_info()
                    .type_path_table()
                    .short_path()
                    .to_string();

                Some((type_id, short_name))
            })
            .collect()
    };

    // Sort components alphabetically by name
    component_ids.sort_by(|a, b| a.1.cmp(&b.1));

    drop(type_registry_guard);

    if component_ids.is_empty() {
        ui.label(
            egui::RichText::new("No reflectable components")
                .color(colors::TEXT_MUTED)
                .italics(),
        );
        return;
    }

    let config = ReflectEditorConfig::default();

    for (type_id, name) in component_ids {
        // Skip components we already have custom editors for
        if name == "Transform"
            || name == "SceneLightMarker"
            || name == "DirectionalLightMarker"
            || name == "FogVolumeMarker"
            || name == "RigidBody"
            || name == "SplineFollower"
            || name == "StairsMarker"
            || name == "RampMarker"
            || name == "ArchMarker"
            || name == "LShapeMarker"
        {
            continue;
        }

        // Skip custom entity components (shown in main inspector area)
        if custom_type_ids.contains(&type_id) {
            continue;
        }

        ui.add_space(2.0);

        // Draw the component using reflection
        component_editor(world, entity, type_id, ui, &config);
    }
}

/// Returns the short names of zero-field (marker) components on this entity.
/// These are displayed in a dedicated "Markers" section and excluded from "All Components".
fn collect_marker_names(world: &World, entity: Entity) -> Vec<String> {
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();
    let entity_ref = world.entity(entity);
    let archetype = entity_ref.archetype();

    let mut names: Vec<String> = archetype
        .components()
        .iter()
        .filter_map(|&component_id| {
            let info = world.components().get_info(component_id)?;
            let type_id = info.type_id()?;
            let reg = type_registry.get(type_id)?;
            if reg.data::<ReflectComponent>().is_none() {
                return None;
            }
            match reg.type_info() {
                TypeInfo::Struct(s) if s.field_len() == 0 => {}
                _ => return None,
            }
            let name = reg
                .type_info()
                .type_path_table()
                .short_path()
                .to_string();
            Some(name)
        })
        .collect();

    names.sort();
    names
}

/// Draw the component editor popup window
fn draw_component_editor_popup(world: &mut World) {
    // Don't draw UI when editor is disabled
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    // Only show in ObjectInspector mode
    let current_mode = world.resource::<State<EditorMode>>().get();
    if *current_mode != EditorMode::ObjectInspector {
        // Clear editing state when leaving mode
        let mut editor_state = world.resource_mut::<ComponentEditorState>();
        editor_state.editing_component = None;
        return;
    }

    // Get the editing component info and just_opened state
    let (editing_component, just_opened) = {
        let state = world.resource::<ComponentEditorState>();
        (state.editing_component.clone(), state.just_opened)
    };

    let Some((type_id, component_name)) = editing_component else {
        return;
    };

    // Get the selected entity
    let selected_entity: Option<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).next()
    };

    let Some(entity) = selected_entity else {
        // Clear editing if no entity selected
        let mut editor_state = world.resource_mut::<ComponentEditorState>();
        editor_state.editing_component = None;
        return;
    };

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

    let mut close_editor = false;

    // Check for Escape to close
    if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close_editor = true;
    }

    let window_title = format!("Edit: {}", component_name);

    // When just opened, clear any previous focus tracking state
    if just_opened {
        clear_focus_state(&ctx);
    }

    // Use focused config - we keep trying to focus until we succeed
    // The focus tracking in egui memory prevents duplicate focus requests
    let config = ReflectEditorConfig::expanded_and_focused();

    egui::Window::new(window_title)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .default_width(350.0)
        .max_height(500.0)
        .show(&ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                component_editor(world, entity, type_id, ui, &config);
            });
        });

    // Clear just_opened flag (focus tracking is handled by egui memory)
    if just_opened {
        let mut editor_state = world.resource_mut::<ComponentEditorState>();
        editor_state.just_opened = false;
    }

    if close_editor {
        // Clear focus state when closing
        clear_focus_state(&ctx);
        let mut editor_state = world.resource_mut::<ComponentEditorState>();
        editor_state.editing_component = None;
    }
}
