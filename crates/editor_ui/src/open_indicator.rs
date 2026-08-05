//! Open-instance indicator v2 (owner: the border box was bad design):
//! 1. a floating breadcrumb pill top-center of the viewport — SCENE ▸ ◆ NAME —
//!    with the escape affordance, and
//! 2. everything OUTSIDE the open instance dims (material swap, restored on
//!    close) — the scene literally recedes while you edit inside.

use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;
use editor_prefabs::open_mode::OpenInstance;

use crate::style::{self, UiFonts};

#[derive(Component)]
pub(crate) struct OpenPill;
#[derive(Component)]
pub(crate) struct OpenPillText;

/// Original material of a dimmed outsider, restored on close.
#[derive(Component)]
pub(crate) struct DimmedMaterial(pub Handle<StandardMaterial>);

#[derive(Resource, Default)]
pub(crate) struct DimAssets(Option<Handle<StandardMaterial>>);

pub(crate) fn spawn_open_pill(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
) {
    let ui = settings.ui.clone();
    commands
        .spawn((
            OpenPill,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(style::space::M),
                justify_content: JustifyContent::Center,
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
            GlobalZIndex(90),
            Visibility::Hidden,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(style::space::S),
                        padding: UiRect::axes(px(style::space::M), px(style::space::XS)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(style::radius::L)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.09, 0.11, 0.92)),
                    BorderColor::all(style::color::accent().with_alpha(0.55)),
                    bevy::picking::Pickable::IGNORE,
                ))
                .with_children(|pill| {
                    pill.spawn((
                        Text::new("SCENE"),
                        style::sans_medium(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                    ));
                    pill.spawn((
                        Text::new("›"),
                        style::mono(&fonts, ui.font_size_s),
                        TextColor(style::color::TEXT_DIM),
                    ));
                    pill.spawn((
                        OpenPillText,
                        Text::new(""),
                        style::sans_medium(&fonts, ui.font_size_s),
                        TextColor(style::color::accent()),
                    ));
                    pill.spawn((
                        Text::new("⎋ close"),
                        style::mono(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                    ));
                });
        });
}

pub(crate) fn sync_open_pill(
    open: Res<OpenInstance>,
    state: Res<EditorState>,
    mut pill: Query<&mut Visibility, With<OpenPill>>,
    mut text: Query<&mut Text, With<OpenPillText>>,
) {
    let name = state
        .active
        .then(|| open.0.as_ref().map(|o| o.name.clone()))
        .flatten();
    for mut visibility in &mut pill {
        let target = if name.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
    if let Some(name) = name {
        let label = format!("◆ {}", name.to_uppercase());
        for mut t in &mut text {
            if t.0 != label {
                t.0 = label.clone();
            }
        }
    }
}

/// Dim every scene entity OUTSIDE the open instance; restore on close. The
/// member set lives in `SelectionScope` (maintained every frame while open).
pub(crate) fn dim_outsiders(
    open: Res<OpenInstance>,
    scope: Res<SelectionScope>,
    mut assets: ResMut<DimAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene_meshes: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        (With<SceneId>, Without<DimmedMaterial>),
    >,
    stamped_meshes: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        (With<editor_scene::PrefabStamped>, Without<DimmedMaterial>),
    >,
    dimmed: Query<(Entity, &DimmedMaterial)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let Some(members) = open.0.as_ref().and(scope.0.as_ref()) else {
        // Closed: restore everyone.
        for (entity, original) in &dimmed {
            commands
                .entity(entity)
                .insert(MeshMaterial3d(original.0.clone()))
                .remove::<DimmedMaterial>();
        }
        return;
    };
    let dim = assets
        .0
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.35, 0.36, 0.38, 0.25),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })
        })
        .clone();
    let inside = |entity: Entity| -> bool {
        let mut current = entity;
        loop {
            if members.contains(&current) {
                return true;
            }
            match parents.get(current) {
                Ok(parent) => current = parent.parent(),
                Err(_) => return false,
            }
        }
    };
    for (entity, material) in scene_meshes.iter().chain(stamped_meshes.iter()) {
        if inside(entity) {
            continue;
        }
        commands.entity(entity).insert((
            DimmedMaterial(material.0.clone()),
            MeshMaterial3d(dim.clone()),
        ));
    }
}
