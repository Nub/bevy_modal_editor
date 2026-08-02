//! The game's editor registration (spec §1) — only compiled with `--features editor`.
//!
//! ALL editor chrome (palette, status bar, which-key, styling) lives in `editor_ui`;
//! this module contributes only what is specific to THIS game: which components
//! serialize, what can be placed, and the input handoff to the game's player.

use bevy::prelude::*;
use editor_core::prelude::*;
use editor_scene::materials::{MaterialLibrary, MaterialRef};
use std::collections::HashMap;

use crate::game::{GameInputActive, Primitive, PrimitiveKind, Spinner};

/// The game's editor-facing registration: which components serialize, what can be
/// placed. Lives editor-side; the game module stays editor-free.
struct GameFeature;

fn cube_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(Transform::from_translation(position + Vec3::Y * 0.5))
            .into_partial_reflect(),
        Box::new(Primitive { kind: PrimitiveKind::Cube, size: 1.0 }).into_partial_reflect(),
        Box::new(Spinner::default()).into_partial_reflect(),
        Box::new(Name::new("Cube")).into_partial_reflect(),
    ]
}

fn sphere_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(Transform::from_translation(position + Vec3::Y * 0.5))
            .into_partial_reflect(),
        Box::new(Primitive { kind: PrimitiveKind::Sphere, size: 1.0 }).into_partial_reflect(),
        Box::new(Name::new("Sphere")).into_partial_reflect(),
    ]
}

impl EditorFeature for GameFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("template-game", "Template Game")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.component::<Transform>()
            .component::<Primitive>()
            .component::<Spinner>()
            .component::<Name>()
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("primitive.cube"),
                display_name: "Cube",
                components: cube_components,
            })
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("primitive.sphere"),
                display_name: "Sphere",
                components: sphere_components,
            });
    }
}

pub struct EditorOverlayPlugin;

impl Plugin for EditorOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(editor_ui::EditorUiPlugin);
        app.add_editor_feature(GameFeature);
        app.init_resource::<MaterialHandles>();
        app.add_systems(
            Update,
            (sync_game_input, sync_material_refs).in_set(editor_core::EditorSet::Sync),
        );
    }
}

/// GPU handles per library material (created on demand, patched in place on
/// library edits so every user re-shades live).
#[derive(Resource, Default)]
struct MaterialHandles(HashMap<uuid::Uuid, Handle<StandardMaterial>>);

fn standard_material(def: &editor_scene::materials::MaterialDef) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(
            def.base_color[0],
            def.base_color[1],
            def.base_color[2],
            def.base_color[3],
        ),
        metallic: def.metallic,
        perceptual_roughness: def.roughness.clamp(0.089, 1.0),
        ..default()
    }
}

/// `MaterialRef` -> `MeshMaterial3d`: assignment/undo re-shade via change
/// detection; library param edits patch the SHARED handles in place; removal
/// (undo of a first assignment) falls back to the default primitive material.
#[allow(clippy::too_many_arguments)]
fn sync_material_refs(
    library: Res<MaterialLibrary>,
    assets: Res<crate::game::PrimitiveAssets>,
    mut handles: ResMut<MaterialHandles>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    changed: Query<(Entity, &MaterialRef), Changed<MaterialRef>>,
    all: Query<(Entity, &MaterialRef)>,
    mut removed: RemovedComponents<MaterialRef>,
    primitives: Query<(), With<Primitive>>,
    mut commands: Commands,
) {
    if library.is_changed() {
        for def in &library.materials {
            if let Some(handle) = handles.0.get(&def.id) {
                if let Some(mut material) = materials.get_mut(handle) {
                    *material = standard_material(def);
                }
            }
        }
    }
    let mut apply = |entity: Entity, material_ref: &MaterialRef| {
        let Some(def) = library.get(&material_ref.0) else { return };
        let handle = handles
            .0
            .entry(def.id)
            .or_insert_with(|| materials.add(standard_material(def)))
            .clone();
        commands.entity(entity).insert(MeshMaterial3d(handle));
    };
    if library.is_changed() {
        for (entity, material_ref) in &all {
            apply(entity, material_ref);
        }
    } else {
        for (entity, material_ref) in &changed {
            apply(entity, material_ref);
        }
    }
    for entity in removed.read() {
        if primitives.get(entity).is_ok() {
            commands
                .entity(entity)
                .insert(MeshMaterial3d(assets.material.clone()));
        }
    }
}

/// The editor owns input while active; the game module knows nothing about the
/// editor — it just honors `GameInputActive`.
fn sync_game_input(
    state: Res<EditorState>,
    mut game_input: ResMut<GameInputActive>,
    mut player: Query<(&mut crate::game::Player, &Transform)>,
) {
    let game_owns = !state.active;
    if game_input.0 != game_owns {
        game_input.0 = game_owns;
        // Handing input back to the game: the editor may have flown the camera —
        // re-derive the player's yaw/pitch so the game view doesn't snap.
        if game_owns {
            for (mut player, transform) in &mut player {
                let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
                player.yaw = yaw;
                player.pitch = pitch;
            }
        }
    }
}
