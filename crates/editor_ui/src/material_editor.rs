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
use editor_scene::materials::{MaterialAlphaMode, MaterialDef, MaterialLibrary, TextureSlot};
use editor_scene::models::ModelLibrary;
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
    /// Rename (through the name prompt) — coalesces like any other field.
    Name,
    /// One per declared texture slot — the row knows which map it fills, so
    /// the panel builds itself from `TextureSlot::ALL`.
    Texture(TextureSlot),
    UvTilingX,
    UvTilingY,
}

/// Text mirroring a def field live. Color sliders draw no value of their own,
/// and rows only rebuild on `refresh` — so a readout has to track the library
/// rather than the layout, or it goes stale the moment a drag starts.
#[derive(Component, Clone, Copy)]
pub(crate) struct FieldReadout(pub Field);

/// The base-color chip in the section header: the resulting color, at a glance,
/// without reading four numbers.
#[derive(Component)]
pub(crate) struct BaseColorSwatch;

/// One option of the alpha-mode segmented control (a blind cycle hides the
/// choices; a segment shows all three and which one is live).
#[derive(Component, Clone, Copy)]
pub(crate) struct AlphaModeChip(pub MaterialAlphaMode);

/// The scalar a field edits, for readouts.
fn field_value(def: &MaterialDef, field: Field) -> Option<f32> {
    Some(match field {
        Field::BaseR => def.base_color[0],
        Field::BaseG => def.base_color[1],
        Field::BaseB => def.base_color[2],
        Field::BaseA => def.base_color[3],
        Field::Metallic => def.metallic,
        Field::Roughness => def.roughness,
        Field::EmissiveR => def.emissive[0],
        Field::EmissiveG => def.emissive[1],
        Field::EmissiveB => def.emissive[2],
        Field::EmissiveIntensity => def.emissive_intensity,
        Field::AlphaCutoff => def.alpha_cutoff,
        Field::UvTilingX => def.uv_tiling[0],
        Field::UvTilingY => def.uv_tiling[1],
        _ => return None,
    })
}

/// Readouts and the swatch follow the LIBRARY, so they stay honest mid-drag
/// and after undo — neither of which rebuilds the rows.
pub(crate) fn sync_readouts(
    state: Res<MaterialEditorState>,
    library: Res<MaterialLibrary>,
    mut readouts: Query<(&FieldReadout, &mut Text)>,
    mut swatches: Query<&mut BackgroundColor, With<BaseColorSwatch>>,
    mut tracks: Query<(&Field, &mut bevy::feathers::controls::SliderBaseColor)>,
) {
    if !state.open || !(library.is_changed() || state.is_changed()) {
        return;
    }
    let Some(def) = state.target.and_then(|id| library.get(&id)) else {
        return;
    };
    for (readout, mut text) in &mut readouts {
        let Some(value) = field_value(def, readout.0) else {
            continue;
        };
        let next = format!("{value:.2}");
        if text.0 != next {
            text.0 = next;
        }
    }
    let base_color = Color::srgba(
        def.base_color[0],
        def.base_color[1],
        def.base_color[2],
        def.base_color[3],
    );
    let emissive_color = Color::srgb(def.emissive[0], def.emissive[1], def.emissive[2]);
    for mut swatch in &mut swatches {
        swatch.0 = base_color;
    }
    // Dragging red re-tints the green and blue tracks: each shows what its own
    // channel does to the color as it stands now.
    for (field, mut track) in &mut tracks {
        let want = match field {
            Field::BaseR | Field::BaseG | Field::BaseB | Field::BaseA => base_color,
            Field::EmissiveR | Field::EmissiveG | Field::EmissiveB => emissive_color,
            _ => continue,
        };
        if track.0 != want {
            track.0 = want;
        }
    }
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
pub(crate) struct PendingSeeds(Vec<Seed>);

pub(crate) struct Seed {
    entity: Entity,
    value: f32,
    min: f32,
    max: f32,
    /// Color sliders only: the OTHER channels, which decide the gradient the
    /// track paints. `SliderBaseColor` defaults to white, so leaving it unset
    /// paints the red channel cyan→white — a CMYK-looking track under an RGB
    /// label, showing white's axes instead of this material's.
    base: Option<Color>,
}

pub(crate) fn seed_slider_values(
    mut seeds: ResMut<PendingSeeds>,
    ready: Query<(), With<bevy::ui_widgets::SliderValue>>,
    entities: &bevy::ecs::entity::Entities,
    mut commands: Commands,
) {
    seeds.0.retain(|seed| {
        // Entities::contains sees RESERVED ids too — a component query here
        // would drop seeds queued this frame before their commands flushed.
        if !entities.contains(seed.entity) {
            return false; // rebuilt away before its template ever landed
        }
        if !ready.contains(seed.entity) {
            return true; // template not applied yet — keep waiting
        }
        // Immutable widget components: re-insert (which also fires the
        // Changed detection feathers' own sync systems key off).
        debug!(
            "seeding slider {:?} to {} in [{}, {}]",
            seed.entity, seed.value, seed.min, seed.max
        );
        let mut entity = commands.entity(seed.entity);
        entity.insert((
            bevy::ui_widgets::SliderValue(seed.value),
            bevy::ui_widgets::SliderRange::new(seed.min, seed.max),
            // Absent from the widget as spawned — and feathers' value/track
            // sync query REQUIRES it, so without this the display never
            // updates at all.
            bevy::ui_widgets::SliderPrecision(2),
        ));
        if let Some(base) = seed.base {
            entity.insert(bevy::feathers::controls::SliderBaseColor(base));
        }
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
        reg.action(
            ActionDef::new("material.rename", "Rename Material")
                .describe("Rename the open (or selected) material")
                .context("normal")
                .bind("space r"),
        );
        reg.action(
            ActionDef::new("material.duplicate", "Duplicate Material")
                .describe(
                    "Copy the open (or selected) material and edit the copy — \
                     the way a variant is actually made",
                )
                .context("normal")
                .bind("space shift+d"),
        );
        reg.action(
            // Opened by pressing a texture chip; listed so the palette and
            // which-key can still find it by name.
            ActionDef::new("material.pick-texture", "Pick Texture")
                .describe("Choose a texture for the slot the material panel is filling")
                .context("normal"),
        );
        reg.action(
            ActionDef::new("material.new-instance", "New Material Instance")
                .describe(
                    "Create a material that INHERITS from the open one — \
                     edit a field and only that field stops following the base",
                )
                .context("normal")
                .bind("space shift+i"),
        );
        reg.action(
            ActionDef::new("material.detach", "Detach Material")
                .describe("Bake the inherited values in and stop following the base")
                .context("normal"),
        );
        reg.action(
            // No binding, deliberately: removing a material is not undoable
            // through the asset history, so it is reached from the palette
            // where it has to be chosen by name rather than by muscle memory.
            ActionDef::new("material.delete", "Delete Material")
                .describe("Remove the open (or selected) material — refuses while it is in use")
                .context("normal"),
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
        Mesh3d(meshes.add(editor_scene::materials::primitive_mesh(Sphere::new(1.0)))),
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

/// `material.duplicate` and `material.delete`: the two verbs that made the
/// library one-way. Without duplicate, "this wall but greener" means building a
/// material from scratch; without delete, a mistyped material is permanent.
pub(crate) fn handle_material_library_verbs(
    mut reader: MessageReader<ActionInvoked>,
    mut state: ResMut<MaterialEditorState>,
    selection: Query<&editor_scene::materials::MaterialRef, With<Selected>>,
    references: Query<&editor_scene::materials::MaterialRef>,
    mut library: ResMut<MaterialLibrary>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        let action = invoked.action.as_str();
        if !matches!(
            action,
            "material.duplicate" | "material.delete" | "material.new-instance" | "material.detach"
        ) {
            continue;
        }
        // Same resolution as rename: the open material, else the selection's.
        let Some(id) = state
            .target
            .or_else(|| selection.iter().next().map(|reference| reference.0))
            .filter(|id| library.get(id).is_some())
        else {
            feedback.write(editor_scene::SceneIoFeedback {
                message: "no material — open one or select a shaded object".into(),
                success: false,
            });
            continue;
        };
        match action {
            "material.duplicate" => {
                let Some(source) = library.get(&id).cloned() else {
                    continue;
                };
                let copy = MaterialDef {
                    id: Uuid::new_v4(),
                    name: format!("{} copy", source.name),
                    ..source
                };
                let (new_id, name) = (copy.id, copy.name.clone());
                library.add(copy);
                // The COPY becomes the current material — duplicating is how a
                // variant starts, and every edit after it belongs to the variant.
                state.target = Some(new_id);
                state.refresh = true;
                feedback.write(editor_scene::SceneIoFeedback {
                    message: format!("duplicated \u{25c6} {name}"),
                    success: true,
                });
            }
            "material.new-instance" => {
                let Some(source) = library.get(&id).cloned() else {
                    continue;
                };
                // An instance starts owning NOTHING: it is its base until a
                // field is edited, and then it owns exactly that field.
                let instance = MaterialDef {
                    id: Uuid::new_v4(),
                    name: format!("{} instance", source.name),
                    base: Some(id),
                    overridden: std::collections::BTreeSet::new(),
                    ..source
                };
                let (new_id, name) = (instance.id, instance.name.clone());
                library.add(instance);
                state.target = Some(new_id);
                state.refresh = true;
                feedback.write(editor_scene::SceneIoFeedback {
                    message: format!("instance \u{25c6} {name}"),
                    success: true,
                });
            }
            "material.detach" => {
                let Some(resolved) = library.resolved(&id) else {
                    continue;
                };
                if resolved.base.is_none() {
                    feedback.write(editor_scene::SceneIoFeedback {
                        message: format!("{} follows nothing", resolved.name),
                        success: false,
                    });
                    continue;
                }
                // Flatten EXACTLY: keep what it looks like right now, then stop
                // following. Detaching must never change the render.
                if let Some(def) = library.get_mut(&id) {
                    *def = MaterialDef {
                        base: None,
                        overridden: std::collections::BTreeSet::new(),
                        ..resolved
                    };
                }
                state.refresh = true;
                feedback.write(editor_scene::SceneIoFeedback {
                    message: "detached — the values are its own now".into(),
                    success: true,
                });
            }
            "material.delete" => {
                // Refuse while anything still wears it: deleting a material out
                // from under a shaded object would leave it silently unpainted,
                // and there is no asset-history entry to undo this with.
                let children = library.children_of(&id);
                if !children.is_empty() {
                    feedback.write(editor_scene::SceneIoFeedback {
                        message: format!(
                            "{} material{} inherit from this one",
                            children.len(),
                            if children.len() == 1 { "" } else { "s" }
                        ),
                        success: false,
                    });
                    continue;
                }
                let users = references
                    .iter()
                    .filter(|reference| reference.0 == id)
                    .count();
                if users > 0 {
                    feedback.write(editor_scene::SceneIoFeedback {
                        message: format!(
                            "{} object{} still use this material",
                            users,
                            if users == 1 { "" } else { "s" }
                        ),
                        success: false,
                    });
                    continue;
                }
                let name = library
                    .remove(&id)
                    .map(|def| def.name)
                    .unwrap_or_else(|| "material".into());
                if state.target == Some(id) {
                    state.target = library.materials.first().map(|def| def.id);
                    state.refresh = true;
                }
                feedback.write(editor_scene::SceneIoFeedback {
                    message: format!("deleted {name}"),
                    success: true,
                });
            }
            _ => {}
        }
    }
}

/// `material.rename` through THE name prompt (the same surface prefab naming
/// uses). Targets the open editor's material, else the selection's — so it
/// works with the panel open or straight off a selected object.
pub(crate) fn collect_rename(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<MaterialEditorState>,
    selection: Query<&editor_scene::materials::MaterialRef, With<Selected>>,
    library: Res<MaterialLibrary>,
    mut prompt: ResMut<editor_prefabs::authoring::GroupPrompt>,
    mut target: ResMut<RenameTarget>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() != "material.rename" {
            continue;
        }
        let wanted = state
            .target
            .or_else(|| selection.iter().next().map(|r| r.0))
            .filter(|id| library.get(id).is_some());
        match wanted {
            Some(id) => {
                target.0 = Some(id);
                prompt.open = true;
                prompt.purpose = editor_prefabs::authoring::PromptPurpose::RenameMaterial;
            }
            None => {
                feedback.write(editor_scene::SceneIoFeedback {
                    message: "no material to rename — open one or select a shaded object".into(),
                    success: false,
                });
            }
        }
    }
}

/// Which material the open rename prompt is for.
#[derive(Resource, Default)]
pub(crate) struct RenameTarget(pub Option<Uuid>);

/// Apply the committed name. Goes through `edit_material`, so a rename lands in
/// the SAME asset history as every other material edit — Ctrl+Z takes it back.
pub(crate) fn apply_rename(
    prompt: Res<editor_prefabs::authoring::GroupPrompt>,
    mut commit: ResMut<editor_prefabs::authoring::GroupCommit>,
    mut target: ResMut<RenameTarget>,
    time: Res<Time>,
    mut library: ResMut<MaterialLibrary>,
    mut history: ResMut<MaterialHistory>,
    mut state: ResMut<MaterialEditorState>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    if prompt.purpose != editor_prefabs::authoring::PromptPurpose::RenameMaterial {
        return;
    }
    let Some(name) = commit.0.take() else { return };
    let Some(id) = target.0.take() else { return };
    let name = name.trim().to_string();
    if name.is_empty() {
        feedback.write(editor_scene::SceneIoFeedback {
            message: "a material needs a name".into(),
            success: false,
        });
        return;
    }
    edit_material(
        &mut library,
        &mut history,
        time.elapsed_secs_f64(),
        id,
        Field::Name,
        true,
        |def| def.name = name.clone(),
    );
    // The header shows the name — rebuild it.
    state.refresh = true;
    feedback.write(editor_scene::SceneIoFeedback {
        message: format!("renamed to {name}"),
        success: true,
    });
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
pub(crate) fn edit_material(
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
        // Editing a field CLAIMS it: from here on this material owns that value
        // and stops following its base for it. Nothing happens for a material
        // with no base, which is every material until someone makes a variant.
        if def.base.is_some()
            && let Some(claimed) = inherited_field(field)
        {
            def.overridden.insert(claimed);
        }
    }
}

/// Which inheritable field a panel control writes. `None` for controls that are
/// not inheritable at all — a rename is identity, not appearance.
fn inherited_field(field: Field) -> Option<editor_scene::materials::MaterialField> {
    use editor_scene::materials::MaterialField as M;
    Some(match field {
        Field::BaseR | Field::BaseG | Field::BaseB | Field::BaseA => M::BaseColor,
        Field::Metallic => M::Metallic,
        Field::Roughness => M::Roughness,
        Field::EmissiveR | Field::EmissiveG | Field::EmissiveB => M::Emissive,
        Field::EmissiveIntensity => M::EmissiveIntensity,
        Field::AlphaCutoff => M::AlphaCutoff,
        Field::AlphaMode => M::AlphaMode,
        Field::Unlit => M::Unlit,
        Field::DoubleSided => M::DoubleSided,
        Field::Texture(_) => M::Textures,
        Field::UvTilingX | Field::UvTilingY => M::UvTiling,
        Field::Name => return None,
    })
}

/// Slider/color-slider commits.
pub(crate) fn on_field_value(
    change: On<ValueChange<f32>>,
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
    // The widget reports the drag; moving the thumb is OURS to do (see
    // `bevy_ui_widgets::slider_self_update`). Without this write-back the
    // slider never visibly slides, however far the pointer travels.
    commands
        .entity(change.source)
        .insert(bevy::ui_widgets::SliderValue(value));
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
            // Tiling never reaches zero: a zero scale collapses every UV on
            // the surface into one texel.
            Field::UvTilingX => def.uv_tiling[0] = value.max(0.01),
            Field::UvTilingY => def.uv_tiling[1] = value.max(0.01),
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
/// Which texture slot the palette is currently picking for. Set when a texture
/// chip opens the picker, cleared when a choice lands.
#[derive(Resource, Default)]
pub(crate) struct PendingTextureSlot(pub Option<TextureSlot>);

/// The revert glyph on a row this material has CLAIMED: pressing it hands the
/// field back to the base.
#[derive(Component, Clone, Copy)]
pub(crate) struct RevertField(pub editor_scene::materials::MaterialField);

/// Give one claimed field back to the base. The value that appears is whatever
/// the base says NOW — reverting is not an undo, it is a change of ownership.
pub(crate) fn on_revert_press(
    press: On<Pointer<Press>>,
    reverts: Query<&RevertField>,
    time: Res<Time>,
    mut library: ResMut<MaterialLibrary>,
    mut history: ResMut<MaterialHistory>,
    mut editor: ResMut<MaterialEditorState>,
) {
    let Ok(revert) = reverts.get(press.entity) else {
        return;
    };
    let Some(id) = editor.target else { return };
    // Through the same history path as any other material edit, so one Ctrl+Z
    // puts the claim back.
    edit_material(
        &mut library,
        &mut history,
        time.elapsed_secs_f64(),
        id,
        Field::Name, // not an inheritable field: reverting must not re-claim
        true,
        |def| {
            def.overridden.remove(&revert.0);
        },
    );
    editor.refresh = true;
}

pub(crate) fn on_chip_press(
    press: On<Pointer<Press>>,
    fields: Query<&Field>,
    mut pending: ResMut<PendingTextureSlot>,
    mut actions: MessageWriter<ActionInvoked>,
    segments: Query<&AlphaModeChip>,
    time: Res<Time>,
    mut library: ResMut<MaterialLibrary>,
    mut history: ResMut<MaterialHistory>,
    mut editor: ResMut<MaterialEditorState>,
) {
    let Ok(field) = fields.get(press.entity) else {
        return;
    };
    debug!("material chip pressed: {field:?}");
    let Some(id) = editor.target else { return };
    // A texture chip OPENS THE PICKER. Cycling blindly through every imported
    // texture was tolerable with one slot and is not with five: you cannot see
    // what you are choosing, and finding one map means pressing a chip until
    // its name goes past. The palette already searches and previews.
    if let Field::Texture(slot) = field {
        pending.0 = Some(*slot);
        actions.write(ActionInvoked {
            action: ActionId::new_static("material.pick-texture"),
            args: None,
            source: InvocationSource::Palette,
        });
        return;
    }
    // A segment says exactly which mode it is; only the legacy single chip
    // cycles.
    let segment = segments.get(press.entity).ok().map(|chip| chip.0);
    edit_material(
        &mut library,
        &mut history,
        time.elapsed_secs_f64(),
        id,
        *field,
        true,
        // The alpha mode is the only chip that still edits in place; texture
        // chips returned above, having opened the picker instead.
        |def| {
            if matches!(field, Field::AlphaMode) {
                def.alpha_mode = segment.unwrap_or(match def.alpha_mode {
                    MaterialAlphaMode::Opaque => MaterialAlphaMode::Blend,
                    MaterialAlphaMode::Blend => MaterialAlphaMode::Mask,
                    MaterialAlphaMode::Mask => MaterialAlphaMode::Opaque,
                });
            }
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
    // The preview shows the RESOLVED material: an inherited value is still what
    // the surface looks like.
    let Some(def) = state.target.and_then(|id| library.resolved(&id)) else {
        return;
    };
    if let Some(mut material) = materials.get_mut(&rig.material) {
        *material = editor_scene::materials::to_standard_material(&def, &models, assets.as_deref());
    }
}

/// Root surface, spawned hidden once at startup.
pub(crate) fn spawn_editor_root(mut commands: Commands, fonts: Res<UiFonts>) {
    let root = commands
        .spawn((
            MaterialEditorRoot,
            FloatingSurface::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(480.0),
                top: px(48.0),
                bottom: px(56.0),
                width: px(340.0),
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
        })
        .id();
    // The scroll viewport: a relative wrapper (the scrollbar's frame of
    // reference) around the scrolling body — the same two-part shape every
    // docked panel uses, so one scrollbar recipe serves both.
    let wrapper = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_height: px(0.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    let body = commands
        .spawn((
            MaterialEditorBody,
            bevy::ui_widgets::ScrollArea,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(style::space::XS),
                flex_grow: 1.0,
                min_height: px(0.0),
                // Right padding keeps content clear of the scrollbar overlay.
                padding: UiRect::right(px(style::space::M)),
                // Rows carry explicit heights and never shrink (see
                // `row_wrapper`), so scrolling cannot collapse the widgets'
                // percent-sized tracks.
                overflow: bevy::ui::Overflow::scroll_y(),
                ..default()
            },
            ChildOf(wrapper),
        ))
        .id();
    crate::dock::spawn_scrollbar(&mut commands, wrapper, body);
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
    // VALUES come from the resolved material — an inherited value is still what
    // the surface looks like, and a slider showing a stale own-value would lie.
    // The CLAIMS come from what is stored, because that is what the editor
    // edits: which fields this material has taken ownership of.
    let Some(def) = state.target.and_then(|id| library.resolved(&id)) else {
        return;
    };
    let claims: Option<std::collections::BTreeSet<editor_scene::materials::MaterialField>> = state
        .target
        .and_then(|id| library.get(&id))
        .filter(|stored| stored.base.is_some())
        .map(|stored| stored.overridden.clone());
    for mut text in &mut title {
        if text.0 != def.name {
            text.0 = def.name.clone();
        }
    }
    let Ok(body) = body.single() else { return };
    commands.entity(body).despawn_related::<Children>();

    let texture_label = |slot: TextureSlot| -> String {
        def.texture(slot)
            .and_then(|uuid| models.get(&uuid).map(|e| e.name.clone()))
            .unwrap_or_else(|| "none".into())
    };

    // Widgets spawn via `commands` + ChildOf (bsn scenes have no child-spawner
    // entry point); plain rows use commands the same way for symmetry.
    if let Some(rig) = rig {
        // The preview sits on its own card: a sphere floating on the panel
        // background reads as an artifact, not a rendering.
        let stage = commands
            .spawn((
                Node {
                    height: px(96.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(style::radius::L)),
                    margin: UiRect::bottom(px(style::space::XS)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.086, 0.084, 0.080)),
                BorderColor::all(style::HAIRLINE),
                ChildOf(body),
            ))
            .id();
        commands.spawn((
            ImageNode::new(rig.image.clone()),
            Node {
                width: px(84.0),
                height: px(84.0),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(stage),
        ));
    }
    // A section header is a label plus a rule to the panel edge — grouping the
    // eye can follow without reading.
    let caption = |commands: &mut Commands, label: &str, swatch: bool| {
        let row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(style::space::S),
                    margin: UiRect::top(px(style::space::XS)),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(body),
            ))
            .id();
        commands.spawn((
            Text::new(label.to_string()),
            style::no_wrap(),
            style::sans_medium(&fonts, 10.0),
            TextColor(style::color::TEXT_DIM),
            ChildOf(row),
        ));
        commands.spawn((
            Node {
                flex_grow: 1.0,
                height: px(1.0),
                ..default()
            },
            BackgroundColor(style::HAIRLINE),
            ChildOf(row),
        ));
        if swatch {
            commands.spawn((
                BaseColorSwatch,
                Node {
                    width: px(22.0),
                    height: px(12.0),
                    flex_shrink: 0.0,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(style::radius::S)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor::all(style::HAIRLINE),
                ChildOf(row),
            ));
        }
    };
    /// The label gutter every row shares — one left edge down the panel.
    const GUTTER: f32 = 64.0;
    const READOUT: f32 = 34.0;
    let gutter_label = |commands: &mut Commands, fonts: &UiFonts, parent: Entity, label: &str| {
        commands.spawn((
            Text::new(label.to_string()),
            style::no_wrap(),
            style::sans(fonts, 11.0),
            TextColor(style::color::TEXT_KEYS),
            Node {
                width: px(GUTTER),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(parent),
        ));
    };
    // On a material that follows a base, the gutter says where each value comes
    // FROM: dimmed while it is the base's, and carrying a revert affordance
    // once this material has claimed it. Without this the inheritance is real
    // but invisible — the only way to tell was to read the file.
    let inheritance_label =
        |commands: &mut Commands, fonts: &UiFonts, parent: Entity, label: &str, field: Field| {
            let Some(claims) = claims.as_ref() else {
                gutter_label(commands, fonts, parent, label);
                return;
            };
            let Some(which) = inherited_field(field) else {
                gutter_label(commands, fonts, parent, label);
                return;
            };
            let claimed = claims.contains(&which);
            let slot = commands
                .spawn((
                    Node {
                        width: px(GUTTER),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        column_gap: px(style::space::XS),
                        ..default()
                    },
                    ChildOf(parent),
                ))
                .id();
            commands.spawn((
                Text::new(label.to_string()),
                style::no_wrap(),
                style::sans(fonts, 11.0),
                TextColor(if claimed {
                    style::color::TEXT_KEYS
                } else {
                    // Inherited: present, readable, plainly not this material's own.
                    style::color::TEXT_DIM
                }),
                ChildOf(slot),
            ));
            if claimed {
                // A claimed field can be given back. The glyph is the affordance —
                // a full button per row would drown a panel of sixteen rows.
                commands
                    .spawn((
                        RevertField(which),
                        Text::new("\u{21b6}".to_string()),
                        style::no_wrap(),
                        style::sans(fonts, 11.0),
                        TextColor(style::color::accent()),
                        ChildOf(slot),
                    ))
                    .observe(on_revert_press);
            }
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
    // Color channels get the same gutter as the sliders, plus the numeric
    // readout feathers' color slider does not draw — dragging blind was the
    // worst of the old surface.
    let color_row = |commands: &mut Commands,
                     fonts: &UiFonts,
                     label: &str,
                     field: Field,
                     channel: ColorChannel,
                     value: f32,
                     base: Color| {
        let wrapper = row_wrapper(commands, 22.0);
        inheritance_label(commands, fonts, wrapper, label, field);
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
            .spawn_scene(bsn! {
                @FeathersColorSlider {
                    @value: {value},
                    @channel: {channel},
                }
            })
            .insert((field, ChildOf(slot)))
            .observe(on_field_value)
            .id();
        commands.spawn((
            FieldReadout(field),
            Text::new(format!("{value:.2}")),
            style::no_wrap(),
            style::mono(fonts, 10.0),
            TextColor(style::color::TEXT_DIM),
            Node {
                width: px(READOUT),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(wrapper),
        ));
        seeds.borrow_mut().push(Seed {
            entity: widget,
            value,
            min: 0.0,
            max: 1.0,
            base: Some(base),
        });
    };
    let slider_row = |commands: &mut Commands,
                      fonts: &UiFonts,
                      label: &str,
                      field: Field,
                      value: f32,
                      max: f32| {
        let wrapper = row_wrapper(commands, 24.0);
        inheritance_label(commands, fonts, wrapper, label, field);
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
        seeds.borrow_mut().push(Seed {
            entity: widget,
            value,
            min: 0.0,
            max,
            base: None,
        });
    };
    // A chip in the gutter-aligned row shape, so every control lines up.
    let chip_in = |commands: &mut Commands,
                   fonts: &UiFonts,
                   parent: Entity,
                   field: Field,
                   label: String,
                   selected: bool| {
        let (background, border, text) = if selected {
            (
                Color::srgba(1.0, 1.0, 1.0, 0.10),
                style::color::accent(),
                style::color::TEXT_BRIGHT,
            )
        } else {
            (
                style::color::CHIP_REST,
                style::HAIRLINE,
                style::color::TEXT_KEYS,
            )
        };
        let chip_entity = commands
            .spawn((
                field,
                Node {
                    padding: UiRect::axes(px(style::space::S), px(style::space::XS)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(style::radius::S)),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(border),
                BackgroundColor(background),
                ChildOf(parent),
            ))
            .observe(on_chip_press)
            .id();
        commands.spawn((
            Text::new(label),
            style::no_wrap(),
            style::sans(fonts, 11.0),
            TextColor(text),
            ChildOf(chip_entity),
        ));
        chip_entity
    };
    let toggle = |commands: &mut Commands, fonts: &UiFonts, label: &str, field: Field, on: bool| {
        let row = row_wrapper(commands, 24.0);
        inheritance_label(commands, fonts, row, label, field);
        let mut switch = commands.spawn_scene(bsn! { @FeathersToggleSwitch });
        switch.insert((field, ChildOf(row)));
        if on {
            switch.insert(Checked);
        }
        switch.observe(on_field_toggle);
    };

    // Each channel track paints THIS material's axis (the other channels held
    // constant) — not white's.
    let base_color = Color::srgba(
        def.base_color[0],
        def.base_color[1],
        def.base_color[2],
        def.base_color[3],
    );
    let emissive_color = Color::srgb(def.emissive[0], def.emissive[1], def.emissive[2]);

    caption(&mut commands, "BASE COLOR", true);
    color_row(
        &mut commands,
        &fonts,
        "red",
        Field::BaseR,
        ColorChannel::Red,
        def.base_color[0],
        base_color,
    );
    color_row(
        &mut commands,
        &fonts,
        "green",
        Field::BaseG,
        ColorChannel::Green,
        def.base_color[1],
        base_color,
    );
    color_row(
        &mut commands,
        &fonts,
        "blue",
        Field::BaseB,
        ColorChannel::Blue,
        def.base_color[2],
        base_color,
    );
    color_row(
        &mut commands,
        &fonts,
        "alpha",
        Field::BaseA,
        ColorChannel::Alpha,
        def.base_color[3],
        base_color,
    );
    caption(&mut commands, "SURFACE", false);
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
    caption(&mut commands, "EMISSIVE", false);
    color_row(
        &mut commands,
        &fonts,
        "red",
        Field::EmissiveR,
        ColorChannel::Red,
        def.emissive[0],
        emissive_color,
    );
    color_row(
        &mut commands,
        &fonts,
        "green",
        Field::EmissiveG,
        ColorChannel::Green,
        def.emissive[1],
        emissive_color,
    );
    color_row(
        &mut commands,
        &fonts,
        "blue",
        Field::EmissiveB,
        ColorChannel::Blue,
        def.emissive[2],
        emissive_color,
    );
    slider_row(
        &mut commands,
        &fonts,
        "intensity",
        Field::EmissiveIntensity,
        def.emissive_intensity,
        10.0,
    );
    caption(&mut commands, "ALPHA", false);
    // Segmented control: all three modes visible, the live one marked. A
    // one-chip cycle hid both which modes exist and where a click would land.
    {
        let row = row_wrapper(&mut commands, 24.0);
        inheritance_label(&mut commands, &fonts, row, "mode", Field::AlphaMode);
        for (mode, label) in [
            (MaterialAlphaMode::Opaque, "opaque"),
            (MaterialAlphaMode::Blend, "blend"),
            (MaterialAlphaMode::Mask, "mask"),
        ] {
            let chip_entity = chip_in(
                &mut commands,
                &fonts,
                row,
                Field::AlphaMode,
                label.into(),
                def.alpha_mode == mode,
            );
            commands.entity(chip_entity).insert(AlphaModeChip(mode));
        }
    }
    slider_row(
        &mut commands,
        &fonts,
        "cutoff",
        Field::AlphaCutoff,
        def.alpha_cutoff,
        1.0,
    );
    caption(&mut commands, "FLAGS", false);
    toggle(&mut commands, &fonts, "unlit", Field::Unlit, def.unlit);
    toggle(
        &mut commands,
        &fonts,
        "2-sided",
        Field::DoubleSided,
        def.double_sided,
    );
    caption(&mut commands, "TEXTURES", false);
    for slot in TextureSlot::ALL {
        let row = row_wrapper(&mut commands, 24.0);
        inheritance_label(
            &mut commands,
            &fonts,
            row,
            slot.label(),
            Field::Texture(slot),
        );
        chip_in(
            &mut commands,
            &fonts,
            row,
            Field::Texture(slot),
            texture_label(slot),
            def.texture(slot).is_some(),
        );
    }
    caption(&mut commands, "TILING", false);
    for (field, label, value) in [
        (Field::UvTilingX, "u", def.uv_tiling[0]),
        (Field::UvTilingY, "v", def.uv_tiling[1]),
    ] {
        slider_row(&mut commands, &fonts, label, field, value, 8.0);
    }
    pending.0.extend(seeds.into_inner());
}
