use avian3d::debug_render::PhysicsGizmos;
use avian3d::prelude::*;
use avian3d::spatial_query::SpatialQueryPipeline;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::prelude::*;
use bevy_editor_game::{
    GameEntity, GamePausedEvent, GameResetEvent, GameResumedEvent, GameStartedEvent, GameState,
    PauseEvent, PlayEvent, ResetEvent,
};
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use bevy_infinite_grid::InfiniteGridSettings;

use crate::editor::{EditorMode, EditorState, TransformOperation};
use crate::scene::{build_editor_scene, restore_scene_from_data, SceneEntity};
use crate::selection::Selected;
use crate::ui::colors;

/// Holds the pre-play scene snapshot for reset
#[derive(Resource, Default)]
pub struct GameSnapshot {
    pub data: Option<String>,
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        // State, resources, and events are registered in EditorStatePlugin
        // so the editor always has the types available. This plugin adds the
        // actual input handling and play/pause/reset command execution.
        app.add_systems(Update, handle_play_input)
            .add_systems(Update, (handle_play, handle_pause, handle_reset))
            .add_systems(EguiPrimaryContextPass, draw_play_controls);
    }
}

/// Handle play/pause/reset hotkeys
fn handle_play_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    game_state: Res<State<GameState>>,
    mut play_events: MessageWriter<PlayEvent>,
    mut pause_events: MessageWriter<PauseEvent>,
    mut reset_events: MessageWriter<ResetEvent>,
) {
    // F4: Play or Resume
    if keyboard.just_pressed(KeyCode::F4) {
        match game_state.get() {
            GameState::Editing | GameState::Paused => {
                play_events.write(PlayEvent);
            }
            GameState::Playing => {} // Already playing
        }
    }

    // F6: Pause
    if keyboard.just_pressed(KeyCode::F6) {
        if *game_state.get() == GameState::Playing {
            pause_events.write(PauseEvent);
        }
    }

    // F7: Reset
    if keyboard.just_pressed(KeyCode::F7) {
        match game_state.get() {
            GameState::Playing | GameState::Paused => {
                reset_events.write(ResetEvent);
            }
            GameState::Editing => {} // Nothing to reset
        }
    }

    // Escape while playing -> pause
    if keyboard.just_pressed(KeyCode::Escape) && *game_state.get() == GameState::Playing {
        pause_events.write(PauseEvent);
    }
}

/// Handle play events — queue a command for exclusive world access
fn handle_play(
    mut events: MessageReader<PlayEvent>,
    mut commands: Commands,
    game_state: Res<State<GameState>>,
) {
    for _ in events.read() {
        match game_state.get() {
            GameState::Editing => {
                commands.queue(PlayCommand { from_editing: true });
            }
            GameState::Paused => {
                commands.queue(PlayCommand { from_editing: false });
            }
            GameState::Playing => {}
        }
    }
}

/// Handle pause events — queue a command for exclusive world access
fn handle_pause(
    mut events: MessageReader<PauseEvent>,
    game_state: Res<State<GameState>>,
    mut commands: Commands,
) {
    for _ in events.read() {
        if *game_state.get() == GameState::Playing {
            commands.queue(PauseCommand);
        }
    }
}

/// Handle reset events — queue a command for exclusive world access
fn handle_reset(
    mut events: MessageReader<ResetEvent>,
    game_state: Res<State<GameState>>,
    mut commands: Commands,
) {
    for _ in events.read() {
        if *game_state.get() == GameState::Editing {
            continue;
        }
        commands.queue(ResetCommand);
    }
}

/// Set grid visibility from exclusive world access.
fn set_grid_visibility(world: &mut World, visible: bool) {
    let grid_entities: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, With<InfiniteGridSettings>>();
        q.iter(world).collect()
    };
    let vis = if visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for entity in grid_entities {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut visibility) = entity_mut.get_mut::<Visibility>() {
                *visibility = vis;
            }
        }
    }
}

/// Command to start playing with exclusive world access
struct PlayCommand {
    from_editing: bool,
}

impl Command for PlayCommand {
    fn apply(self, world: &mut World) {
        // Only snapshot when transitioning from Editing (not from Paused)
        if self.from_editing {
            let scene_entity_ids: Vec<Entity> = {
                let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
                query.iter(world).collect()
            };

            let scene = build_editor_scene(world, scene_entity_ids.into_iter());

            let type_registry = world.resource::<AppTypeRegistry>().clone();
            let type_registry = type_registry.read();

            if let Ok(serialized) = scene.serialize(&type_registry) {
                drop(type_registry);
                if let Some(mut snapshot) = world.get_resource_mut::<GameSnapshot>() {
                    snapshot.data = Some(serialized);
                }
                info!("Game snapshot taken");
            } else {
                drop(type_registry);
                warn!("Failed to serialize game snapshot");
            }
        }

        // Enable physics
        if let Some(mut physics_time) = world.get_resource_mut::<Time<Physics>>() {
            physics_time.set_relative_speed(1.0);
        }

        // Disable editor
        if let Some(mut editor_state) = world.get_resource_mut::<EditorState>() {
            editor_state.editor_active = false;
            editor_state.ui_enabled = false;
            editor_state.gizmos_visible = false;
        }

        // Enter preview mode: hide physics debug gizmos and grid
        if let Some(mut gizmo_config) = world.get_resource_mut::<GizmoConfigStore>() {
            gizmo_config.config_mut::<PhysicsGizmos>().0.enabled = false;
        }
        set_grid_visibility(world, false);


        // Transition state
        if let Some(mut next_state) = world.get_resource_mut::<NextState<GameState>>() {
            next_state.set(GameState::Playing);
        }

        // Fire lifecycle event
        if self.from_editing {
            world.write_message(GameStartedEvent);
        } else {
            world.write_message(GameResumedEvent);
        }

        info!("Game: PLAYING");
    }
}

/// Command to pause with exclusive world access
struct PauseCommand;

impl Command for PauseCommand {
    fn apply(self, world: &mut World) {
        // Pause physics
        if let Some(mut physics_time) = world.get_resource_mut::<Time<Physics>>() {
            physics_time.set_relative_speed(0.0);
        }

        // Re-enable editor
        if let Some(mut editor_state) = world.get_resource_mut::<EditorState>() {
            editor_state.editor_active = true;
            editor_state.ui_enabled = true;
            editor_state.gizmos_visible = true;
        }

        // Exit preview mode: restore physics debug gizmos and grid
        if let Some(mut gizmo_config) = world.get_resource_mut::<GizmoConfigStore>() {
            gizmo_config.config_mut::<PhysicsGizmos>().0.enabled = true;
        }
        set_grid_visibility(world, true);


        // Force View mode and clear transform operations
        if let Some(mut next_mode) = world.get_resource_mut::<NextState<EditorMode>>() {
            next_mode.set(EditorMode::View);
        }
        if let Some(mut op) = world.get_resource_mut::<TransformOperation>() {
            *op = TransformOperation::None;
        }

        // Transition state
        if let Some(mut next_state) = world.get_resource_mut::<NextState<GameState>>() {
            next_state.set(GameState::Paused);
        }

        // Fire lifecycle event
        world.write_message(GamePausedEvent);

        info!("Game: PAUSED");
    }
}

/// Draw play/pause/reset control buttons.
/// Visible in all game states (not gated by ui_enabled).
fn draw_play_controls(
    mut contexts: EguiContexts,
    game_state: Res<State<GameState>>,
    mut play_events: MessageWriter<PlayEvent>,
    mut pause_events: MessageWriter<PauseEvent>,
    mut reset_events: MessageWriter<ResetEvent>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::Area::new(egui::Id::new("play_controls"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 8.0])
        .show(ctx, |ui| {
            egui::Frame::popup(&ctx.style())
                .fill(colors::BG_DARK)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        match game_state.get() {
                            GameState::Editing => {
                                if ui
                                    .button(egui::RichText::new("\u{25B6} Play").color(colors::STATUS_SUCCESS))
                                    .clicked()
                                {
                                    play_events.write(PlayEvent);
                                }
                            }
                            GameState::Playing => {
                                if ui
                                    .button(egui::RichText::new("\u{23F8} Pause").color(colors::STATUS_WARNING))
                                    .clicked()
                                {
                                    pause_events.write(PauseEvent);
                                }
                                if ui
                                    .button(egui::RichText::new("\u{25A0} Reset").color(colors::STATUS_ERROR))
                                    .clicked()
                                {
                                    reset_events.write(ResetEvent);
                                }
                            }
                            GameState::Paused => {
                                if ui
                                    .button(egui::RichText::new("\u{25B6} Resume").color(colors::STATUS_SUCCESS))
                                    .clicked()
                                {
                                    play_events.write(PlayEvent);
                                }
                                if ui
                                    .button(egui::RichText::new("\u{25A0} Reset").color(colors::STATUS_ERROR))
                                    .clicked()
                                {
                                    reset_events.write(ResetEvent);
                                }
                            }
                        }
                    });
                });
        });

    Ok(())
}

/// Command to reset scene to pre-play state with exclusive world access
struct ResetCommand;

impl Command for ResetCommand {
    fn apply(self, world: &mut World) {
        // Despawn all GameEntity-marked entities before restoring scene
        let game_entities: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<GameEntity>>();
            q.iter(world).collect()
        };
        for entity in game_entities {
            world.despawn(entity);
        }

        // Clear selection before restoring — prevents sync_selection_outlines
        // from queuing MeshOutline inserts on entities about to be despawned
        let selected: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<Selected>>();
            q.iter(world).collect()
        };
        for entity in selected {
            if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                entity_mut.remove::<Selected>();
            }
        }

        // Get snapshot data
        let data = world
            .get_resource::<GameSnapshot>()
            .and_then(|s| s.data.clone());

        if let Some(data) = data {
            restore_scene_from_data(world, &data);
            rebuild_spatial_query_pipeline(world);
            info!("Scene restored from game snapshot");
        } else {
            warn!("No game snapshot to restore");
        }

        // Re-enable physics so the spatial query pipeline stays updated
        // (normal editing state has physics running).
        if let Some(mut physics_time) = world.get_resource_mut::<Time<Physics>>() {
            physics_time.set_relative_speed(1.0);
        }

        // Re-enable editor
        if let Some(mut editor_state) = world.get_resource_mut::<EditorState>() {
            editor_state.editor_active = true;
            editor_state.ui_enabled = true;
            editor_state.gizmos_visible = true;
        }

        // Exit preview mode: restore physics debug gizmos and grid
        if let Some(mut gizmo_config) = world.get_resource_mut::<GizmoConfigStore>() {
            gizmo_config.config_mut::<PhysicsGizmos>().0.enabled = true;
        }
        set_grid_visibility(world, true);


        // Clear snapshot
        if let Some(mut snapshot) = world.get_resource_mut::<GameSnapshot>() {
            snapshot.data = None;
        }

        // Transition state
        if let Some(mut next_state) = world.get_resource_mut::<NextState<GameState>>() {
            next_state.set(GameState::Editing);
        }

        // Fire lifecycle event
        world.write_message(GameResetEvent);

        info!("Game: RESET to Editing");
    }
}

/// Sync physics Position/Rotation from Transform for all collider entities,
/// then rebuild the SpatialQueryPipeline BVH so `SpatialQuery::cast_ray`
/// works immediately without waiting for the physics schedule to tick.
fn rebuild_spatial_query_pipeline(world: &mut World) {
    // Sync Position/Rotation from Transform for newly-created colliders
    // that still have PLACEHOLDER values.
    let to_sync: Vec<(Entity, Vec3, Quat)> = {
        let mut q = world.query::<(Entity, &Transform, &Position)>();
        q.iter(world)
            .filter(|(_, _, pos)| **pos == Position::PLACEHOLDER)
            .map(|(e, t, _)| (e, t.translation, t.rotation))
            .collect()
    };
    for (entity, translation, rotation) in to_sync {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut pos) = entity_mut.get_mut::<Position>() {
                *pos = Position::new(translation);
            }
            if let Some(mut rot) = entity_mut.get_mut::<Rotation>() {
                *rot = Rotation::from(rotation);
            }
        }
    }

    // Collect collider data and rebuild the spatial query BVH.
    let collider_data: Vec<(Entity, Position, Rotation, Collider, CollisionLayers)> = {
        let mut q = world.query_filtered::<
            (Entity, &Position, &Rotation, &Collider, &CollisionLayers),
            Without<ColliderDisabled>,
        >();
        q.iter(world)
            .map(|(e, p, r, c, l)| (e, *p, *r, c.clone(), *l))
            .collect()
    };
    if let Some(mut pipeline) = world.get_resource_mut::<SpatialQueryPipeline>() {
        pipeline.update(
            collider_data
                .iter()
                .map(|(e, p, r, c, l)| (*e, p, r, c, l)),
        );
    }
}
