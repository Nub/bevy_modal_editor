//! The material editor (M4-D11, spec §7): a dedicated floating surface for
//! authoring library materials — live render-to-texture preview, the full
//! `MaterialDef` surface (color, metallic/roughness, emissive, alpha modes,
//! flags, base-color texture through the identity pipeline), and ASSET-scoped
//! undo: while the editor is open it claims `HistoryScope::Asset`, so Ctrl+Z
//! unwinds material edits, never scene work behind the panel.
//!
//! Interaction grammar (v1 consult, behavior only): `material.edit` toggles
//! the editor for the selection's material (else the first library material);
//! Escape closes it only when empty-handed — one layer per press, same as
//! open prefab instances.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::feathers::controls::{
    ColorChannel, FeathersColorSlider, FeathersSlider, FeathersToggleSwitch,
};
use bevy::prelude::*;
use bevy::ui::widget::ImageNode;
use bevy::ui::{Checked, PositionType, px};
use bevy::ui_widgets::ValueChange;
use editor_core::prelude::*;
use editor_scene::materials::{MaterialAlphaMode, MaterialDef, MaterialLibrary};
use editor_scene::models::{EntryKind, ModelLibrary};
use uuid::Uuid;

use crate::appear::FloatingSurface;
use crate::style::{self, UiFonts};

/// Distinct from the palette preview (41) and the outliner (31).
const MATERIAL_PREVIEW_LAYER: usize = 42;
const MATERIAL_PREVIEW_HOME: Vec3 = Vec3::new(0.0, -950.0, 0.0);
const PREVIEW_SIZE: u32 = 256;

#[derive(Resource, Default)]
pub(crate) struct MaterialEditorState {
    pub open: bool,
    pub target: Option<Uuid>,
    /// Set by undo/redo/target-switch: widget values re-seed from the library
    /// exactly once (never during a live drag, which owns its own value).
    pub refresh: bool,
}

/// Asset history (D11): snapshots of the def BEFORE an edit burst. Slider
/// drags coalesce by (material, field) within a short window — one undo entry
/// per gesture, not per pixel of drag.
#[derive(Resource, Default)]
pub(crate) struct MaterialHistory {
    undo: Vec<(Uuid, MaterialDef)>,
    redo: Vec<(Uuid, MaterialDef)>,
    last_edit: Option<(Uuid, Field, f64)>,
}

const COALESCE_SECONDS: f64 = 0.75;

/// Which def field a widget edits.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Field {
    BaseR,
    BaseG,
    BaseB,
    BaseA,
    Metallic,
    Roughness,
    EmissiveR,
    EmissiveG,
    EmissiveB,
    EmissiveIntensity,
    AlphaCutoff,
    Unlit,
    DoubleSided,
    AlphaMode,
    Texture,
}

#[derive(Component)]
pub(crate) struct MaterialEditorRoot;
#[derive(Component)]
pub(crate) struct MaterialEditorBody;
#[derive(Component)]
pub(crate) struct MaterialEditorTitle;

#[derive(Resource)]
pub(crate) struct MaterialPreviewRig {
    image: Handle<Image>,
    pub(crate) material: Handle<StandardMaterial>,
    pub(crate) camera: Entity,
}

/// Widget state to re-assert AFTER the bsn template applies: the template
/// lands later (scene construction) and overwrites direct SliderValue
/// inserts — and retained-scene patching strips marker COMPONENTS, so the
/// pending seeds live in a resource the template can't touch.
#[derive(Resource, Default)]
pub(crate) struct PendingSeeds(Vec<(Entity, f32, f32, f32)>);

pub(crate) fn seed_slider_values(
    mut seeds: ResMut<PendingSeeds>,
    ready: Query<(), With<bevy::ui_widgets::SliderValue>>,
    entities: &bevy::ecs::entity::Entities,
    mut commands: Commands,
) {
    seeds.0.retain(|(entity, value, min, max)| {
        // Entities::contains sees RESERVED ids too — a component query here
        // would drop seeds queued this frame before their commands flushed.
        if !entities.contains(*entity) {
            return false; // rebuilt away before its template ever landed
        }
        if !ready.contains(*entity) {
            return true; // template not applied yet — keep waiting
        }
        // Immutable widget components: re-insert (which also fires the
        // Changed detection feathers' own sync systems key off).
        debug!("seeding slider {entity:?} to {value} in [{min}, {max}]");
        commands.entity(*entity).insert((
            bevy::ui_widgets::SliderValue(*value),
            bevy::ui_widgets::SliderRange::new(*min, *max),
            // Absent from the widget as spawned — and feathers' value/track
            // sync query REQUIRES it, so without this the display never
            // updates at all.
            bevy::ui_widgets::SliderPrecision(2),
        ));
        false
    });
}

pub(crate) struct MaterialEditorFeature;

impl EditorFeature for MaterialEditorFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("material-editor", "Material Editor")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            ActionDef::new("material.edit", "Edit Material")
                .describe("Open the material editor for the selection's material")
                .context("normal")
                .bind("space shift+m"),
        );
    }
}

pub(crate) fn setup_material_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let image = images.add(Image::new_target_texture(
        PREVIEW_SIZE,
        PREVIEW_SIZE,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let material = materials.add(StandardMaterial::default());
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(MATERIAL_PREVIEW_HOME),
        RenderLayers::layer(MATERIAL_PREVIEW_LAYER),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 7_000.0,
            ..default()
        },
        Transform::from_translation(MATERIAL_PREVIEW_HOME + Vec3::new(3.0, 5.0, 2.5))
            .looking_at(MATERIAL_PREVIEW_HOME, Vec3::Y),
        RenderLayers::layer(MATERIAL_PREVIEW_LAYER),
    ));
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::NONE),
                order: -11,
                is_active: false,
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Transform::from_translation(MATERIAL_PREVIEW_HOME + Vec3::new(0.0, 0.9, 2.6))
                .looking_at(MATERIAL_PREVIEW_HOME, Vec3::Y),
            RenderLayers::layer(MATERIAL_PREVIEW_LAYER),
        ))
        .id();
    commands.insert_resource(MaterialPreviewRig {
        image,
        material,
        camera,
    });
}

/// `material.edit` toggles; empty-handed Escape closes (one layer per press —
/// a live selection absorbs its Escape first, same grammar as open prefabs).
pub(crate) fn collect_editor_actions(
    mut reader: MessageReader<ActionInvoked>,
    selection: Query<&editor_scene::materials::MaterialRef, With<Selected>>,
    selected_any: Query<(), With<Selected>>,
    escape_from_capture: Res<editor_core::resolver::EscapeFromCapture>,
    library: Res<MaterialLibrary>,
    mut state: ResMut<MaterialEditorState>,
    mut scope: ResMut<HistoryScope>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "material.edit" => {
                if state.open {
                    state.open = false;
                    continue;
                }
                let target = selection
                    .iter()
                    .next()
                    .map(|r| r.0)
                    .filter(|id| library.get(id).is_some())
                    .or_else(|| library.materials.last().map(|def| def.id));
                match target {
                    Some(target_id) => {
                        state.open = true;
                        state.target = Some(target_id);
                        state.refresh = true;
                    }
                    None => {
                        feedback.write(editor_scene::SceneIoFeedback {
                            message: "no materials yet — create one with 'New Material'".into(),
                            success: false,
                        });
                    }
                }
            }
            "core.escape-home"
                if state.open && !escape_from_capture.0 && selected_any.is_empty() =>
            {
                state.open = false;
            }
            _ => {}
        }
    }
    // The open editor owns Ctrl+Z (asset history).
    let want = if state.open {
        HistoryScope::Asset
    } else {
        HistoryScope::Scene
    };
    if *scope != want {
        *scope = want;
    }
}

/// Undo/redo while the editor holds the scope.
pub(crate) fn apply_material_history(
    mut reader: MessageReader<ActionInvoked>,
    mut history: ResMut<MaterialHistory>,
    mut library: ResMut<MaterialLibrary>,
    mut editor: ResMut<MaterialEditorState>,
) {
    if !editor.open {
        return;
    }
    for invoked in reader.read() {
        let (from, to) = match invoked.action.as_str() {
            "core.undo" => (&mut history.undo, true),
            "core.redo" => (&mut history.redo, false),
            _ => continue,
        };
        let Some((id, snapshot)) = from.pop() else {
            continue;
        };
        let current = library.get(&id).cloned();
        if let (Some(current), Some(def)) = (current, library.get_mut(&id)) {
            *def = snapshot;
            if to {
                history.redo.push((id, current));
            } else {
                history.undo.push((id, current));
            }
            history.last_edit = None;
            editor.refresh = true;
        }
    }
}

/// One committed edit: snapshot-before into the undo stack (coalescing drag
/// bursts), apply the mutation, bump the library (visual sync + save follow).
fn edit_material(
    library: &mut MaterialLibrary,
    history: &mut MaterialHistory,
    time_seconds: f64,
    id: Uuid,
    field: Field,
    is_final: bool,
    mutate: impl FnOnce(&mut MaterialDef),
) {
    let Some(before) = library.get(&id).cloned() else {
        return;
    };
    let coalesce = history.last_edit.is_some_and(|(last_id, last_field, at)| {
        last_id == id && last_field == field && time_seconds - at < COALESCE_SECONDS
    });
    if !coalesce {
        history.undo.push((id, before));
        history.redo.clear();
    }
    // A finished interaction ends the burst — the next edit starts a fresh
    // undo entry even on the same field.
    history.last_edit = if is_final {
        None
    } else {
        Some((id, field, time_seconds))
    };
    if let Some(def) = library.get_mut(&id) {
        mutate(def);
    }
}

/// Slider/color-slider commits.
pub(crate) fn on_field_value(
    change: On<ValueChange<f32>>,
    fields: Query<&Field>,
    state: Res<MaterialEditorState>,
    time: Res<Time>,
    mut library: ResMut<MaterialLibrary>,
    mut history: ResMut<MaterialHistory>,
) {
    let Ok(field) = fields.get(change.source) else {
        return;
    };
    let Some(id) = state.target else { return };
    let value = change.value;
    edit_material(
        &mut library,
        &mut history,
        time.elapsed_secs_f64(),
        id,
        *field,
        change.is_final,
        |def| match field {
            Field::BaseR => def.base_color[0] = value,
            Field::BaseG => def.base_color[1] = value,
            Field::BaseB => def.base_color[2] = value,
            Field::BaseA => def.base_color[3] = value,
            Field::Metallic => def.metallic = value,
            Field::Roughness => def.roughness = value,
            Field::EmissiveR => def.emissive[0] = value,
            Field::EmissiveG => def.emissive[1] = value,
            Field::EmissiveB => def.emissive[2] = value,
            Field::EmissiveIntensity => def.emissive_intensity = value,
            Field::AlphaCutoff => def.alpha_cutoff = value,
            _ => {}
        },
    );
}

/// Toggle commits (unlit, double-sided).
pub(crate) fn on_field_toggle(
    change: On<ValueChange<bool>>,
    fields: Query<&Field>,
    state: Res<MaterialEditorState>,
    time: Res<Time>,
    mut library: ResMut<MaterialLibrary>,
    mut history: ResMut<MaterialHistory>,
    mut commands: Commands,
) {
    let Ok(field) = fields.get(change.source) else {
        return;
    };
    let Some(id) = state.target else { return };
    let value = change.value;
    edit_material(
        &mut library,
        &mut history,
        time.elapsed_secs_f64(),
        id,
        *field,
        true,
        |def| match field {
            Field::Unlit => def.unlit = value,
            Field::DoubleSided => def.double_sided = value,
            _ => {}
        },
    );
    // Feathers toggles are stateless: reflect the committed value back.
    if value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
}

/// Chip presses: alpha-mode cycle, texture cycle (imported textures + none).
pub(crate) fn on_chip_press(
    press: On<Pointer<Press>>,
    fields: Query<&Field>,
    time: Res<Time>,
    models: Res<ModelLibrary>,
    mut library: ResMut<MaterialLibrary>,
    mut history: ResMut<MaterialHistory>,
    mut editor: ResMut<MaterialEditorState>,
) {
    let Ok(field) = fields.get(press.entity) else {
        return;
    };
    debug!("material chip pressed: {field:?}");
    let Some(id) = editor.target else { return };
    let textures: Vec<Uuid> = models
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Texture)
        .map(|e| e.uuid)
        .collect();
    edit_material(
        &mut library,
        &mut history,
        time.elapsed_secs_f64(),
        id,
        *field,
        true,
        |def| match field {
            Field::AlphaMode => {
                def.alpha_mode = match def.alpha_mode {
                    MaterialAlphaMode::Opaque => MaterialAlphaMode::Blend,
                    MaterialAlphaMode::Blend => MaterialAlphaMode::Mask,
                    MaterialAlphaMode::Mask => MaterialAlphaMode::Opaque,
                };
            }
            Field::Texture => {
                // none → tex0 → tex1 → … → none
                let next = match def.base_color_texture {
                    None => textures.first().copied(),
                    Some(current) => textures
                        .iter()
                        .position(|t| *t == current)
                        .and_then(|i| textures.get(i + 1))
                        .copied(),
                };
                def.base_color_texture = next;
            }
            _ => {}
        },
    );
    // Chip labels come from the def — rebuild them.
    editor.refresh = true;
}

/// Live preview: the def renders through THE conversion every library change.
pub(crate) fn sync_preview(
    state: Res<MaterialEditorState>,
    library: Res<MaterialLibrary>,
    models: Res<ModelLibrary>,
    assets: Option<Res<AssetServer>>,
    rig: Option<Res<MaterialPreviewRig>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cameras: Query<&mut Camera>,
) {
    let Some(rig) = rig else { return };
    if let Ok(mut camera) = cameras.get_mut(rig.camera) {
        let want = state.open;
        if camera.is_active != want {
            camera.is_active = want;
        }
    }
    if !state.open || !(library.is_changed() || state.is_changed()) {
        return;
    }
    let Some(def) = state.target.and_then(|id| library.get(&id)) else {
        return;
    };
    if let Some(mut material) = materials.get_mut(&rig.material) {
        *material = editor_scene::materials::to_standard_material(def, &models, assets.as_deref());
    }
}

/// Root surface, spawned hidden once at startup.
pub(crate) fn spawn_editor_root(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            MaterialEditorRoot,
            FloatingSurface::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(480.0),
                top: px(48.0),
                bottom: px(56.0),
                width: px(300.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(style::space::S),
                padding: UiRect::all(px(style::space::M)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(style::radius::L)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.125, 0.122, 0.117)),
            BorderColor::all(style::HAIRLINE),
            style::floating_shadow(),
            GlobalZIndex(60),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((Node {
                align_items: AlignItems::Center,
                column_gap: px(style::space::S),
                ..default()
            },))
                .with_children(|header| {
                    header.spawn((
                        Text::new("MATERIAL"),
                        style::sans_medium(&fonts, 11.0),
                        TextColor(style::color::TEXT_DIM),
                    ));
                    header.spawn((
                        MaterialEditorTitle,
                        Text::new(""),
                        style::no_wrap(),
                        style::sans_medium(&fonts, 13.0),
                        TextColor(style::color::TEXT_BRIGHT),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new("\u{238b} close"),
                        style::mono(&fonts, 10.0),
                        TextColor(style::color::TEXT_DIM),
                    ));
                });
            root.spawn((
                MaterialEditorBody,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(style::space::XS),
                    flex_grow: 1.0,
                    min_height: px(0.0),
                    // Clip, don't scroll: scroll containers unconstrain child
                    // heights and collapse the widgets' percent-sized tracks.
                    overflow: bevy::ui::Overflow::clip(),
                    ..default()
                },
            ));
        });
}

/// Rebuild the body when the editor opens/retargets or a chip changed a
/// discrete field. Slider drags never rebuild (state.refresh gates it).
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_editor_ui(
    mut state: ResMut<MaterialEditorState>,
    library: Res<MaterialLibrary>,
    models: Res<ModelLibrary>,
    rig: Option<Res<MaterialPreviewRig>>,
    fonts: Res<UiFonts>,
    mut root: Query<&mut Visibility, With<MaterialEditorRoot>>,
    body: Query<Entity, With<MaterialEditorBody>>,
    mut title: Query<&mut Text, With<MaterialEditorTitle>>,
    mut pending: ResMut<PendingSeeds>,
    mut commands: Commands,
    mut was_open: Local<bool>,
) {
    let visibility_changed = state.open != *was_open;
    if !visibility_changed && !state.refresh {
        return;
    }
    *was_open = state.open;
    state.refresh = false;
    for mut visibility in &mut root {
        *visibility = if state.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !state.open {
        return;
    }
    let Some(def) = state.target.and_then(|id| library.get(&id)).cloned() else {
        return;
    };
    for mut text in &mut title {
        if text.0 != def.name {
            text.0 = def.name.clone();
        }
    }
    let Ok(body) = body.single() else { return };
    commands.entity(body).despawn_related::<Children>();

    let texture_label = def
        .base_color_texture
        .and_then(|uuid| models.get(&uuid).map(|e| e.name.clone()))
        .unwrap_or_else(|| "none".into());
    let alpha_label = match def.alpha_mode {
        MaterialAlphaMode::Opaque => "opaque",
        MaterialAlphaMode::Blend => "blend",
        MaterialAlphaMode::Mask => "mask",
    };

    // Widgets spawn via `commands` + ChildOf (bsn scenes have no child-spawner
    // entry point); plain rows use commands the same way for symmetry.
    if let Some(rig) = rig {
        commands.spawn((
            ImageNode::new(rig.image.clone()),
            Node {
                width: px(80.0),
                height: px(80.0),
                flex_shrink: 0.0,
                align_self: AlignSelf::Center,
                border_radius: BorderRadius::all(px(style::radius::S)),
                ..default()
            },
            ChildOf(body),
        ));
    }
    let caption = |commands: &mut Commands, label: &str| {
        commands.spawn((
            Text::new(label.to_string()),
            style::sans_medium(&fonts, 10.0),
            TextColor(style::color::TEXT_DIM),
            Node {
                margin: UiRect::top(px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(body),
        ));
    };
    // Fixed-height wrappers with flex_shrink 0: the height-capped panel would
    // otherwise COMPRESS rows, collapsing the widgets' percent-sized tracks.
    // Seeds queue in a resource — the bsn template lands after this frame and
    // would overwrite (and its patching strips marker components).
    let seeds = std::cell::RefCell::new(Vec::new());
    let row_wrapper = |commands: &mut Commands, height: f32| {
        commands
            .spawn((
                Node {
                    height: px(height),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    column_gap: px(style::space::S),
                    ..default()
                },
                ChildOf(body),
            ))
            .id()
    };
    let color_row = |commands: &mut Commands, field: Field, channel: ColorChannel, value: f32| {
        let wrapper = row_wrapper(commands, 20.0);
        let widget = commands
            .spawn_scene(bsn! {
                @FeathersColorSlider {
                    @value: {value},
                    @channel: {channel},
                }
            })
            .insert((field, ChildOf(wrapper)))
            .observe(on_field_value)
            .id();
        seeds.borrow_mut().push((widget, value, 0.0, 1.0));
    };
    let slider_row = |commands: &mut Commands,
                      fonts: &UiFonts,
                      label: &str,
                      field: Field,
                      value: f32,
                      max: f32| {
        let wrapper = row_wrapper(commands, 24.0);
        commands.spawn((
            Text::new(label.to_string()),
            style::no_wrap(),
            style::sans(fonts, 11.0),
            TextColor(style::color::TEXT_KEYS),
            Node {
                width: px(76.0),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(wrapper),
        ));
        let slot = commands
            .spawn((
                Node {
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(wrapper),
            ))
            .id();
        let widget = commands
            .spawn_scene(bsn! { @FeathersSlider })
            .insert((field, ChildOf(slot)))
            .observe(on_field_value)
            .id();
        seeds.borrow_mut().push((widget, value, 0.0, max));
    };
    let chip = |commands: &mut Commands, fonts: &UiFonts, field: Field, label: String| {
        let chip_entity = commands
            .spawn((
                field,
                Node {
                    padding: UiRect::axes(px(style::space::S), px(style::space::XS)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(style::radius::S)),
                    flex_shrink: 0.0,
                    align_self: AlignSelf::FlexStart,
                    ..default()
                },
                BorderColor::all(style::HAIRLINE),
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
                ChildOf(body),
            ))
            .observe(on_chip_press)
            .id();
        commands.spawn((
            Text::new(label),
            style::no_wrap(),
            style::sans(fonts, 11.0),
            TextColor(style::color::TEXT_KEYS),
            ChildOf(chip_entity),
        ));
    };
    let toggle = |commands: &mut Commands, fonts: &UiFonts, label: &str, field: Field, on: bool| {
        let row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(style::space::S),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(body),
            ))
            .id();
        let mut switch = commands.spawn_scene(bsn! { @FeathersToggleSwitch });
        switch.insert((field, ChildOf(row)));
        if on {
            switch.insert(Checked);
        }
        switch.observe(on_field_toggle);
        commands.spawn((
            Text::new(label.to_string()),
            style::sans(fonts, 11.0),
            TextColor(style::color::TEXT_KEYS),
            ChildOf(row),
        ));
    };

    caption(&mut commands, "BASE COLOR");
    color_row(
        &mut commands,
        Field::BaseR,
        ColorChannel::Red,
        def.base_color[0],
    );
    color_row(
        &mut commands,
        Field::BaseG,
        ColorChannel::Green,
        def.base_color[1],
    );
    color_row(
        &mut commands,
        Field::BaseB,
        ColorChannel::Blue,
        def.base_color[2],
    );
    color_row(
        &mut commands,
        Field::BaseA,
        ColorChannel::Alpha,
        def.base_color[3],
    );
    caption(&mut commands, "SURFACE");
    slider_row(
        &mut commands,
        &fonts,
        "metallic",
        Field::Metallic,
        def.metallic,
        1.0,
    );
    slider_row(
        &mut commands,
        &fonts,
        "roughness",
        Field::Roughness,
        def.roughness,
        1.0,
    );
    caption(&mut commands, "EMISSIVE");
    color_row(
        &mut commands,
        Field::EmissiveR,
        ColorChannel::Red,
        def.emissive[0],
    );
    color_row(
        &mut commands,
        Field::EmissiveG,
        ColorChannel::Green,
        def.emissive[1],
    );
    color_row(
        &mut commands,
        Field::EmissiveB,
        ColorChannel::Blue,
        def.emissive[2],
    );
    slider_row(
        &mut commands,
        &fonts,
        "intensity",
        Field::EmissiveIntensity,
        def.emissive_intensity,
        10.0,
    );
    caption(&mut commands, "ALPHA");
    chip(
        &mut commands,
        &fonts,
        Field::AlphaMode,
        format!("mode: {alpha_label}"),
    );
    slider_row(
        &mut commands,
        &fonts,
        "cutoff",
        Field::AlphaCutoff,
        def.alpha_cutoff,
        1.0,
    );
    caption(&mut commands, "FLAGS");
    toggle(&mut commands, &fonts, "unlit", Field::Unlit, def.unlit);
    toggle(
        &mut commands,
        &fonts,
        "double-sided",
        Field::DoubleSided,
        def.double_sided,
    );
    caption(&mut commands, "BASE COLOR TEXTURE");
    chip(
        &mut commands,
        &fonts,
        Field::Texture,
        format!("texture: {texture_label}"),
    );
    pending.0.extend(seeds.into_inner());
}
