//! The game's editor registration (spec §1) — only compiled with `--features editor`.
//!
//! ALL editor chrome (palette, status bar, which-key, styling) lives in `editor_ui`;
//! this module contributes only what is specific to THIS game: which components
//! serialize, what can be placed, and the input handoff to the game's player.

use bevy::prelude::*;
use editor_core::prelude::*;
use editor_scene::materials::{MaterialLibrary, MaterialRef};
use editor_scene::session::EditorSession;
use std::collections::HashMap;

use crate::game::{GameInputActive, Primitive, PrimitiveKind, Spinner};

/// The game's editor-facing registration: which components serialize, what can be
/// placed. Lives editor-side; the game module stays editor-free.
struct GameFeature;

fn cube_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(Transform::from_translation(position + Vec3::Y * 0.5)).into_partial_reflect(),
        Box::new(Primitive {
            kind: PrimitiveKind::Cube,
            size: 1.0,
        })
        .into_partial_reflect(),
        Box::new(Spinner::default()).into_partial_reflect(),
        Box::new(Name::new("Cube")).into_partial_reflect(),
    ]
}

fn sphere_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(Transform::from_translation(position + Vec3::Y * 0.5)).into_partial_reflect(),
        Box::new(Primitive {
            kind: PrimitiveKind::Sphere,
            size: 1.0,
        })
        .into_partial_reflect(),
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
        app.add_systems(Startup, arm_session_restore);
        app.add_systems(
            Update,
            (sync_game_input, sync_material_refs, drive_session_restore)
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            probe_spin
                .run_if(|| std::env::var("SPIN_PROBE").is_ok())
                .in_set(editor_core::EditorSet::Sync),
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
            if let Some(handle) = handles.0.get(&def.id)
                && let Some(mut material) = materials.get_mut(handle)
            {
                *material = standard_material(def);
            }
        }
    }
    let mut apply = |entity: Entity, material_ref: &MaterialRef| {
        let Some(def) = library.get(&material_ref.0) else {
            return;
        };
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

// ---------------------------------------------------------------------------
// Fast-relaunch session restore (M3-C8 fallback): a fresh sidecar written by
// `editor.reload` drives boot straight back into the editing context — menu
// skipped, scene reloaded, editor active, selection and camera restored.
// ---------------------------------------------------------------------------

enum RestoreStage {
    SkipMenu,
    WaitLevel,
    Settle,
    Apply,
}

#[derive(Resource)]
struct SessionRestore {
    session: EditorSession,
    stage: RestoreStage,
}

fn arm_session_restore(mut commands: Commands) {
    if let Some(session) = editor_scene::session::take_session() {
        info!("fast-relaunch: restoring editor session");
        commands.insert_resource(SessionRestore {
            session,
            stage: RestoreStage::SkipMenu,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_session_restore(
    restore: Option<ResMut<SessionRestore>>,
    app_state: Res<State<game_framework::AppState>>,
    mut next: ResMut<NextState<game_framework::AppState>>,
    index: Res<SceneIndex>,
    mut editor_state: ResMut<EditorState>,
    mut actions: MessageWriter<ActionInvoked>,
    mut changed: MessageWriter<SelectionChanged>,
    mut cameras: Query<(&Camera, &mut Transform, Option<&bevy::camera::RenderTarget>)>,
    mut commands: Commands,
) {
    let Some(mut restore) = restore else { return };
    match restore.stage {
        RestoreStage::SkipMenu => {
            next.set(game_framework::AppState::LoadingLevel);
            restore.stage = RestoreStage::WaitLevel;
        }
        RestoreStage::WaitLevel => {
            if *app_state.get() == game_framework::AppState::InGame && !index.is_empty() {
                restore.stage = if restore.session.scene_path.exists() {
                    invoke_open_scene(&mut actions);
                    RestoreStage::Settle
                } else {
                    RestoreStage::Apply
                };
            }
        }
        RestoreStage::Settle => restore.stage = RestoreStage::Apply,
        RestoreStage::Apply => {
            editor_state.active = restore.session.editor_active;
            for id in &restore.session.selection {
                if let Some(entity) = index.get(id) {
                    commands.entity(entity).insert(Selected);
                }
            }
            if !restore.session.selection.is_empty() {
                changed.write(SelectionChanged);
            }
            if let Some(matrix) = restore.session.camera {
                let target = Transform::from_matrix(Mat4::from_cols_array(&matrix));
                if let Some((_, mut transform, _)) = cameras
                    .iter_mut()
                    .find(|(c, _, t)| is_viewport_camera(c, t.as_deref()))
                {
                    *transform = target;
                }
            }
            commands.remove_resource::<SessionRestore>();
            info!("fast-relaunch: session restored");
        }
    }
}

fn invoke_open_scene(actions: &mut MessageWriter<ActionInvoked>) {
    actions.write(ActionInvoked {
        action: ActionId::new_static("scene.open"),
        args: None,
        source: InvocationSource::Test,
    });
}

/// TEMP diagnostic (SPIN_PROBE=1, with INSPECTOR_PROBE=1 driving menu/editor):
/// enable a spinner, trigger play, log every gate the spin system depends on.
#[allow(clippy::too_many_arguments)]
pub(crate) fn probe_spin(
    mut frames: Local<u32>,
    mut writer: MessageWriter<ActionInvoked>,
    mut spinners: Query<(Entity, &mut Spinner, &Transform)>,
    game_input: Res<GameInputActive>,
    editor_state: Res<EditorState>,
    app_state: Res<State<game_framework::AppState>>,
    time: Res<Time>,
) {
    *frames += 1;
    if *frames == 300
        && std::env::var("BOOL_PROBE").is_err()
        && let Some((entity, mut spinner, _)) = spinners.iter_mut().next()
    {
        spinner.enabled = true;
        info!("SPIN enabled directly on {entity:?}");
    }
    if *frames == 360 {
        writer.write(ActionInvoked {
            action: ActionId::new_static("editor.play"),
            args: None,
            source: InvocationSource::Test,
        });
        info!("SPIN play triggered");
    }
    if *frames > 400 && (*frames).is_multiple_of(60) {
        let enabled_count = spinners.iter().filter(|(_, s, _)| s.enabled).count();
        let rotation = spinners
            .iter()
            .find(|(_, s, _)| s.enabled)
            .or_else(|| spinners.iter().next())
            .map(|(_, s, t)| (s.enabled, t.rotation.to_axis_angle().1.to_degrees()));
        info!("SPIN enabled_count={enabled_count}");
        info!(
            "SPIN state={:?} editor_active={} game_input={} delta={:.3} rot={:?}",
            app_state.get(),
            editor_state.active,
            game_input.0,
            time.delta_secs(),
            rotation
        );
    }
}
