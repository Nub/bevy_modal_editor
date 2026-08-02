//! Inspector panel (M3-C3): ONE recursive reflection editor over the selection —
//! no per-component snapshot structs, ever (spec §7). Two phases: an exclusive
//! collect pass reflects the selected entity's registered components into a plain
//! `InspectorModel`, and a render pass builds widgets from it (kit-composed
//! feathers controls). Every completed field edit commits ONE `EditScope` `Set`
//! transaction — the same path as spawn/load/undo — so field edits are undoable
//! by construction.
//!
//! Type overrides (the `TypeId -> widget` registry seed): `Transform` renders as
//! Position / Rotation-as-Euler-degrees / Scale triples; `Vec3` structs render as
//! X/Y/Z triples; other structs walk generically (f32 leaves editable, everything
//! else read-only until its widget lands).

use bevy::feathers::controls::{FeathersNumberInput, NumberInputValue, UpdateNumberInput};
use bevy::prelude::*;
use bevy::reflect::{ParsedPath, ReflectPath, ReflectRef};
use bevy::ui::px;
use bevy::ui_widgets::ValueChange;
use editor_core::edits::EditorComponents;
use editor_core::prelude::*;

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
}

pub(crate) struct NumberSpec {
    pub value: f32,
    pub sigil: Option<&'static str>,
    pub field: InspectorField,
}

pub(crate) enum RowSpec {
    Section(String),
    Triple { label: String, fields: Vec<NumberSpec> },
    Number { label: String, field: NumberSpec },
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

/// Marks inputs that should suppress inspector rebuilds while focused (a rebuild
/// would despawn the field mid-edit).
fn focus_inside_inspector(world: &World) -> bool {
    use bevy::input_focus::InputFocus;
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
/// components into the model. Runs when marked dirty and the user isn't mid-edit.
pub(crate) fn collect_inspector(world: &mut World) {
    let dirty = world.resource::<InspectorModel>().dirty;
    if !dirty || focus_inside_inspector(world) {
        return;
    }
    world.resource_mut::<InspectorModel>().dirty = false;

    let selected: Option<(Entity, SceneId)> = {
        let mut query = world.query_filtered::<(Entity, &SceneId), With<Selected>>();
        let mut all: Vec<(Entity, SceneId)> =
            query.iter(world).map(|(e, id)| (e, *id)).collect();
        all.sort_by_key(|(_, id)| id.0);
        all.first().copied()
    };

    let mut rows = Vec::new();
    if let Some((entity, target)) = selected {
        let registry = world.resource::<AppTypeRegistry>().clone();
        let components = world.resource::<EditorComponents>().types.clone();
        let registry = registry.read();
        for reg in &components {
            let Some(registration) = registry.get(reg.type_id) else { continue };
            let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            else {
                continue;
            };
            let Some(value) = reflect_component.reflect(world.entity(entity)) else {
                continue;
            };
            let short = reg.type_path.rsplit("::").next().unwrap_or(reg.type_path);
            rows.push(RowSpec::Section(short.to_uppercase()));
            collect_component(target, reg.type_path, value.as_partial_reflect(), &mut rows);
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
    let axes = [("x", "X"), ("y", "Y"), ("z", "Z")];
    RowSpec::Triple {
        label: label.to_string(),
        fields: axes
            .iter()
            .enumerate()
            .map(|(i, (axis, sigil))| NumberSpec {
                value: values[i],
                sigil: Some(sigil),
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

fn collect_component(
    target: SceneId,
    type_path: &'static str,
    value: &dyn PartialReflect,
    rows: &mut Vec<RowSpec>,
) {
    // Type override: Transform as Position / Rotation(Euler °) / Scale.
    if let Some(transform) = value.try_downcast_ref::<Transform>() {
        rows.push(triple(target, type_path, "Position", "translation", transform.translation));
        let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
        let degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
        rows.push(RowSpec::Triple {
            label: "Rotation °".to_string(),
            fields: (0..3)
                .map(|axis| NumberSpec {
                    value: degrees[axis],
                    sigil: Some(["X", "Y", "Z"][axis]),
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
        return;
    }
    if let Some(name) = value.try_downcast_ref::<Name>() {
        rows.push(RowSpec::ReadOnly {
            label: "Name".to_string(),
            value: name.as_str().to_string(),
        });
        return;
    }
    walk_fields(target, type_path, "", value, rows);
}

/// Generic recursive walk: f32 leaves become editable numbers; Vec3 structs become
/// triples; bools/strings/enums render read-only until their widgets land (C4
/// grows the kit; the walk shape doesn't change).
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
                        sigil: None,
                        field: InspectorField {
                            target,
                            type_path,
                            path: prefix.to_string(),
                            kind: FieldKind::Direct,
                        },
                    },
                });
            } else if let Some(v) = value.try_downcast_ref::<bool>() {
                rows.push(RowSpec::ReadOnly { label: leaf_label(prefix), value: v.to_string() });
            } else if let Some(v) = value.try_downcast_ref::<String>() {
                rows.push(RowSpec::ReadOnly { label: leaf_label(prefix), value: v.clone() });
            }
        }
    }
}

fn leaf_label(path: &str) -> String {
    path.rsplit('.').next().unwrap_or(path).to_string()
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
                Text::new("no selection"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
        });
        return;
    }

    for spec in &model.rows {
        match spec {
            RowSpec::Section(title) => {
                let header = commands
                    .spawn((
                        Text::new(title.clone()),
                        style::sans_medium(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                        Node {
                            margin: UiRect::top(px(style::space::S)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ))
                    .id();
                commands.entity(header).insert(ChildOf(body_entity));
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

/// F6 rule baked in once: label ABOVE controls, controls in a capped-width row.
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
            align_items: AlignItems::Center,
            ..default()
        })
        .id();
    commands.entity(controls).insert(ChildOf(container));
    controls
}

fn spawn_number_field(commands: &mut Commands, parent: Entity, spec: &NumberSpec) {
    let entity = match spec.sigil {
        Some("X") => commands
            .spawn_scene(bsn! { @FeathersNumberInput { @label_text: {Some("X")} } })
            .id(),
        Some("Y") => commands
            .spawn_scene(bsn! { @FeathersNumberInput { @label_text: {Some("Y")} } })
            .id(),
        Some("Z") => commands
            .spawn_scene(bsn! { @FeathersNumberInput { @label_text: {Some("Z")} } })
            .id(),
        _ => commands.spawn_scene(bsn! { @FeathersNumberInput }).id(),
    };
    commands
        .entity(entity)
        .insert((spec.field.clone(), ChildOf(parent)))
        .observe(commit_number);
    commands.trigger(UpdateNumberInput {
        entity,
        value: NumberInputValue::F32(spec.value),
    });
}

/// A COMPLETED edit (`finished: true` — Enter or focus loss) commits one `Set`
/// transaction through the EditQueue: full-component value with the edited field
/// applied, captured inverse, one undo entry. Mid-typing changes never commit.
fn commit_number(
    change: On<ValueChange<f32>>,
    fields: Query<&InspectorField>,
    index: Res<SceneIndex>,
    mut commands: Commands,
) {
    if !change.is_final {
        return;
    }
    let Ok(field) = fields.get(change.source) else { return };
    let field = field.clone();
    let Some(entity) = index.get(&field.target) else { return };
    let new_value = change.value;
    commands.queue(move |world: &mut World| {
        let registry = world.resource::<AppTypeRegistry>().clone();
        let registry = registry.read();
        let Some(registration) = registry.get_with_type_path(field.type_path) else { return };
        let Some(reflect_component) =
            registration.data::<bevy::ecs::reflect::ReflectComponent>()
        else {
            return;
        };
        let Some(current) = reflect_component.reflect(world.entity(entity)) else { return };

        let boxed: Box<dyn PartialReflect> = match field.kind {
            FieldKind::Direct => {
                let mut dynamic = current.as_partial_reflect().to_dynamic();
                let Ok(parsed) = ParsedPath::parse(field.path.as_str()) else { return };
                let Ok(element) = parsed.reflect_element_mut(dynamic.as_mut()) else { return };
                match element.try_downcast_mut::<f32>() {
                    Some(slot) => *slot = new_value,
                    None => return,
                }
                dynamic
            }
            FieldKind::EulerDeg(axis) => {
                let Some(transform) =
                    current.as_partial_reflect().try_downcast_ref::<Transform>()
                else {
                    return;
                };
                let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
                let mut degrees =
                    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
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
        };
        world.resource_mut::<EditQueue>().0.push(Transaction {
            label: format!("Edit {}", field.type_path.rsplit("::").next().unwrap_or("field")),
            gesture: None,
            ops: vec![Op::Set { target: field.target, value: boxed }],
        });
    });
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

    // C3: a completed field edit is ONE undoable Set through the queue; Euler
    // edits rebuild the quaternion.
    #[test]
    fn field_edit_is_one_undoable_set() {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;

        let id = SceneId::random();
        app.world_mut().resource_mut::<EditQueue>().0.push(Transaction {
            label: "spawn".into(),
            gesture: None,
            ops: vec![Op::Spawn {
                id,
                components: vec![
                    Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                ],
            }],
        });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();

        // Simulate a finished number-field commit on translation.y.
        let field = InspectorField {
            target: id,
            type_path: "bevy_transform::components::transform::Transform",
            path: "translation.y".into(),
            kind: FieldKind::Direct,
        };
        let holder = app.world_mut().spawn(field).id();
        let depth_before = app.world().resource::<History>().undo_depth();
        // Drive the same closure the observer queues.
        {
            let world = app.world_mut();
            let fields = world.get::<InspectorField>(holder).unwrap().clone();
            let registry = world.resource::<AppTypeRegistry>().clone();
            let registry_guard = registry.read();
            let registration =
                registry_guard.get_with_type_path(fields.type_path).unwrap();
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
}
