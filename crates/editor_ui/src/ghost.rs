//! Ghost styling for insert previews: translucent accent material applied over
//! whatever material the entity's regenerate systems attached. Kind-agnostic — any
//! `InsertPreview` with a `StandardMaterial` mesh gets the treatment.

use bevy::prelude::*;
use editor_core::prelude::*;

#[derive(Component)]
pub(crate) struct GhostApplied;

#[derive(Resource)]
pub(crate) struct GhostMaterial(Handle<StandardMaterial>);

pub(crate) fn init_ghost_material(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let [r, g, b, a] = settings.viewport.ghost_color;
    commands.insert_resource(GhostMaterial(materials.add(StandardMaterial {
        base_color: Color::srgba(r, g, b, a),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    })));
}

#[allow(clippy::type_complexity)]
pub(crate) fn apply_ghost_material(
    ghost: Res<GhostMaterial>,
    mut previews: Query<
        (Entity, &mut MeshMaterial3d<StandardMaterial>),
        (With<InsertPreview>, Without<GhostApplied>),
    >,
    mut commands: Commands,
) {
    for (entity, mut material) in &mut previews {
        material.0 = ghost.0.clone();
        commands.entity(entity).insert(GhostApplied);
    }
}
