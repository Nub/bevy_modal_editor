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
            })
            // D8 proof-of-seam: a game-registered bake step. Real games hang
            // collider/LOD derivation here; the census proves determinism and
            // the registry path end-to-end.
            .baker(editor_api::prelude::BakerDef {
                id: editor_api::prelude::BakerId::new_static("game.census"),
                name: "Entity Census",
                version: 1,
                bake: |cx| {
                    let spinners = cx.template_ron.matches("Spinner").count();
                    let primitives = cx.template_ron.matches("Primitive").count();
                    Ok(Some(
                        format!(
                            "(prefab: {:?}, spinners: {spinners}, primitives: {primitives})",
                            cx.prefab_name
                        )
                        .into_bytes(),
                    ))
                },
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
            demo_kit_generator.run_if(|| std::env::var("EDITOR_DEMO_KIT").is_ok()),
        );
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

/// EDITOR_DEMO_KIT=1 (owner ask): generate a socketed wall kit — Wall, Corner,
/// Gate — through the REAL prefab pipeline, chain instances into a small
/// courtyard via the mating math, save the scene to level.ron, exit. Open it
/// with "Open Scene" (palette) afterwards.
pub(crate) fn demo_kit_generator(world: &mut World, mut frame: Local<u32>) {
    if std::env::var("EDITOR_DEMO_KIT").is_err() {
        return;
    }
    *frame += 1;
    use editor_prefabs::sockets::{Socket, mate_transform, template_sockets};
    use editor_prefabs::{PrefabDef, PrefabInstance, PrefabLibrary, PrefabOverrides};

    let face = |direction: Vec3| Quat::from_rotation_arc(Vec3::Z, direction);
    let cube = |size: f32, at: Vec3, scale: Vec3| {
        vec![
            Box::new(Primitive {
                kind: PrimitiveKind::Cube,
                size,
            })
            .into_partial_reflect(),
            Box::new(Transform::from_translation(at).with_scale(scale)).into_partial_reflect(),
            Box::new(Name::new("Part")).into_partial_reflect(),
        ]
    };
    let socket = |name: &str, at: Vec3, direction: Vec3| {
        vec![
            Box::new(Socket {
                name: name.into(),
                socket_type: "wall".into(),
            })
            .into_partial_reflect(),
            Box::new(Transform::from_translation(at).with_rotation(face(direction)))
                .into_partial_reflect(),
            Box::new(Name::new(format!("socket {name}"))).into_partial_reflect(),
        ]
    };
    let def = |name: &str, records: Vec<Vec<Box<dyn bevy::reflect::PartialReflect>>>| PrefabDef {
        kit: Some("demo".into()),
        id: uuid::Uuid::new_v4(),
        name: name.into(),
        template: editor_scene::snapshot_from_parts(
            records
                .into_iter()
                .map(|components| (editor_api::prelude::SceneId::random(), None, components))
                .collect(),
        ),
    };

    if *frame == 1 {
        world
            .resource_mut::<NextState<game_framework::AppState>>()
            .set(game_framework::AppState::LoadingLevel);
    }
    if *frame == 10 {
        let wall = def(
            "Wall",
            vec![
                cube(1.0, Vec3::new(0.0, 0.5, 0.0), Vec3::new(2.0, 1.0, 0.15)),
                socket("west", Vec3::new(-1.0, 0.5, 0.0), -Vec3::X),
                socket("east", Vec3::new(1.0, 0.5, 0.0), Vec3::X),
            ],
        );
        let corner = def(
            "Corner",
            vec![
                cube(1.0, Vec3::new(0.0, 0.6, 0.0), Vec3::new(0.3, 1.3, 0.3)),
                socket("in", Vec3::ZERO.with_y(0.5), -Vec3::X),
                socket("out", Vec3::ZERO.with_y(0.5), Vec3::Z),
            ],
        );
        let gate = def(
            "Gate",
            vec![
                cube(1.0, Vec3::new(-0.85, 0.5, 0.0), Vec3::new(0.3, 1.0, 0.2)),
                cube(1.0, Vec3::new(0.85, 0.5, 0.0), Vec3::new(0.3, 1.0, 0.2)),
                cube(1.0, Vec3::new(0.0, 1.1, 0.0), Vec3::new(2.0, 0.2, 0.2)),
                socket("west", Vec3::new(-1.0, 0.5, 0.0), -Vec3::X),
                socket("east", Vec3::new(1.0, 0.5, 0.0), Vec3::X),
            ],
        );
        // The ring: wall, corner ×4 sides, one side a gate.
        let sequence = [
            "Wall", "Corner", "Wall", "Corner", "Gate", "Corner", "Wall", "Corner",
        ];
        for prefab in [&wall, &corner, &gate] {
            editor_prefabs::authoring::save_prefab_public(world, prefab);
        }
        let defs = [wall, corner, gate];
        let mut cursor: Option<GlobalTransform> = None; // previous piece's EXIT frame
        for name in sequence {
            let def = defs.iter().find(|d| d.name == name).unwrap();
            let sockets = template_sockets(def);
            let (entry, exit) = (&sockets[0], &sockets[sockets.len() - 1]);
            let root = match &cursor {
                None => Transform::from_xyz(0.0, 0.0, 4.0),
                Some(previous_exit) => mate_transform(previous_exit, &entry.0),
            };
            cursor = Some(
                GlobalTransform::from(root) * GlobalTransform::from(exit.0).compute_transform(),
            );
            let id = editor_api::prelude::SceneId::random();
            world
                .resource_mut::<editor_core::prelude::EditQueue>()
                .0
                .push(editor_core::prelude::Transaction {
                    label: format!("Place {name}"),
                    gesture: None,
                    ops: vec![editor_core::prelude::Op::Spawn {
                        id,
                        components: vec![
                            Box::new(PrefabInstance(def.id)).into_partial_reflect(),
                            Box::new(PrefabOverrides::default()).into_partial_reflect(),
                            Box::new(root).into_partial_reflect(),
                            Box::new(Name::new(name.to_string())).into_partial_reflect(),
                        ],
                    }],
                });
        }
        let mut library = world.resource_mut::<PrefabLibrary>();
        for def in defs {
            library.prefabs.insert(def.id, def);
        }
        library.generation += 1;
        world
            .resource_mut::<editor_core::prelude::EditorState>()
            .active = true;
    }
    if *frame == 30 {
        world.write_message(editor_api::prelude::ActionInvoked {
            action: editor_api::prelude::ActionId::new_static("scene.save"),
            args: None,
            source: editor_api::prelude::InvocationSource::Test,
        });
    }
    if *frame == 45 {
        world
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(
                "target/demo-kit.png",
            ));
    }
    if *frame == 60 {
        println!(
            "demo kit: prefabs saved (Wall, Corner, Gate) and courtyard scene written to level.ron"
        );
        world.write_message(bevy::app::AppExit::Success);
    }
}
