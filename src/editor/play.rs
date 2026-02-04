use avian3d::debug_render::PhysicsGizmos;
use avian3d::prelude::Physics;
use avian3d::schedule::PhysicsTime;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use bevy_infinite_grid::InfiniteGridSettings;

use crate::editor::{EditorCamera, EditorState, GameCamera};
use crate::scene::{build_editor_scene, restore_scene_from_data, SceneEntity};
use crate::ui::colors;

/// Simulation state orthogonal to EditorMode.
///
/// Controls whether physics is running and the editor is active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum SimulationState {
    /// Physics paused, editor active (default state)
    #[default]
    Editing,
    /// Physics running, editor hidden, game logic runs
    Playing,
    /// Physics paused, editor overlays shown
    Paused,
}

/// Event to start playing (or resume from paused)
#[derive(Message)]
pub struct PlayEvent;

/// Event to pause while playing
#[derive(Message)]
pub struct PauseEvent;

/// Event to reset scene to pre-play state
#[derive(Message)]
pub struct ResetEvent;

/// Holds the pre-play scene snapshot for reset
#[derive(Resource, Default)]
pub struct PlaySnapshot {
    pub data: Option<String>,
}

pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        // State, resources, and events are registered in EditorStatePlugin
        // so the editor always has the types available. This plugin adds the
        // actual input handling and play/pause/reset command execution.
        app.add_systems(Update, handle_play_input)
            .add_systems(
                Update,
                (handle_play, handle_pause, handle_reset),
            )
            .add_systems(EguiPrimaryContextPass, draw_play_controls);
    }
}

/// Handle play/pause/reset hotkeys
fn handle_play_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    sim_state: Res<State<SimulationState>>,
    mut play_events: MessageWriter<PlayEvent>,
    mut pause_events: MessageWriter<PauseEvent>,
    mut reset_events: MessageWriter<ResetEvent>,
) {
    // F5: Play or Resume
    if keyboard.just_pressed(KeyCode::F5) {
        match sim_state.get() {
            SimulationState::Editing | SimulationState::Paused => {
                play_events.write(PlayEvent);
            }
            SimulationState::Playing => {} // Already playing
        }
    }

    // F6: Pause
    if keyboard.just_pressed(KeyCode::F6) {
        if *sim_state.get() == SimulationState::Playing {
            pause_events.write(PauseEvent);
        }
    }

    // F7: Reset
    if keyboard.just_pressed(KeyCode::F7) {
        match sim_state.get() {
            SimulationState::Playing | SimulationState::Paused => {
                reset_events.write(ResetEvent);
            }
            SimulationState::Editing => {} // Nothing to reset
        }
    }

    // Escape while playing → pause
    if keyboard.just_pressed(KeyCode::Escape) && *sim_state.get() == SimulationState::Playing {
        pause_events.write(PauseEvent);
    }
}

/// Handle play events — queue a command for exclusive world access
fn handle_play(
    mut events: MessageReader<PlayEvent>,
    mut commands: Commands,
    sim_state: Res<State<SimulationState>>,
) {
    for _ in events.read() {
        match sim_state.get() {
            SimulationState::Editing => {
                commands.queue(PlayCommand { from_editing: true });
            }
            SimulationState::Paused => {
                commands.queue(PlayCommand { from_editing: false });
            }
            SimulationState::Playing => {}
        }
    }
}

/// Handle pause events — queue a command for exclusive world access
fn handle_pause(
    mut events: MessageReader<PauseEvent>,
    sim_state: Res<State<SimulationState>>,
    mut commands: Commands,
) {
    for _ in events.read() {
        if *sim_state.get() == SimulationState::Playing {
            commands.queue(PauseCommand);
        }
    }
}

/// Handle reset events — queue a command for exclusive world access
fn handle_reset(
    mut events: MessageReader<ResetEvent>,
    sim_state: Res<State<SimulationState>>,
    mut commands: Commands,
) {
    for _ in events.read() {
        if *sim_state.get() == SimulationState::Editing {
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

/// Sync camera active states based on editor_active.
/// Called from exclusive Commands since ToggleEditorEvent would conflict.
///
/// The editor camera is kept always active so that `PrimaryEguiContext`
/// (which lives on it) keeps working. During play mode the game camera
/// renders at a higher order and overwrites the editor camera output.
fn sync_cameras(world: &mut World) {
    let editor_active = world
        .get_resource::<EditorState>()
        .map(|s| s.editor_active)
        .unwrap_or(true);

    // Editor camera stays active at all times for egui rendering.
    // No need to toggle it.

    let game_cam_entities: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, (With<GameCamera>, Without<EditorCamera>)>();
        q.iter(world).collect()
    };
    for entity in game_cam_entities {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut camera) = entity_mut.get_mut::<Camera>() {
                camera.is_active = !editor_active;
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
                if let Some(mut snapshot) = world.get_resource_mut::<PlaySnapshot>() {
                    snapshot.data = Some(serialized);
                }
                info!("Play snapshot taken");
            } else {
                drop(type_registry);
                warn!("Failed to serialize play snapshot");
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

        // Sync cameras
        sync_cameras(world);

        // Transition state
        if let Some(mut next_state) = world.get_resource_mut::<NextState<SimulationState>>() {
            next_state.set(SimulationState::Playing);
        }

        info!("Simulation: PLAYING");
    }
}

/// Command to pause simulation with exclusive world access
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

        // Sync cameras
        sync_cameras(world);

        // Transition state
        if let Some(mut next_state) = world.get_resource_mut::<NextState<SimulationState>>() {
            next_state.set(SimulationState::Paused);
        }

        info!("Simulation: PAUSED");
    }
}

/// Draw play/pause/reset control buttons.
/// Visible in all simulation states (not gated by ui_enabled).
fn draw_play_controls(
    mut contexts: EguiContexts,
    sim_state: Res<State<SimulationState>>,
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
                        match sim_state.get() {
                            SimulationState::Editing => {
                                if ui
                                    .button(egui::RichText::new("\u{25B6} Play").color(colors::STATUS_SUCCESS))
                                    .clicked()
                                {
                                    play_events.write(PlayEvent);
                                }
                            }
                            SimulationState::Playing => {
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
                            SimulationState::Paused => {
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
        // Get snapshot data
        let data = world
            .get_resource::<PlaySnapshot>()
            .and_then(|s| s.data.clone());

        if let Some(data) = data {
            restore_scene_from_data(world, &data);
            info!("Scene restored from play snapshot");
        } else {
            warn!("No play snapshot to restore");
        }

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

        // Sync cameras
        sync_cameras(world);

        // Clear snapshot
        if let Some(mut snapshot) = world.get_resource_mut::<PlaySnapshot>() {
            snapshot.data = None;
        }

        // Transition state
        if let Some(mut next_state) = world.get_resource_mut::<NextState<SimulationState>>() {
            next_state.set(SimulationState::Editing);
        }

        info!("Simulation: RESET to Editing");
    }
}
