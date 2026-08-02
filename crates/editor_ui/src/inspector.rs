//! Inspector panel (M3-C3): ONE recursive reflection editor over the selection —
//! no per-component snapshot structs, ever (spec §7). Two phases: an exclusive
//! collect pass reflects the selected entity's registered components into a plain
//! `InspectorModel`, and a render pass builds widgets from it (kit-composed
//! feathers controls). Every completed field edit commits ONE `EditScope` `Set`
//! transaction — the same path as spawn/load/undo — so field edits are undoable
//! by construction.
//!
//! Custom editors are a REGISTRY (`InspectorOverrides`: `TypeId → collect fn`),
//! not a hardcoded match — generic reflection is the fallback, never the ceiling
//! (owner: some components want explicit editors). Defaults registered here:
//! `Transform` as Position / Rotation-in-Euler-degrees / Scale, `Name` as a label.
//!
//! Number fields are BOTH typing-editable and drag-slideable (owner): typing
//! commits on Enter/blur as one undo entry; horizontal dragging live-edits the
//! scene through gesture-tagged transactions that coalesce into one undo entry —
//! the exact machinery the viewport move gesture uses.

use bevy::feathers::controls::{
    FeathersCheckbox, FeathersNumberInput, FeathersTextInput, FeathersTextInputContainer,
    NumberInputValue, UpdateNumberInput,
};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::reflect::{ParsedPath, ReflectPath, ReflectRef};
use bevy::ui::px;
use bevy::ui_widgets::ValueChange;
use editor_core::edits::EditorComponents;
use editor_core::prelude::*;
use std::any::TypeId;

use crate::dock::PanelBody;
use crate::style::{self, UiFonts};

pub(crate) const INSPECTOR_PANEL: &str = "inspector";

/// What a number field edits when it commits.
#[derive(Component, Clone)]
pub(crate) struct InspectorField {
    pub target: SceneId,
    pub type_path: &'static str,
    /// Reflect path within the component ("translation.x"). Unused for Euler.
    pub path: String,
    pub kind: FieldKind,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum FieldKind {
    /// `path` names an `f32` leaf.
    Direct,
    /// Euler-degrees editing of `Transform.rotation` (axis 0/1/2, XYZ order).
    EulerDeg(usize),
    /// `path` names a `bool` leaf.
    Bool,
    /// `path` names a `String` leaf.
    Str,
    /// The whole component is a `Name` (hash must be recomputed on set).
    NameText,
}

/// The typed payload a commit carries (matched against `FieldKind`).
#[derive(Clone)]
pub(crate) enum FieldNewValue {
    F32(f32),
    Bool(bool),
    Text(String),
}

/// The field's current value, refreshed each rebuild — drag starts from here.
#[derive(Component, Clone, Copy)]
pub(crate) struct FieldValue(pub f32);

/// Live drag state on a number field (slide-editing).
#[derive(Component)]
pub(crate) struct FieldDrag {
    start: f32,
    gesture: u64,
}

pub(crate) struct NumberSpec {
    pub value: f32,
    /// Axis index (0/1/2) selects the upstream X/Y/Z sigil styling; None = plain.
    pub axis: Option<usize>,
    pub field: InspectorField,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum GroupKind {
    Tags,
    ReadOnly,
}

/// Collapsed/expanded state of the noise-reduction groups (persists across
/// rebuilds and selections).
#[derive(Resource, Default)]
pub(crate) struct InspectorGroups {
    pub tags_open: bool,
    pub readonly_open: bool,
}

pub(crate) enum RowSpec {
    /// WHAT is selected: entity name + id (+ multi-select count) — always first.
    Header { title: String, detail: String },
    /// Collapsible group header (Tags / Read-only).
    GroupHeader { title: String, count: usize, open: bool, group: GroupKind },
    /// Compact name chips (group contents).
    Chips(Vec<String>),
    Section(String),
    Triple { label: String, fields: Vec<NumberSpec> },
    Number { label: String, field: NumberSpec },
    Toggle { label: String, value: bool, field: InspectorField },
    TextField { label: String, value: String, field: InspectorField },
    ReadOnly { label: String, value: String },
}

/// The collected view of the selection — plain data between the two phases.
#[derive(Resource, Default)]
pub(crate) struct InspectorModel {
    pub rows: Vec<RowSpec>,
    /// Bumped whenever `rows` is rebuilt (drives the render pass).
    pub generation: u64,
    pub dirty: bool,
}

/// `TypeId → collect fn` (spec §7's type-override registry): returns true if it
/// produced rows for the component. Feature crates will extend this through the
/// `editor_api` ui surface; editor defaults register at plugin build.
pub(crate) type CollectOverride =
    fn(SceneId, &'static str, &dyn PartialReflect, &mut Vec<RowSpec>) -> bool;

#[derive(Resource, Default)]
pub(crate) struct InspectorOverrides(pub Vec<(TypeId, CollectOverride)>);

pub(crate) fn default_overrides() -> InspectorOverrides {
    InspectorOverrides(vec![
        (TypeId::of::<Transform>(), collect_transform),
        (TypeId::of::<Name>(), collect_name),
    ])
}

fn focus_inside_inspector(world: &World) -> bool {
    let Some(focus) = world.get_resource::<InputFocus>().and_then(|f| f.get()) else {
        return false;
    };
    let mut current = focus;
    loop {
        if let Some(body) = world.get::<PanelBody>(current) {
            return body.0.as_str() == INSPECTOR_PANEL;
        }
        match world.get::<ChildOf>(current) {
            Some(parent) => current = parent.parent(),
            None => return false,
        }
    }
}

/// Exclusive collect pass: reflect the (first) selected entity's registered
/// components into the model. Suppressed while a field is focused or a slide-drag
/// is live (a rebuild would despawn the widget mid-interaction).
pub(crate) fn collect_inspector(world: &mut World) {
    let dirty = world.resource::<InspectorModel>().dirty;
    if !dirty || focus_inside_inspector(world) {
        return;
    }
    if world.query::<&FieldDrag>().iter(world).next().is_some() {
        return;
    }
    world.resource_mut::<InspectorModel>().dirty = false;

    let selected: Vec<(Entity, SceneId)> = {
        let mut query = world.query_filtered::<(Entity, &SceneId), With<Selected>>();
        let mut all: Vec<(Entity, SceneId)> =
            query.iter(world).map(|(e, id)| (e, *id)).collect();
        all.sort_by_key(|(_, id)| id.0);
        all
    };

    let mut rows = Vec::new();
    if let Some((entity, target)) = selected.first().copied() {
        let title = world
            .get::<Name>(entity)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "entity".to_string());
        let detail = if selected.len() > 1 {
            format!(
                "{} selected · editing first · {}",
                selected.len(),
                &target.0.to_string()[..8]
            )
        } else {
            target.0.to_string()[..8].to_string()
        };
        rows.push(RowSpec::Header { title, detail });

        let registry = world.resource::<AppTypeRegistry>().clone();
        let registered = world.resource::<EditorComponents>().types.clone();
        let overrides = world.resource::<InspectorOverrides>().0.clone();
        let registry = registry.read();

        // The FULL archetype (owner: see and edit ALL components unless marked):
        // every reflectable component on the entity, registered (serialized) types
        // first in registration order, the rest alphabetically. A small policy
        // keeps chrome out: editor-internal markers hide; derived/asset-handle
        // components render read-only.
        const HIDDEN: &[&str] = &[
            "SceneId",
            "Selected",
            "MeshOutline",
            "HasSilhouetteMesh",
            "GhostApplied",
            "InsertPreview",
            "PreviewEntity",
            "ChildOf",
            "Children",
            "TabOrdered",
        ];
        const READ_ONLY: &[&str] = &[
            "GlobalTransform",
            "InheritedVisibility",
            "ViewVisibility",
            "Mesh3d",
            "MeshMaterial3d",
            "Aabb",
        ];
        let mut present: Vec<(TypeId, &'static str)> = Vec::new();
        for &component_id in world.entity(entity).archetype().components() {
            let Some(info) = world.components().get_info(component_id) else { continue };
            let Some(type_id) = info.type_id() else { continue };
            let Some(registration) = registry.get(type_id) else { continue };
            if registration.data::<bevy::ecs::reflect::ReflectComponent>().is_none() {
                continue;
            }
            let type_path = registration.type_info().type_path();
            let short = type_path.rsplit("::").next().unwrap_or(type_path);
            if HIDDEN.contains(&short) {
                continue;
            }
            present.push((type_id, type_path));
        }
        present.sort_by_key(|(type_id, type_path)| {
            let registered_index =
                registered.iter().position(|r| r.type_id == *type_id).unwrap_or(usize::MAX);
            let short = type_path.rsplit("::").next().unwrap_or(type_path).to_string();
            (registered_index, short)
        });

        let groups = world.resource::<InspectorGroups>();
        let (tags_open, readonly_open) = (groups.tags_open, groups.readonly_open);
        let mut has_name = false;
        let mut tags: Vec<String> = Vec::new();
        let mut readonly: Vec<String> = Vec::new();
        for (type_id, type_path) in present {
            let Some(registration) = registry.get(type_id) else { continue };
            let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            else {
                continue;
            };
            let Some(value) = reflect_component.reflect(world.entity(entity)) else {
                continue;
            };
            let value = value.as_partial_reflect();
            let short = type_path.rsplit("::").next().unwrap_or(type_path);
            has_name |= short == "Name";
            // Policy read-only components go straight to the Read-only group.
            if READ_ONLY.contains(&short) {
                readonly.push(short.to_string());
                continue;
            }
            let mut component_rows = Vec::new();
            let handled = overrides
                .iter()
                .find(|(id, _)| *id == type_id)
                .is_some_and(|(_, f)| f(target, type_path, value, &mut component_rows));
            if !handled {
                walk_fields(target, type_path, "", value, &mut component_rows);
            }
            if component_rows.is_empty() {
                // Field-less marker: a TAG — name only, no section (owner).
                tags.push(short.to_string());
            } else if component_rows.iter().all(|r| matches!(r, RowSpec::ReadOnly { .. })) {
                // Nothing editable in it: Read-only group, name only (owner).
                readonly.push(short.to_string());
            } else {
                rows.push(RowSpec::Section(short.to_uppercase()));
                rows.append(&mut component_rows);
            }
        }
        tags.sort();
        readonly.sort();
        if !tags.is_empty() {
            rows.push(RowSpec::GroupHeader {
                title: "TAGS".into(),
                count: tags.len(),
                open: tags_open,
                group: GroupKind::Tags,
            });
            if tags_open {
                rows.push(RowSpec::Chips(tags));
            }
        }
        if !readonly.is_empty() {
            rows.push(RowSpec::GroupHeader {
                title: "READ-ONLY".into(),
                count: readonly.len(),
                open: readonly_open,
                group: GroupKind::ReadOnly,
            });
            if readonly_open {
                rows.push(RowSpec::Chips(readonly));
            }
        }
        // Always offer a Name (owner: the field must exist even before the
        // component does — committing INSERTS it via the same Set path).
        if !has_name {
            rows.push(RowSpec::Section("NAME".into()));
            rows.push(RowSpec::TextField {
                label: "Name".into(),
                value: String::new(),
                field: InspectorField {
                    target,
                    type_path: "bevy_ecs::name::Name",
                    path: String::new(),
                    kind: FieldKind::NameText,
                },
            });
        }
    }
    let mut model = world.resource_mut::<InspectorModel>();
    model.rows = rows;
    model.generation += 1;
}

fn triple(
    target: SceneId,
    type_path: &'static str,
    label: &str,
    base_path: &str,
    values: Vec3,
) -> RowSpec {
    RowSpec::Triple {
        label: label.to_string(),
        fields: ["x", "y", "z"]
            .iter()
            .enumerate()
            .map(|(i, axis)| NumberSpec {
                value: values[i],
                axis: Some(i),
                field: InspectorField {
                    target,
                    type_path,
                    path: format!("{base_path}.{axis}"),
                    kind: FieldKind::Direct,
                },
            })
            .collect(),
    }
}

fn collect_transform(
    target: SceneId,
    type_path: &'static str,
    value: &dyn PartialReflect,
    rows: &mut Vec<RowSpec>,
) -> bool {
    let Some(transform) = value.try_downcast_ref::<Transform>() else { return false };
    rows.push(triple(target, type_path, "Position", "translation", transform.translation));
    let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
    let degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    rows.push(RowSpec::Triple {
        label: "Rotation °".to_string(),
        fields: (0..3)
            .map(|axis| NumberSpec {
                value: degrees[axis],
                axis: Some(axis),
                field: InspectorField {
                    target,
                    type_path,
                    path: String::new(),
                    kind: FieldKind::EulerDeg(axis),
                },
            })
            .collect(),
    });
    rows.push(triple(target, type_path, "Scale", "scale", transform.scale));
    true
}

fn collect_name(
    target: SceneId,
    type_path: &'static str,
    value: &dyn PartialReflect,
    rows: &mut Vec<RowSpec>,
) -> bool {
    let Some(name) = value.try_downcast_ref::<Name>() else { return false };
    rows.push(RowSpec::TextField {
        label: "Name".into(),
        value: name.as_str().to_string(),
        field: InspectorField {
            target,
            type_path,
            path: String::new(),
            kind: FieldKind::NameText,
        },
    });
    true
}

/// Generic recursive walk: f32 leaves become editable numbers; Vec3 structs become
/// triples; bools/strings/enums render read-only until their widgets land.
fn walk_fields(
    target: SceneId,
    type_path: &'static str,
    prefix: &str,
    value: &dyn PartialReflect,
    rows: &mut Vec<RowSpec>,
) {
    match value.reflect_ref() {
        ReflectRef::Struct(s) => {
            if let Some(vec3) = value.try_downcast_ref::<Vec3>() {
                let label = prefix.rsplit('.').next().unwrap_or(prefix).to_string();
                rows.push(triple(target, type_path, &label, prefix, *vec3));
                return;
            }
            for i in 0..s.field_len() {
                let name = s.name_at(i).unwrap_or_default().to_string();
                let path =
                    if prefix.is_empty() { name.clone() } else { format!("{prefix}.{name}") };
                let Some(field) = s.field_at(i) else { continue };
                walk_fields(target, type_path, &path, field, rows);
            }
        }
        ReflectRef::Enum(e) => {
            rows.push(RowSpec::ReadOnly {
                label: leaf_label(prefix),
                value: e.variant_name().to_string(),
            });
        }
        _ => {
            if let Some(v) = value.try_downcast_ref::<f32>() {
                rows.push(RowSpec::Number {
                    label: leaf_label(prefix),
                    field: NumberSpec {
                        value: *v,
                        axis: None,
                        field: InspectorField {
                            target,
                            type_path,
                            path: prefix.to_string(),
                            kind: FieldKind::Direct,
                        },
                    },
                });
            } else if let Some(v) = value.try_downcast_ref::<bool>() {
                rows.push(RowSpec::Toggle {
                    label: leaf_label(prefix),
                    value: *v,
                    field: InspectorField {
                        target,
                        type_path,
                        path: prefix.to_string(),
                        kind: FieldKind::Bool,
                    },
                });
            } else if let Some(v) = value.try_downcast_ref::<String>() {
                rows.push(RowSpec::TextField {
                    label: leaf_label(prefix),
                    value: v.clone(),
                    field: InspectorField {
                        target,
                        type_path,
                        path: prefix.to_string(),
                        kind: FieldKind::Str,
                    },
                });
            }
        }
    }
}

fn leaf_label(path: &str) -> String {
    path.rsplit('.').next().unwrap_or(path).to_string()
}

/// TEMP diagnostic (INSPECTOR_PROBE=1): drive menu -> level -> select, log model.
pub(crate) fn probe_inspector(
    mut frames: Local<u32>,
    mut writer: MessageWriter<ActionInvoked>,
    mut changed: MessageWriter<SelectionChanged>,
    mut key_events: MessageWriter<bevy::input::keyboard::KeyboardInput>,
    window: Query<Entity, With<bevy::window::PrimaryWindow>>,
    scene: Query<Entity, With<SceneId>>,
    model: Res<InspectorModel>,
    body: Query<(Entity, &PanelBody)>,
    children: Query<&Children>,
    mut selected_once: Local<bool>,
    name_field: Query<(Entity, &InspectorField)>,
    children_q: Query<&Children>,
    editable_q: Query<&bevy::text::EditableText>,
    mut focus_res: ResMut<InputFocus>,
    mut commands: Commands,
) {
    *frames += 1;
    if *frames == 60 || *frames == 62 {
        // Leave the main menu with a REAL key event (ButtonInput.press is cleared
        // by the input plugin before game systems see just_pressed).
        if let Ok(window) = window.single() {
            key_events.write(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: bevy::input::keyboard::Key::Enter,
                state: if *frames == 60 {
                    bevy::input::ButtonState::Pressed
                } else {
                    bevy::input::ButtonState::Released
                },
                text: None,
                repeat: false,
                window,
            });
        }
    }
    if *frames == 90 {
        writer.write(ActionInvoked {
            action: ActionId::new_static("core.toggle-editor"),
            args: None,
            source: InvocationSource::Test,
        });
    }
    if *frames > 120 && !*selected_once {
        if let Some(entity) = scene.iter().next() {
            commands.entity(entity).insert(Selected);
            changed.write(SelectionChanged);
            *selected_once = true;
            info!("PROBE selected {entity:?}");
        }
    }
    if *frames == 240 {
        // Drive the Name field: focus inner editable, then type via key events.
        let name_container = name_field
            .iter()
            .find(|(_, f)| f.kind == FieldKind::NameText)
            .map(|(e, _)| e);
        if let Some((container, inner)) = name_container.and_then(|c| {
            find_editable(&children_q, &editable_q, c).map(|inner| (c, inner))
        }) {
            info!("PROBE name field container={container:?} inner={inner:?}");
            focus_res.set(inner, bevy::input_focus::FocusCause::Navigated);
        } else {
            info!("PROBE no name field found");
        }
    }
    if *frames == 270 || *frames == 272 {
        if let Ok(window) = window.single() {
            key_events.write(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::KeyZ,
                logical_key: bevy::input::keyboard::Key::Character("z".into()),
                state: if *frames == 270 {
                    bevy::input::ButtonState::Pressed
                } else {
                    bevy::input::ButtonState::Released
                },
                text: (*frames == 270).then(|| "z".into()),
                repeat: false,
                window,
            });
        }
    }
    if *frames == 300 {
        for (container, field) in name_field.iter() {
            if field.kind != FieldKind::NameText {
                continue;
            }
            if let Some(inner) = find_editable(&children_q, &editable_q, container) {
                if let Ok(text) = editable_q.get(inner) {
                    info!("PROBE name field text now: {:?}", text.value().to_string());
                }
            }
        }
    }
    if *frames > 150 && *frames % 60 == 0 {
        let body_children = body
            .iter()
            .find(|(_, b)| b.0.as_str() == INSPECTOR_PANEL)
            .and_then(|(e, _)| children.get(e).ok().map(|c| c.len()));
        info!(
            "PROBE rows={} gen={} dirty={} body_children={:?}",
            model.rows.len(),
            model.generation,
            model.dirty,
            body_children
        );
    }
}

fn find_editable(
    children: &Query<&Children>,
    editable: &Query<&bevy::text::EditableText>,
    root: Entity,
) -> Option<Entity> {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if editable.get(entity).is_ok() {
            return Some(entity);
        }
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
    }
    None
}

/// Anything that changes the selection or the scene marks the inspector dirty.
pub(crate) fn watch_inspector_inputs(
    mut edited: MessageReader<Edited>,
    mut selection: MessageReader<SelectionChanged>,
    state: Res<EditorState>,
    mut model: ResMut<InspectorModel>,
) {
    if edited.read().next().is_some()
        || selection.read().next().is_some()
        || state.is_changed()
    {
        model.dirty = true;
    }
}

/// Render pass: rebuild the widget tree whenever the model generation advances.
pub(crate) fn render_inspector(
    model: Res<InspectorModel>,
    mut last_generation: Local<u64>,
    body: Query<(Entity, &PanelBody)>,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    if model.generation == *last_generation {
        return;
    }
    *last_generation = model.generation;
    let Some((body_entity, _)) =
        body.iter().find(|(_, b)| b.0.as_str() == INSPECTOR_PANEL)
    else {
        return;
    };
    let ui = settings.ui.clone();
    commands.entity(body_entity).despawn_related::<Children>();

    if model.rows.is_empty() {
        commands.entity(body_entity).with_children(|body| {
            body.spawn((
                Text::new("no selection — click an entity"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
        });
        return;
    }

    for spec in &model.rows {
        match spec {
            RowSpec::Header { title, detail } => {
                let header = commands
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(1.0),
                        flex_shrink: 0.0,
                        margin: UiRect::bottom(px(style::space::XS)),
                        ..default()
                    })
                    .id();
                commands.entity(header).insert(ChildOf(body_entity));
                let title_id = commands
                    .spawn((
                        Text::new(title.clone()),
                        style::sans_medium(&fonts, ui.font_size_m),
                        TextColor(style::color::accent()),
                    ))
                    .id();
                commands.entity(title_id).insert(ChildOf(header));
                let detail_id = commands
                    .spawn((
                        Text::new(detail.clone()),
                        style::mono(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                    ))
                    .id();
                commands.entity(detail_id).insert(ChildOf(header));
            }
            RowSpec::GroupHeader { title, count, open, group } => {
                let glyph = if *open { "▾" } else { "▸" };
                let group = *group;
                let header = commands
                    .spawn((
                        Node {
                            margin: UiRect::top(px(style::space::S)),
                            padding: UiRect::axes(px(2.0), px(2.0)),
                            column_gap: px(style::space::XS),
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(px(style::radius::S)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .observe(
                        move |_press: On<Pointer<Press>>,
                              mut groups: ResMut<InspectorGroups>,
                              mut model: ResMut<InspectorModel>| {
                            match group {
                                GroupKind::Tags => groups.tags_open = !groups.tags_open,
                                GroupKind::ReadOnly => {
                                    groups.readonly_open = !groups.readonly_open
                                }
                            }
                            model.dirty = true;
                        },
                    )
                    .id();
                commands.entity(header).insert(ChildOf(body_entity));
                let text = commands
                    .spawn((
                        Text::new(format!("{glyph} {title} ({count})")),
                        style::sans_medium(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                    ))
                    .id();
                commands.entity(text).insert(ChildOf(header));
            }
            RowSpec::Chips(names) => {
                let wrap = commands
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(style::space::XS),
                        row_gap: px(style::space::XS),
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .id();
                commands.entity(wrap).insert(ChildOf(body_entity));
                for name in names {
                    let chip = commands
                        .spawn((
                            Node {
                                padding: UiRect::axes(px(style::space::XS), px(1.0)),
                                border_radius: BorderRadius::all(px(style::radius::S)),
                                ..default()
                            },
                            BackgroundColor(style::color::selection()),
                        ))
                        .id();
                    commands.entity(chip).insert(ChildOf(wrap));
                    let text = commands
                        .spawn((
                            Text::new(name.clone()),
                            style::mono(&fonts, ui.font_size_xs),
                            TextColor(style::color::TEXT_KEYS),
                        ))
                        .id();
                    commands.entity(text).insert(ChildOf(chip));
                }
            }
            RowSpec::Section(title) => {
                let header = commands
                    .spawn((
                        Node {
                            margin: UiRect::top(px(style::space::S)),
                            padding: UiRect::bottom(px(2.0)),
                            border: UiRect::bottom(px(1.0)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BorderColor::all(style::HAIRLINE),
                    ))
                    .id();
                commands.entity(header).insert(ChildOf(body_entity));
                let text = commands
                    .spawn((
                        Text::new(title.clone()),
                        style::sans_medium(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                    ))
                    .id();
                commands.entity(text).insert(ChildOf(header));
            }
            RowSpec::Triple { label, fields } => {
                let row = spawn_labeled_row(&mut commands, body_entity, label, &fonts, &ui);
                for spec in fields {
                    spawn_number_field(&mut commands, row, spec);
                }
            }
            RowSpec::Number { label, field } => {
                let row = spawn_labeled_row(&mut commands, body_entity, label, &fonts, &ui);
                spawn_number_field(&mut commands, row, field);
            }
            RowSpec::Toggle { label, value, field } => {
                let row = spawn_labeled_row(&mut commands, body_entity, label, &fonts, &ui);
                let checkbox = commands.spawn_scene(bsn! { @FeathersCheckbox }).id();
                commands
                    .entity(checkbox)
                    .insert((field.clone(), ChildOf(row)))
                    .observe(commit_bool);
                if *value {
                    commands.entity(checkbox).insert(bevy::ui::Checked);
                }
            }
            RowSpec::TextField { label, value, field } => {
                let row = spawn_labeled_row(&mut commands, body_entity, label, &fonts, &ui);
                let container = commands
                    .spawn_scene(bsn! {
                        @FeathersTextInputContainer
                        Node { flex_grow: 1.0 }
                        Children [
                            (
                                @FeathersTextInput
                                bevy::ui_widgets::SelectAllOnFocus
                            )
                        ]
                    })
                    .id();
                commands
                    .entity(container)
                    .insert((field.clone(), ChildOf(row)))
                    .observe(commit_text_on_enter);
                // Seed the inner editable with the current value post-spawn.
                let seed = value.clone();
                commands.queue(move |world: &mut World| {
                    if let Some(inner) = find_editable_descendant(world, container) {
                        world
                            .entity_mut(inner)
                            .insert(bevy::text::EditableText::new(seed));
                    }
                });
            }
            RowSpec::ReadOnly { label, value } => {
                let row = spawn_labeled_row(&mut commands, body_entity, label, &fonts, &ui);
                let text = commands
                    .spawn((
                        Text::new(value.clone()),
                        style::sans(&fonts, ui.font_size_s),
                        TextColor(style::color::TEXT_KEYS),
                    ))
                    .id();
                commands.entity(text).insert(ChildOf(row));
            }
        }
    }
}

/// F6 rule baked in once: label ABOVE controls, controls in an even row.
fn spawn_labeled_row(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    fonts: &UiFonts,
    ui: &editor_core::settings::UiSettings,
) -> Entity {
    let container = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(2.0),
            flex_shrink: 0.0,
            ..default()
        })
        .id();
    commands.entity(container).insert(ChildOf(parent));
    let text = commands
        .spawn((
            Text::new(label.to_string()),
            style::sans(fonts, ui.font_size_xs),
            TextColor(style::color::TEXT_DIM),
        ))
        .id();
    commands.entity(text).insert(ChildOf(container));
    let controls = commands
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(style::space::XS),
            align_items: AlignItems::Stretch,
            ..default()
        })
        .id();
    commands.entity(controls).insert(ChildOf(container));
    controls
}

fn spawn_number_field(commands: &mut Commands, parent: Entity, spec: &NumberSpec) {
    // Equal-width slot; the feathers container stretches to fill it.
    let slot = commands
        .spawn(Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            ..default()
        })
        .id();
    commands.entity(slot).insert(ChildOf(parent));

    // Upstream axis styling: the feathers X/Y/Z sigil tokens (owner call).
    let entity = match spec.axis {
        Some(0) => commands
            .spawn_scene(bsn! { @FeathersNumberInput {
                @label_text: {Some("X")},
                @sigil_color: {tokens::TEXT_INPUT_X_AXIS},
            } })
            .id(),
        Some(1) => commands
            .spawn_scene(bsn! { @FeathersNumberInput {
                @label_text: {Some("Y")},
                @sigil_color: {tokens::TEXT_INPUT_Y_AXIS},
            } })
            .id(),
        Some(2) => commands
            .spawn_scene(bsn! { @FeathersNumberInput {
                @label_text: {Some("Z")},
                @sigil_color: {tokens::TEXT_INPUT_Z_AXIS},
            } })
            .id(),
        _ => commands.spawn_scene(bsn! { @FeathersNumberInput }).id(),
    };
    commands
        .entity(entity)
        .insert((spec.field.clone(), FieldValue(spec.value), ChildOf(slot)))
        .observe(commit_number)
        .observe(field_drag_start)
        .observe(field_drag)
        .observe(field_drag_end);
    commands.trigger(UpdateNumberInput {
        entity,
        value: NumberInputValue::F32(spec.value),
    });
}

/// Queue one `Set` through the EditQueue: full component with the edited field
/// applied, inverse captured by the kernel. `gesture` makes drag frames coalesce.
fn queue_set(
    commands: &mut Commands,
    field: InspectorField,
    new_value: FieldNewValue,
    gesture: Option<u64>,
) {
    commands.queue(move |world: &mut World| {
        let Some(entity) = world.resource::<SceneIndex>().get(&field.target) else { return };
        let registry = world.resource::<AppTypeRegistry>().clone();
        let registry = registry.read();
        let Some(registration) = registry.get_with_type_path(field.type_path) else { return };
        let Some(reflect_component) =
            registration.data::<bevy::ecs::reflect::ReflectComponent>()
        else {
            return;
        };
        // NameText INSERTS when absent — every other kind edits an existing value.
        let current = reflect_component.reflect(world.entity(entity));
        if current.is_none() && !matches!(field.kind, FieldKind::NameText) {
            return;
        }

        let boxed: Box<dyn PartialReflect> = match (field.kind, new_value) {
            (FieldKind::Direct, FieldNewValue::F32(new_value)) => {
                let Some(current) = current else { return };
                let mut dynamic = current.as_partial_reflect().to_dynamic();
                let Ok(parsed) = ParsedPath::parse(field.path.as_str()) else { return };
                let Ok(element) = parsed.reflect_element_mut(dynamic.as_mut()) else { return };
                match element.try_downcast_mut::<f32>() {
                    Some(slot) => *slot = new_value,
                    None => return,
                }
                dynamic
            }
            (FieldKind::Bool, FieldNewValue::Bool(new_value)) => {
                let Some(current) = current else { return };
                let mut dynamic = current.as_partial_reflect().to_dynamic();
                let Ok(parsed) = ParsedPath::parse(field.path.as_str()) else { return };
                let Ok(element) = parsed.reflect_element_mut(dynamic.as_mut()) else { return };
                match element.try_downcast_mut::<bool>() {
                    Some(slot) => *slot = new_value,
                    None => return,
                }
                dynamic
            }
            (FieldKind::Str, FieldNewValue::Text(new_value)) => {
                let Some(current) = current else { return };
                let mut dynamic = current.as_partial_reflect().to_dynamic();
                let Ok(parsed) = ParsedPath::parse(field.path.as_str()) else { return };
                let Ok(element) = parsed.reflect_element_mut(dynamic.as_mut()) else { return };
                match element.try_downcast_mut::<String>() {
                    Some(slot) => *slot = new_value,
                    None => return,
                }
                dynamic
            }
            // Name's hash is derived — always rebuild through the constructor.
            (FieldKind::NameText, FieldNewValue::Text(new_value)) => {
                Box::new(Name::new(new_value))
            }
            (FieldKind::EulerDeg(axis), FieldNewValue::F32(new_value)) => {
                let Some(current) = current else { return };
                let Some(transform) =
                    current.as_partial_reflect().try_downcast_ref::<Transform>()
                else {
                    return;
                };
                let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
                let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
                degrees[axis] = new_value;
                let mut next = *transform;
                next.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    degrees.x.to_radians(),
                    degrees.y.to_radians(),
                    degrees.z.to_radians(),
                );
                Box::new(next)
            }
            _ => return,
        };
        drop(registry);
        world.resource_mut::<EditQueue>().0.push(Transaction {
            label: format!(
                "Edit {}",
                field.type_path.rsplit("::").next().unwrap_or("field")
            ),
            gesture,
            ops: vec![Op::Set { target: field.target, value: boxed }],
        });
    });
}

/// A COMPLETED typed edit (`is_final` — Enter or focus loss) commits one undo
/// entry. Focus is released ONLY when it is still inside the committing field
/// (the Enter case — feathers keeps it, which left the keyboard captured). When
/// the commit was CAUSED by focus moving on (Tab, click into the next field),
/// the new focus must survive — clearing unconditionally raced tab navigation
/// and dropped focus entirely (owner-diagnosed).
fn commit_number(
    change: On<ValueChange<f32>>,
    fields: Query<&InspectorField>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if !change.is_final {
        return;
    }
    let Ok(field) = fields.get(change.source) else { return };
    queue_set(&mut commands, field.clone(), FieldNewValue::F32(change.value), None);

    let still_ours = focus.get().is_some_and(|focused| {
        let mut current = focused;
        loop {
            if current == change.source {
                break true;
            }
            match parents.get(current) {
                Ok(parent) => current = parent.parent(),
                Err(_) => break false,
            }
        }
    });
    if still_ours {
        focus.clear();
    }
}

/// Checkbox commits are always final: one undo entry per toggle.
fn commit_bool(
    change: On<ValueChange<bool>>,
    fields: Query<&InspectorField>,
    mut commands: Commands,
) {
    let Ok(field) = fields.get(change.source) else { return };
    queue_set(&mut commands, field.clone(), FieldNewValue::Bool(change.value), None);
}

fn find_editable_descendant(world: &World, root: Entity) -> Option<Entity> {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<bevy::text::EditableText>(entity).is_some() {
            return Some(entity);
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    None
}

/// Text fields commit on Enter (predictable; Escape/blur = leave without commit).
fn commit_text_on_enter(
    event: On<bevy::input_focus::FocusedInput<bevy::input::keyboard::KeyboardInput>>,
    fields: Query<&InspectorField>,
    editable: Query<&bevy::text::EditableText>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    use bevy::input::ButtonState;
    if event.input.key_code != KeyCode::Enter || event.input.state != ButtonState::Pressed {
        return;
    }
    // The focused inner input is the event target; our field data sits on its
    // container (the entity this observer is attached to = the input's parent).
    let inner = event.event_target();
    let Ok(container) = parents.get(inner).map(|p| p.parent()) else { return };
    let Ok(field) = fields.get(container) else { return };
    let Ok(text) = editable.get(inner) else { return };
    let value = text.value().to_string();
    queue_set(&mut commands, field.clone(), FieldNewValue::Text(value), None);

    let still_ours = focus.get().is_some_and(|focused| {
        let mut current = focused;
        loop {
            if current == container {
                break true;
            }
            match parents.get(current) {
                Ok(parent) => current = parent.parent(),
                Err(_) => break false,
            }
        }
    });
    if still_ours {
        focus.clear();
    }
}

/// Slide-editing (owner): horizontal drag on a number field live-edits through
/// gesture-coalesced transactions — the whole drag is ONE undo entry. Shift = fine.
fn field_drag_start(
    drag: On<Pointer<DragStart>>,
    fields: Query<&FieldValue, With<InspectorField>>,
    mut counter: ResMut<GestureCounter>,
    mut commands: Commands,
) {
    let Ok(value) = fields.get(drag.entity) else { return };
    commands.entity(drag.entity).insert(FieldDrag {
        start: value.0,
        gesture: counter.next(),
    });
}

fn field_drag(
    drag: On<Pointer<Drag>>,
    state: Query<(&FieldDrag, &InspectorField)>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let Ok((field_drag, field)) = state.get(drag.entity) else { return };
    let fine = keys
        .map(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight))
        .unwrap_or(false);
    let step = if fine { 0.01 } else { 0.1 };
    let value = field_drag.start + drag.distance.x * step;
    queue_set(&mut commands, field.clone(), FieldNewValue::F32(value), Some(field_drag.gesture));
    commands.trigger(UpdateNumberInput {
        entity: drag.entity,
        value: NumberInputValue::F32(value),
    });
}

fn field_drag_end(
    drag: On<Pointer<DragEnd>>,
    state: Query<(), With<FieldDrag>>,
    mut model: ResMut<InspectorModel>,
    mut commands: Commands,
) {
    if state.get(drag.entity).is_ok() {
        commands.entity(drag.entity).remove::<FieldDrag>();
        model.dirty = true; // refresh FieldValue baselines post-drag
    }
}

/// Marks inputs whose tab order WE have assigned (feathers inserts its own
/// `TabIndex(0)` on every text input — filtering on `Without<TabIndex>` would
/// never fire, leaving every field tied at 0 and Tab order arbitrary).
#[derive(Component)]
pub(crate) struct TabOrdered;

/// Tab cycles fields (owner): stamp `TabIndex` on every focusable text input under
/// the inspector body in geometric order (top→bottom, left→right).
pub(crate) fn stamp_tab_indices(
    unstamped: Query<Entity, (With<bevy::text::EditableText>, Without<TabOrdered>)>,
    geometry: Query<(&ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    bodies: Query<&PanelBody>,
    mut commands: Commands,
) {
    let mut candidates: Vec<(Entity, Vec2)> = Vec::new();
    for entity in &unstamped {
        let mut current = entity;
        let inside = loop {
            if let Ok(body) = bodies.get(current) {
                break body.0.as_str() == INSPECTOR_PANEL;
            }
            match parents.get(current) {
                Ok(parent) => current = parent.parent(),
                Err(_) => break false,
            }
        };
        if inside {
            // Freshly spawned widgets have zeroed geometry until layout runs —
            // stamping now would sort every field at (0,0) and Tab order would be
            // garbage. Defer the WHOLE batch until all candidates are laid out.
            let Ok((node, transform)) = geometry.get(entity) else { return };
            if node.size() == Vec2::ZERO {
                return;
            }
            candidates.push((entity, transform.translation));
        }
    }
    if candidates.is_empty() {
        return;
    }
    candidates.sort_by(|a, b| {
        (a.1.y, a.1.x)
            .partial_cmp(&(b.1.y, b.1.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, (entity, _)) in candidates.iter().enumerate() {
        commands.entity(*entity).insert((
            bevy::input_focus::tab_navigation::TabIndex(i as i32),
            TabOrdered,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::prelude::History;
    use editor_core::EditorCorePlugin;

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("inspector-test", "Inspector Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Transform>();
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    fn spawn_transform(app: &mut App, id: SceneId, transform: Transform) {
        app.world_mut().resource_mut::<EditQueue>().0.push(Transaction {
            label: "spawn".into(),
            gesture: None,
            ops: vec![Op::Spawn {
                id,
                components: vec![Box::new(transform).into_partial_reflect()],
            }],
        });
        app.update();
    }

    // C3: a completed field edit is ONE undoable Set through the queue.
    #[test]
    fn field_edit_is_one_undoable_set() {
        let mut app = test_app();
        let id = SceneId::random();
        spawn_transform(&mut app, id, Transform::from_xyz(1.0, 2.0, 3.0));
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();

        let depth_before = app.world().resource::<History>().undo_depth();
        {
            let world = app.world_mut();
            let registry = world.resource::<AppTypeRegistry>().clone();
            let registry_guard = registry.read();
            let registration = registry_guard
                .get_with_type_path("bevy_transform::components::transform::Transform")
                .unwrap();
            let reflect_component = registration
                .data::<bevy::ecs::reflect::ReflectComponent>()
                .unwrap();
            let current = reflect_component.reflect(world.entity(entity)).unwrap();
            let mut dynamic = current.as_partial_reflect().to_dynamic();
            let parsed = ParsedPath::parse("translation.y").unwrap();
            *parsed
                .reflect_element_mut(dynamic.as_mut())
                .unwrap()
                .try_downcast_mut::<f32>()
                .unwrap() = 20.0;
            drop(registry_guard);
            world.resource_mut::<EditQueue>().0.push(Transaction {
                label: "Edit Transform".into(),
                gesture: None,
                ops: vec![Op::Set { target: id, value: dynamic }],
            });
        }
        app.update();

        let world = app.world_mut();
        let entity = world.resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.y, 20.0);
        assert_eq!(
            world.resource::<History>().undo_depth(),
            depth_before + 1,
            "one undo entry per completed edit"
        );
        world.resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        let world = app.world_mut();
        let entity = world.resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.y, 2.0);
    }

    // Slide-edits: drag frames sharing one gesture id coalesce into ONE undo entry.
    #[test]
    fn slide_edit_coalesces() {
        let mut app = test_app();
        let id = SceneId::random();
        spawn_transform(&mut app, id, Transform::IDENTITY);
        let depth_before = app.world().resource::<History>().undo_depth();

        for value in [0.5_f32, 1.0, 1.5] {
            app.world_mut().resource_mut::<EditQueue>().0.push(Transaction {
                label: "Edit Transform".into(),
                gesture: Some(999),
                ops: vec![Op::Set {
                    target: id,
                    value: Box::new(Transform::from_xyz(value, 0.0, 0.0))
                        .into_partial_reflect(),
                }],
            });
            app.update();
        }
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before + 1,
            "whole slide = one entry"
        );
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        let world = app.world_mut();
        let entity = world.resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(
            world.get::<Transform>(entity).unwrap().translation.x,
            0.0,
            "undo restores pre-drag value"
        );
    }
}
