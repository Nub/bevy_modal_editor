//! The game's editor registration (spec §1) — only compiled with `--features editor`.
//!
//! ALL editor chrome (palette, status bar, which-key, styling) lives in `editor_ui`;
//! this module contributes only what is specific to THIS game: which components
//! serialize, what can be placed, and the input handoff to the game's player.

use avian3d::prelude::PhysicsGizmos;
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_scene::session::EditorSession;

use game_framework::GameplayActive;

use crate::game::{AutoBoxCollider, BoxCollider, PhysicsBody, Primitive, PrimitiveKind, Spinner};

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

/// Lights are CONTENT the game owns (the editor draws their gizmos). Placed
/// with values you can actually see: a bare `PointLight::default()` has a
/// 9-unit range and low intensity, which reads as "nothing happened".
fn point_light_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(Transform::from_translation(position + Vec3::Y * 2.0)).into_partial_reflect(),
        Box::new(PointLight {
            intensity: 250_000.0,
            range: 12.0,
            shadow_maps_enabled: true,
            ..default()
        })
        .into_partial_reflect(),
        Box::new(Name::new("Point Light")).into_partial_reflect(),
    ]
}

fn spot_light_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        // Aimed down: a spot pointing along -Z from head height is what you
        // want nine times out of ten, and rotating it is the fiddly part.
        Box::new(
            Transform::from_translation(position + Vec3::Y * 4.0).looking_at(position, Vec3::Z),
        )
        .into_partial_reflect(),
        Box::new(SpotLight {
            intensity: 400_000.0,
            range: 15.0,
            shadow_maps_enabled: true,
            inner_angle: 0.3,
            outer_angle: 0.6,
            ..default()
        })
        .into_partial_reflect(),
        Box::new(Name::new("Spot Light")).into_partial_reflect(),
    ]
}

fn directional_light_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(
            Transform::from_translation(position + Vec3::Y * 6.0)
                .looking_at(position + Vec3::new(2.0, 0.0, 1.0), Vec3::Y),
        )
        .into_partial_reflect(),
        Box::new(DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        })
        .into_partial_reflect(),
        Box::new(Name::new("Directional Light")).into_partial_reflect(),
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
        reg.level_validator(editor_api::validate::LevelValidatorDef {
            id: editor_api::prelude::ValidatorId::new_static("game.spinner-config"),
            name: "Spinner config sanity",
            validate: |world| {
                // The game's own level rule (owner ask): required component
                // CONFIG — an enabled spinner that can't spin is a mistake.
                let mut problems = Vec::new();
                let mut query = world.query::<(
                    &editor_core::prelude::SceneId,
                    &crate::game::Spinner,
                    Option<&Name>,
                )>();
                for (id, spinner, name) in query.iter(world) {
                    if spinner.enabled && spinner.degrees_per_sec == 0.0 {
                        problems.push(editor_api::validate::LevelProblem {
                            validator: editor_api::prelude::ValidatorId::new_static(
                                "game.spinner-config",
                            ),
                            severity: editor_api::validate::Severity::Warning,
                            message: format!(
                                "{:?}: Spinner enabled with degrees_per_sec = 0",
                                name.map(|n| n.as_str()).unwrap_or("entity")
                            ),
                            entity: Some(*id),
                        });
                    }
                }
                problems
            },
        });
        // A volume that cannot fire is worse than a missing one: it is on
        // screen, it looks armed, and it is not. Both cases are geometry the
        // designer can see but not reason about.
        reg.level_validator(editor_api::validate::LevelValidatorDef {
            id: editor_api::prelude::ValidatorId::new_static("game.trigger-volume"),
            name: "Trigger volume geometry",
            validate: |world| {
                let mut problems = Vec::new();
                let mut query = world.query::<(
                    &editor_core::prelude::SceneId,
                    &game_framework::TriggerVolume,
                    &Transform,
                    Option<&Name>,
                )>();
                for (id, volume, transform, name) in query.iter(world) {
                    let label = name.map(|n| n.as_str()).unwrap_or("Trigger");
                    let size = transform.scale.abs();
                    let thinnest = size.x.min(size.y).min(size.z);
                    let (severity, message) = if thinnest < f32::EPSILON {
                        (
                            editor_api::validate::Severity::Error,
                            format!(
                                "{label}: flattened to nothing, so '{}' can never fire",
                                volume.name
                            ),
                        )
                    } else if thinnest < 0.5 {
                        (
                            editor_api::validate::Severity::Warning,
                            format!(
                                "{label}: only {thinnest:.2}m thick — occupancy is sampled once a \
                                 frame, so something moving fast can pass straight through"
                            ),
                        )
                    } else {
                        continue;
                    };
                    problems.push(editor_api::validate::LevelProblem {
                        validator: editor_api::prelude::ValidatorId::new_static(
                            "game.trigger-volume",
                        ),
                        severity,
                        message,
                        entity: Some(*id),
                    });
                }
                problems
            },
        });
        // Physics debug view (keymap §"Space t toggles": grid, gizmos, physics
        // debug, shading). Colliders are DERIVED from `BoxCollider` data, so
        // seeing the wireframe is how you check that authored data against the
        // geometry it is supposed to hug.
        reg.action(
            editor_api::actions::ActionDef::new("view.toggle-colliders", "Toggle Colliders")
                .describe("Show or hide physics collider wireframes")
                .context("normal")
                .bind("space t p"),
        );
        reg.action(
            editor_api::actions::ActionDef::new("game.fit-collider", "Fit Collider To Bounds")
                .describe(
                    "Size a BoxCollider from the selection's visual bounds — \
                     the asset-prep verb for imported models",
                )
                .context("normal"),
        );
        // A spawn point has no geometry: the editor draws it from THIS, and the
        // pick proxy makes it clickable like anything with a mesh. A person is
        // person-sized wherever you put one, so a fixed sphere is right here.
        reg.gizmo::<crate::game::PlayerSpawn>(
            editor_api::ids::GizmoId::new_static("game.player-spawn"),
            Some(editor_api::gizmos::PickProxy::Sphere { radius: 0.5 }),
            draw_player_spawn,
        );
        // A volume is invisible: the gizmo IS the object, and the pick proxy is
        // the only reason it can be clicked at all. Its size is authored, so the
        // proxy is a unit box that inherits the transform — a room-sized volume
        // with a half-metre click target would be worse than none.
        reg.gizmo::<game_framework::TriggerVolume>(
            editor_api::ids::GizmoId::new_static("game.trigger-volume"),
            Some(editor_api::gizmos::PickProxy::UnitBox),
            draw_trigger_volume,
        );
        // A game declares its own families. `kind` and not the whole value:
        // two cubes of different sizes are both cubes, which is what a person
        // means by "select every cube".
        reg.identity::<Primitive>(
            editor_api::identity::priority::GAME,
            "kind",
            "same primitive",
        );
        reg.identity::<game_framework::TriggerVolume>(
            editor_api::identity::priority::GAME + 1,
            // Presence: two trigger volumes are the same kind of thing even
            // though one is named "lift" and the other "pit".
            "*",
            "trigger volume",
        );
        reg.component::<Transform>()
            .component::<Primitive>()
            .component::<Spinner>()
            .component::<BoxCollider>()
            .component::<AutoBoxCollider>()
            .component::<crate::game::Ground>()
            .component::<crate::game::PlayerSpawn>()
            .component::<game_framework::PostProcess>()
            .component::<game_framework::Burst>()
            .component::<game_framework::TriggerVolume>()
            .component::<game_framework::TriggerActor>()
            .component::<PhysicsBody>()
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
            // Registered so they SERIALIZE: without this a placed light looks
            // right until the scene reloads and it is gone.
            .component::<PointLight>()
            .component::<SpotLight>()
            .component::<DirectionalLight>()
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("trigger.volume"),
                display_name: "Trigger Volume",
                components: trigger_volume_components,
            })
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("light.point"),
                display_name: "Point Light",
                components: point_light_components,
            })
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("light.spot"),
                display_name: "Spot Light",
                components: spot_light_components,
            })
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("light.directional"),
                display_name: "Directional Light",
                components: directional_light_components,
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
        // Collider debug rendering is a DEVELOPMENT view: it enters the binary
        // with the editor feature, never a release build. Starts off — the
        // toggle turns it on.

        app.add_plugins(avian3d::debug_render::PhysicsDebugPlugin);
        app.add_systems(Startup, show_colliders_by_default);
        app.add_observer(guard_collider_constructor);
        app.add_observer(mark_sockets_as_decoration);
        app.add_editor_feature(GameFeature);
        app.add_systems(Startup, arm_session_restore);
        app.add_systems(Update, (answer_timeline_events, announce_triggers));
        app.add_systems(
            Update,
            demo_kit_generator.run_if(|| std::env::var("EDITOR_DEMO_KIT").is_ok()),
        );
        app.init_resource::<PhysicsProbe>();
        app.add_systems(
            Update,
            (
                // Pause must follow the input handoff in the SAME frame, or
                // reset leaks a few live sim steps before physics stops.
                (sync_game_input, crate::game::sync_physics_pause_now).chain(),
                drive_session_restore,
                handle_fit_collider,
                toggle_collider_debug,
                probe_physics.run_if(|| std::env::var("PHYSICS_PROBE").is_ok()),
            )
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

/// Authoring must never be able to CRASH the engine (spec §8: the editor is a
/// tool, not a minefield).
///
/// `ColliderConstructor`'s derived `Default` is `TrimeshFromMesh`, and avian
/// PANICS — not warns — when that lands on an entity with no `Mesh3d`. The
/// add-component palette offers every reflectable component at its default, so
/// adding this one to anything mesh-less took the whole app down. A placed
/// model is exactly that case: its meshes live in the derived gltf children,
/// never on the entity you select.
///
/// An observer runs at INSERT time, so it always beats avian's `Update` pass
/// (a guard system would race it — avian adds its own unordered).
fn guard_collider_constructor(
    add: On<Add, avian3d::prelude::ColliderConstructor>,
    constructors: Query<(&avian3d::prelude::ColliderConstructor, Has<Mesh3d>)>,
    mut commands: Commands,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    let Ok((constructor, has_mesh)) = constructors.get(add.entity) else {
        return;
    };
    if has_mesh || !constructor.requires_mesh() {
        return;
    }
    commands
        .entity(add.entity)
        .remove::<avian3d::prelude::ColliderConstructor>();
    feedback.write(editor_scene::SceneIoFeedback {
        message: "that collider is built FROM a mesh, and this entity has none — \
                  flatten the model first, or use Fit Collider"
            .into(),
        success: false,
    });
}

/// A socket is an authoring MARKER, not part of the piece's shape — its gizmo
/// would otherwise inflate every fitted collider to swallow the cone. Marked
/// here rather than in `editor_ui` (which must not know about this game's
/// physics) or in `game.rs` (which must not know about the editor): the overlay
/// is the one place that sees both.
fn mark_sockets_as_decoration(
    add: On<Add, editor_prefabs::sockets::Socket>,
    mut commands: Commands,
) {
    commands
        .entity(add.entity)
        .insert(crate::game::BoundsIgnored);
}

/// Avian's gizmo group is enabled the moment its plugin lands, and the editor
/// leaves it that way (owner direction): a collider that does not match its
/// mesh is invisible until something falls through the floor, and the whole
/// reason to look at a level in an editor is to see what the game will see.
/// Toggle it off with `␣tp` when the wireframe is in the way.
fn show_colliders_by_default(mut store: ResMut<GizmoConfigStore>) {
    store.config_mut::<PhysicsGizmos>().0.enabled = true;
}

/// `view.toggle-colliders`: flip avian's gizmo group. The debug plugin's
/// systems all early-out while the group is disabled, so the cost of "off" is
/// a bool check — and the plugin is only ever added in an editor build.
fn toggle_collider_debug(
    mut reader: MessageReader<ActionInvoked>,
    mut store: ResMut<GizmoConfigStore>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() != "view.toggle-colliders" {
            continue;
        }
        let (config, _) = store.config_mut::<PhysicsGizmos>();
        config.enabled = !config.enabled;
        feedback.write(editor_scene::SceneIoFeedback {
            message: format!(
                "collider debug {}",
                if config.enabled { "on" } else { "off" }
            ),
            success: true,
        });
    }
}

/// The editor owns input while active; the game module knows nothing about the
/// editor — it just honors `GameplayActive`.
fn sync_game_input(
    state: Res<EditorState>,
    mut game_input: ResMut<GameplayActive>,
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
    game_input: Res<GameplayActive>,
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

/// `game.fit-collider` (owner ask, asset prep): compute the selection's
/// visual bounds — every mesh Aabb in its subtree, derived gltf content
/// included — in the entity's local space, and write a `BoxCollider` through
/// the kernel (one undoable Set per entity).
pub(crate) fn handle_fit_collider(
    mut reader: MessageReader<ActionInvoked>,
    selected: Query<(&SceneId, Entity, &GlobalTransform), With<Selected>>,
    children: Query<&Children>,
    aabbs: Query<(&bevy::camera::primitives::Aabb, &GlobalTransform)>,
    ignored: Query<(), With<crate::game::BoundsIgnored>>,
    mut edits: EditScope,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() != "game.fit-collider" {
            continue;
        }
        let mut fitted = 0usize;
        for (id, root, root_global) in &selected {
            // THE bounds measurement (shared with `AutoBoxCollider`), so the
            // one-shot verb and the live one can never disagree.
            let Some((half_extents, offset)) =
                crate::game::visual_bounds(root, root_global, &children, &aabbs, &ignored)
            else {
                continue; // no meshes under this selection (still loading?)
            };
            edits
                .transaction("Fit Collider")
                .set(
                    *id,
                    BoxCollider {
                        half_extents,
                        offset,
                    },
                )
                .commit();
            fitted += 1;
        }
        feedback.write(editor_scene::SceneIoFeedback {
            message: if fitted > 0 {
                format!(
                    "collider fit to bounds on {fitted} entit{}",
                    if fitted == 1 { "y" } else { "ies" }
                )
            } else {
                "select something with visible meshes to fit a collider".into()
            },
            success: fitted > 0,
        });
    }
}

/// PHYSICS_PROBE: the avian loop end-to-end — paused while editing, fit
/// collider from bounds, dynamic prop falls in play, reset restores.
#[derive(Resource, Default)]
pub(crate) struct PhysicsProbe {
    frame: u32,
    failures: Vec<String>,
    prop: Option<SceneId>,
}

pub(crate) fn probe_physics(world: &mut World) {
    use bevy::reflect::PartialReflect;
    use editor_api::edits::{EditQueue, Op, Transaction};

    fn check(world: &mut World, ok: bool, what: &str) {
        if ok {
            info!("PHYSICS-PROBE PASS: {what}");
        } else {
            error!("PHYSICS-PROBE FAIL: {what}");
            world
                .resource_mut::<PhysicsProbe>()
                .failures
                .push(what.to_string());
        }
    }
    fn invoke(world: &mut World, action: &'static str) {
        world.write_message(ActionInvoked {
            action: ActionId::new_static(action),
            args: None,
            source: InvocationSource::Test,
        });
    }
    fn prop_y(world: &mut World) -> Option<f32> {
        let id = world.resource::<PhysicsProbe>().prop?;
        let entity = world.resource::<SceneIndex>().get(&id)?;
        world.get::<Transform>(entity).map(|t| t.translation.y)
    }

    world.resource_mut::<PhysicsProbe>().frame += 1;
    let frame = world.resource::<PhysicsProbe>().frame;
    match frame {
        1 => {
            info!("PHYSICS-PROBE armed");
        }
        60 => {
            let window = world
                .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
                .iter(world)
                .next()
                .unwrap_or(Entity::PLACEHOLDER);
            world.write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: bevy::input::keyboard::Key::Enter,
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        }
        120 => invoke(world, "core.toggle-editor"),
        // Stale props from a previous run's play-save.
        160 => {
            let stale: Vec<SceneId> = {
                let mut query = world.query::<(&SceneId, &Name)>();
                query
                    .iter(world)
                    .filter(|(_, n)| n.as_str() == "physics-prop")
                    .map(|(id, _)| *id)
                    .collect()
            };
            if !stale.is_empty() {
                world.resource_mut::<EditQueue>().0.push(Transaction {
                    label: "probe cleanup".into(),
                    gesture: None,
                    ops: stale.into_iter().map(|id| Op::Despawn { id }).collect(),
                });
            }
        }
        // A dynamic cube 5m up, through the kernel like any placement.
        200 => {
            let id = SceneId::random();
            world.resource_mut::<PhysicsProbe>().prop = Some(id);
            world.resource_mut::<EditQueue>().0.push(Transaction {
                label: "probe spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Transform::from_xyz(6.0, 5.0, 6.0)).into_partial_reflect(),
                        Box::new(Name::new("physics-prop")).into_partial_reflect(),
                        Box::new(Primitive {
                            kind: PrimitiveKind::Cube,
                            size: 1.0,
                        })
                        .into_partial_reflect(),
                        Box::new(BoxCollider {
                            half_extents: Vec3::splat(2.0), // deliberately wrong
                            offset: Vec3::ZERO,
                        })
                        .into_partial_reflect(),
                        Box::new(PhysicsBody::Dynamic).into_partial_reflect(),
                    ],
                }],
            });
        }
        // Editing: the paused simulation must not move it.
        320 => {
            let held = prop_y(world).is_some_and(|y| (y - 5.0).abs() < 0.001);
            check(
                world,
                held,
                "physics holds still while the editor owns input",
            );
            if let Some(id) = world.resource::<PhysicsProbe>().prop {
                world
                    .resource_mut::<editor_core::selection::PendingSelect>()
                    .0 = Some(id);
            }
        }
        360 => invoke(world, "game.fit-collider"),
        420 => {
            let fitted = {
                let id = world.resource::<PhysicsProbe>().prop;
                id.and_then(|id| world.resource::<SceneIndex>().get(&id))
                    .and_then(|e| world.get::<BoxCollider>(e))
                    .is_some_and(|c| (c.half_extents - Vec3::splat(0.5)).length() < 0.01)
            };
            check(
                world,
                fitted,
                "Fit Collider sized the box from the mesh bounds",
            );
        }
        // The collider debug view: ON by default (owner direction), off after
        // one toggle, on again after a second.
        424 => {
            let on = world
                .resource::<GizmoConfigStore>()
                .config::<PhysicsGizmos>()
                .0
                .enabled;
            check(world, on, "collider debug starts ON");
            invoke(world, "view.toggle-colliders");
        }
        428 => {
            let off = !world
                .resource::<GizmoConfigStore>()
                .config::<PhysicsGizmos>()
                .0
                .enabled;
            check(
                world,
                off,
                "view.toggle-colliders turned the wireframes off",
            );
        }
        432 => invoke(world, "view.toggle-colliders"),
        436 => {
            let on = world
                .resource::<GizmoConfigStore>()
                .config::<PhysicsGizmos>()
                .0
                .enabled;
            check(world, on, "toggling again turned them back on");
            // Leave it ON for the play stage — the debug view must survive the
            // editor→game handoff, which is where colliders matter most.
        }
        // Authoring must not be able to crash the engine: the palette hands
        // every component its reflected DEFAULT, and avian's is a mesh-only
        // constructor that panics on anything mesh-less.
        438 => {
            let target = world
                .resource::<PhysicsProbe>()
                .prop
                .and_then(|id| world.resource::<SceneIndex>().get(&id));
            if let Some(entity) = target {
                world
                    .entity_mut(entity)
                    .insert(avian3d::prelude::ColliderConstructor::default());
            }
        }
        439 => {
            let target = world
                .resource::<PhysicsProbe>()
                .prop
                .and_then(|id| world.resource::<SceneIndex>().get(&id));
            let refused = target.is_some_and(|entity| {
                world
                    .get::<avian3d::prelude::ColliderConstructor>(entity)
                    .is_none()
            });
            check(
                world,
                refused,
                "a mesh-only ColliderConstructor is refused, not fatal",
            );
        }
        // AutoBoxCollider fits the CONTENT and keeps it fitted — the probe's
        // prop is a 1m cube, so the fit must land on 0.5 half-extents whatever
        // the authored BoxCollider said (it was deliberately wrong at 2.0).
        442 => {
            if let Some(entity) = world
                .resource::<PhysicsProbe>()
                .prop
                .and_then(|id| world.resource::<SceneIndex>().get(&id))
            {
                world
                    .entity_mut(entity)
                    .insert(crate::game::AutoBoxCollider)
                    .insert(BoxCollider {
                        half_extents: Vec3::splat(2.0),
                        offset: Vec3::ZERO,
                    });
            }
        }
        448 => {
            let fitted = world
                .resource::<PhysicsProbe>()
                .prop
                .and_then(|id| world.resource::<SceneIndex>().get(&id))
                .and_then(|entity| world.get::<crate::game::AutoFitted>(entity))
                .map(|f| (f.half_extents, f.offset));
            check(
                world,
                fitted.is_some_and(|(he, _)| he.abs_diff_eq(Vec3::splat(0.5), 0.01)),
                &format!("AutoBoxCollider fit the box to the content ({fitted:?})"),
            );
            // It OWNS the collider: the stale manual 2.0 must not win.
            let overridden = world
                .resource::<PhysicsProbe>()
                .prop
                .and_then(|id| world.resource::<SceneIndex>().get(&id))
                .and_then(|entity| world.get::<Children>(entity).map(|c| c.len()))
                .is_some_and(|kids| kids > 0);
            check(world, overridden, "the auto fit owns the derived collider");
        }
        440 => invoke(world, "editor.play"),
        620 => {
            let fell = prop_y(world).is_some_and(|y| y < 4.0);
            check(world, fell, "the dynamic prop falls under avian in play");
            let still_on = world
                .resource::<GizmoConfigStore>()
                .config::<PhysicsGizmos>()
                .0
                .enabled;
            check(world, still_on, "the debug view survives into play");
        }
        640 => invoke(world, "editor.reset"),
        760 => {
            // Back at its AUTHORED spot (vs. < 4.0 where it fell) — a frame of
            // engine settling is fine, staying fallen is not.
            let y = prop_y(world);
            check(
                world,
                y.is_some_and(|y| (y - 5.0).abs() < 0.05),
                &format!("reset restores the pre-play physics state (y={y:?})"),
            );
        }
        800 => {
            if let Some(id) = world.resource::<PhysicsProbe>().prop {
                world.resource_mut::<EditQueue>().0.push(Transaction {
                    label: "probe cleanup".into(),
                    gesture: None,
                    ops: vec![Op::Despawn { id }],
                });
            }
        }
        860 => {
            let failures = world.resource::<PhysicsProbe>().failures.clone();
            if failures.is_empty() {
                info!("PHYSICS-PROBE PASS: the avian loop end-to-end");
                world.write_message(bevy::app::AppExit::Success);
            } else {
                error!("PHYSICS-PROBE FAILED: {failures:?}");
                world.write_message(bevy::app::AppExit::error());
            }
        }
        _ => {}
    }
}

/// A trigger volume has no geometry, so the editor draws it from its data —
/// the box it actually watches, at the size it actually is.
///
/// Gizmos cannot draw text, and a level with six identical wire boxes is a
/// level where nobody knows which one is the checkpoint. The colour comes from
/// the CUE NAME, so two volumes that fire different things look different and
/// two that fire the same thing match — which is exactly the fact a designer
/// needs at a glance, and it is true rather than decorative.
fn draw_trigger_volume(cx: &mut editor_api::gizmos::GizmoCx) {
    let volume = cx
        .read::<game_framework::TriggerVolume>()
        .unwrap_or_default();
    let hue = hue_for(&volume.name);
    let shape = Transform::from(cx.transform);
    let degenerate = shape.scale.x.abs() < f32::EPSILON
        || shape.scale.y.abs() < f32::EPSILON
        || shape.scale.z.abs() < f32::EPSILON;
    let color = match (cx.selected, degenerate) {
        // A volume flattened on an axis catches nothing. Drawing it in the
        // problem colour is the difference between "I see my kill plane" and
        // "I see a box that cannot fire" — the picture must not promise
        // behaviour the rules will not deliver.
        (_, true) => Color::srgb(0.95, 0.35, 0.35),
        (true, false) => Color::hsl(hue, 0.85, 0.65),
        (false, false) => Color::hsla(hue, 0.7, 0.55, 0.55),
    };
    // The box IS the transform: a unit cube scaled by what the designer scaled.
    cx.painter.cuboid(shape, color);
    // Where it MEETS THE FLOOR. A box seen edge-on is a line, and the ring is
    // how you find the thing you are looking straight through.
    let floor = cx.at() + cx.dir(Vec3::NEG_Y) * (shape.scale.y * 0.5);
    cx.painter.circle(floor, Vec3::Y, 0.35, color);
    if volume.once {
        // One-shot: an inner box, because "fires every time" and "fires once
        // and is over" are different objects and should not look identical.
        let mut inner = shape;
        inner.scale *= 0.55;
        cx.painter.cuboid(inner, color);
    }
}

/// A stable hue in 0..360 for a cue name.
///
/// Deliberately not `DefaultHasher`: that is seeded per process, so the
/// checkpoint would be a different colour every launch and colour could carry
/// no meaning at all. FNV-1a is fixed forever.
fn hue_for(name: &str) -> f32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in name.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    // Golden-angle spacing so two names that hash close together still land on
    // visibly different colours.
    ((hash % 360) as f32 * 137.508) % 360.0
}

/// The placed volume is a WORKING EXAMPLE, not a bare component.
///
/// A designer who places a trigger and walks through it should see something
/// happen on the first try. A lone `TriggerVolume` fires a cue that nothing in
/// the level answers, so the preset brings its own emitter, already listening
/// for the cue the volume already sends. Place one thing, walk in, particles —
/// then rename both to mean whatever the level needs.
fn trigger_volume_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    const SIZE: f32 = 3.0;
    let cue = "burst";
    vec![
        // Sitting ON the floor rather than half-buried in it: you walk into a
        // volume, and burying half of it is the classic first annoyance.
        Box::new(
            Transform::from_translation(position + Vec3::Y * (SIZE * 0.5))
                .with_scale(Vec3::splat(SIZE)),
        )
        .into_partial_reflect(),
        Box::new(game_framework::TriggerVolume {
            name: cue.into(),
            once: false,
        })
        .into_partial_reflect(),
        Box::new(game_framework::Burst {
            event: cue.into(),
            ..default()
        })
        .into_partial_reflect(),
        // The entity label and the cue agree at birth. They are different
        // strings on purpose — the cue is what the game matches on, the label
        // is what the hierarchy shows — but they should not start out lying
        // about each other.
        Box::new(Name::new("Trigger: burst")).into_partial_reflect(),
    ]
}

/// Say so, in the editor, when a trigger fires during an in-editor play session.
///
/// A volume you walked through either fired or did not, and at 60fps a colour
/// flash is not an answer. The toast is: `trigger 'burst' entered`. It lives
/// here rather than in `game_framework` because it is a DIAGNOSTIC for the
/// person authoring the level, not behaviour the shipped game has.
fn announce_triggers(
    mut entered: MessageReader<game_framework::TriggerEntered>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for trigger in entered.read() {
        feedback.write(editor_scene::SceneIoFeedback {
            message: format!("trigger '{}' entered", trigger.name),
            success: true,
        });
    }
}

/// The player spawn widget: a figure standing at the point, facing the way the
/// player will look. Reads `eye_height` off the component, so the gizmo shows
/// the DATA rather than a fixed glyph — edit the field and the drawing follows.
fn draw_player_spawn(cx: &mut editor_api::gizmos::GizmoCx) {
    let accent = if cx.selected {
        Color::srgb(0.98, 0.78, 0.35)
    } else {
        Color::srgba(0.98, 0.78, 0.35, 0.5)
    };
    let base = cx.at();
    let eye_height = cx
        .read::<crate::game::PlayerSpawn>()
        .map(|spawn| spawn.eye_height)
        .unwrap_or(1.7)
        .max(0.1);
    let eye = base + Vec3::Y * eye_height;
    // A stick figure reads as "a person stands here" at any distance.
    cx.painter.circle(base, Vec3::Y, 0.35, accent);
    cx.painter.line(base, eye, accent);
    cx.painter.sphere(eye, 0.16, accent);
    // Facing: which way you are looking when the level starts.
    let forward = cx.dir(Vec3::NEG_Z);
    cx.painter.arrow(eye, eye + forward * 1.2, accent);
}

/// The reference game answering a timeline event (spec §9: a sequencer fires
/// events, and the GAME decides what they mean).
///
/// This is the half of the contract that proves the other half is worth having:
/// the timeline says `"spin"` happened at 1.4s and knows nothing more, and the
/// game turns that into behaviour it already owns. Matching on the NAME is the
/// whole interface — no editor type crosses into gameplay logic.
///
/// It lives in the overlay because the timeline currently ships with the editor
/// and not with the game (see the note in spec §9): a released build has no
/// sequencer to listen to yet. When the runtime moves, this moves with it and
/// stops being editor-gated.
fn answer_timeline_events(
    mut events: MessageReader<editor_scene::anim::TimelineEvent>,
    mut spinners: Query<&mut crate::game::Spinner>,
    mut effects: MessageWriter<game_framework::FireEffect>,
) {
    for event in events.read() {
        // EVERY event becomes an effect cue as well. An emitter that wants this
        // name will fire; nothing else notices. That is the whole translation —
        // and when the sequencer moves game-side, this line is what disappears.
        effects.write(game_framework::FireEffect {
            name: event.name.clone(),
        });
        match event.name.as_str() {
            "spin" => {
                for mut spinner in &mut spinners {
                    spinner.enabled = true;
                }
            }
            "stop" => {
                for mut spinner in &mut spinners {
                    spinner.enabled = false;
                }
            }
            _ => {}
        }
    }
}
