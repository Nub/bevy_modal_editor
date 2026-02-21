use bevy::prelude::*;
use bevy_editor_game::MaterialLibrary;
use bevy_egui::egui;
use std::path::PathBuf;

use super::registry::PrefabRegistry;
use super::spawn::{ClosePrefabEvent, OpenPrefabEvent};
use crate::editor::{EditorCamera, EditorState};
use crate::scene::{build_editor_scene, LoadSceneEvent, SaveSceneEvent, SceneEntity, SceneFile};
use crate::selection::Selected;
use crate::ui::theme::{colors, window_frame};

/// Resource present when editing a prefab (not the main scene).
/// Stores the parent scene state for restoration on close.
#[derive(Resource)]
pub struct PrefabEditingContext {
    pub prefab_name: String,
    pub prefab_dir: PathBuf,
    /// Serialized RON of the parent scene (all SceneEntities)
    pub parent_scene_snapshot: String,
    /// The parent scene file path (if any)
    pub parent_scene_path: Option<String>,
    /// Whether the parent scene had unsaved modifications
    pub parent_scene_modified: bool,
    /// Parent material library
    pub parent_material_library: MaterialLibrary,
    /// Parent camera transform
    pub parent_camera_transform: Transform,
}

/// Command to snapshot the current scene, then load the prefab
struct OpenPrefabCommand {
    prefab_name: String,
    prefab_dir: PathBuf,
    scene_path: PathBuf,
}

impl Command for OpenPrefabCommand {
    fn apply(self, world: &mut World) {
        // Snapshot the current scene
        let scene_entity_ids: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
            query.iter(world).collect()
        };

        let scene = build_editor_scene(world, scene_entity_ids.iter().copied());

        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();

        let snapshot = match scene.serialize(&type_registry) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to snapshot scene before opening prefab: {:?}", e);
                return;
            }
        };

        drop(type_registry);

        // Capture parent state
        let parent_scene_path = world
            .get_resource::<SceneFile>()
            .and_then(|sf| sf.path.clone());
        let parent_scene_modified = world
            .get_resource::<SceneFile>()
            .map(|sf| sf.modified)
            .unwrap_or(false);
        let parent_material_library = world
            .get_resource::<MaterialLibrary>()
            .cloned()
            .unwrap_or_default();

        let parent_camera_transform = {
            let mut query = world.query_filtered::<&Transform, With<EditorCamera>>();
            query
                .iter(world)
                .next()
                .copied()
                .unwrap_or_default()
        };

        // Insert the editing context
        world.insert_resource(PrefabEditingContext {
            prefab_name: self.prefab_name.clone(),
            prefab_dir: self.prefab_dir,
            parent_scene_snapshot: snapshot,
            parent_scene_path,
            parent_scene_modified,
            parent_material_library,
            parent_camera_transform,
        });

        // Clear selection before despawning to avoid stale outline commands
        let selected_entities: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<Selected>>();
            query.iter(world).collect()
        };
        for entity in selected_entities {
            if let Ok(mut e) = world.get_entity_mut(entity) {
                e.remove::<Selected>();
            }
        }

        // Despawn all existing scene entities
        let entities_to_despawn: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
            query.iter(world).collect()
        };
        for entity in entities_to_despawn {
            world.despawn(entity);
        }

        // Load the prefab scene via the existing load system
        let scene_path_str = self.scene_path.to_string_lossy().to_string();
        world.write_message(LoadSceneEvent {
            path: scene_path_str,
        });

        // Update SceneFile to reflect prefab editing
        if let Some(mut scene_file) = world.get_resource_mut::<SceneFile>() {
            scene_file.path = Some(self.scene_path.to_string_lossy().to_string());
            scene_file.clear_modified();
        }

        info!("Opened prefab '{}' for editing", self.prefab_name);
    }
}

/// Command to restore the parent scene from snapshot
struct ClosePrefabCommand;

impl Command for ClosePrefabCommand {
    fn apply(self, world: &mut World) {
        let context = match world.remove_resource::<PrefabEditingContext>() {
            Some(ctx) => ctx,
            None => {
                warn!("No prefab editing context to close");
                return;
            }
        };

        // Clear selection before despawning to avoid stale outline commands
        let selected_entities: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<Selected>>();
            query.iter(world).collect()
        };
        for entity in selected_entities {
            if let Ok(mut e) = world.get_entity_mut(entity) {
                e.remove::<Selected>();
            }
        }

        // Despawn all current scene entities (the prefab's entities)
        let entities_to_despawn: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
            query.iter(world).collect()
        };
        for entity in entities_to_despawn {
            world.despawn(entity);
        }

        // Restore parent scene from snapshot
        use bevy::scene::serde::SceneDeserializer;
        use serde::de::DeserializeSeed;

        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();
        let scene_deserializer = SceneDeserializer {
            type_registry: &type_registry,
        };

        let mut ron_deserializer =
            match ron::de::Deserializer::from_str(&context.parent_scene_snapshot) {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to parse parent scene snapshot: {}", e);
                    return;
                }
            };

        let scene: DynamicScene = match scene_deserializer.deserialize(&mut ron_deserializer) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to deserialize parent scene snapshot: {:?}", e);
                return;
            }
        };

        drop(type_registry);

        let mut entity_map = bevy::ecs::entity::EntityHashMap::default();
        if let Err(e) = scene.write_to_world(world, &mut entity_map) {
            error!("Failed to restore parent scene: {:?}", e);
            return;
        }

        // Regenerate runtime components
        crate::scene::regenerate_runtime_components(world);
        crate::scene::resolve_entity_references(world);

        // Restore parent state
        if let Some(mut scene_file) = world.get_resource_mut::<SceneFile>() {
            scene_file.path = context.parent_scene_path;
            scene_file.modified = context.parent_scene_modified;
        }

        // Restore material library
        if let Some(mut library) = world.get_resource_mut::<MaterialLibrary>() {
            *library = context.parent_material_library;
        }

        // Restore camera transform
        {
            let mut query = world.query_filtered::<&mut Transform, With<EditorCamera>>();
            if let Some(mut transform) = query.iter_mut(world).next() {
                *transform = context.parent_camera_transform;
            }
        }

        // Refresh registry to pick up any changes made during editing
        if let Some(mut registry) = world.get_resource_mut::<PrefabRegistry>() {
            registry.refresh();
        }

        info!("Closed prefab editing, restored parent scene");
    }
}

/// Resource for the "unsaved changes" confirmation dialog when opening a prefab.
#[derive(Resource, Default)]
pub struct PrefabOpenConfirmDialog {
    pub open: bool,
    pub prefab_name: String,
    /// If true, auto-open the prefab after the next save completes.
    pub open_after_save: bool,
}

pub fn handle_open_prefab(
    mut events: MessageReader<OpenPrefabEvent>,
    mut commands: Commands,
    registry: Res<PrefabRegistry>,
    existing_context: Option<Res<PrefabEditingContext>>,
    scene_file: Res<SceneFile>,
    mut confirm_dialog: ResMut<PrefabOpenConfirmDialog>,
) {
    for event in events.read() {
        if existing_context.is_some() {
            warn!("Already editing a prefab — close it first");
            continue;
        }

        let Some(entry) = registry.get(&event.prefab_name) else {
            warn!("Prefab not found: {}", event.prefab_name);
            continue;
        };

        // If scene has unsaved changes, show confirmation dialog
        if scene_file.modified {
            confirm_dialog.open = true;
            confirm_dialog.prefab_name = event.prefab_name.clone();
            confirm_dialog.open_after_save = false;
            continue;
        }

        commands.queue(OpenPrefabCommand {
            prefab_name: event.prefab_name.clone(),
            prefab_dir: entry.directory.clone(),
            scene_path: entry.scene_path.clone(),
        });
    }
}

/// After a save completes, check if we should auto-open a prefab.
pub fn check_open_after_save(
    scene_file: Res<SceneFile>,
    mut confirm_dialog: ResMut<PrefabOpenConfirmDialog>,
    mut open_events: MessageWriter<OpenPrefabEvent>,
) {
    if confirm_dialog.open_after_save && !scene_file.modified {
        confirm_dialog.open_after_save = false;
        let name = std::mem::take(&mut confirm_dialog.prefab_name);
        open_events.write(OpenPrefabEvent { prefab_name: name });
    }
}

/// Draw the Save/Discard/Cancel dialog when opening a prefab with unsaved scene changes.
pub fn draw_prefab_open_confirm_dialog(world: &mut World) {
    let dialog_open = world
        .get_resource::<PrefabOpenConfirmDialog>()
        .is_some_and(|d| d.open);
    if !dialog_open {
        return;
    }

    let ui_enabled = world
        .get_resource::<EditorState>()
        .is_some_and(|s| s.ui_enabled);
    if !ui_enabled {
        return;
    }

    let ctx = {
        let Some(mut egui_ctx) = world
            .query::<&mut bevy_egui::EguiContext>()
            .iter_mut(world)
            .next()
        else {
            return;
        };
        egui_ctx.get_mut().clone()
    };

    enum Action {
        None,
        Save,
        Discard,
        Cancel,
    }

    let mut action = Action::None;

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = Action::Cancel;
    }

    let prefab_name = world
        .resource::<PrefabOpenConfirmDialog>()
        .prefab_name
        .clone();

    egui::Window::new("Unsaved Changes")
        .collapsible(false)
        .resizable(false)
        .frame(window_frame(&ctx.style()))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([360.0, 120.0])
        .show(&ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Scene has unsaved changes.\nSave before editing prefab \"{}\"?",
                        prefab_name
                    ))
                    .color(colors::TEXT_PRIMARY),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(60.0);
                    if ui.button("Save").clicked() {
                        action = Action::Save;
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("Discard").color(colors::STATUS_WARNING))
                        .clicked()
                    {
                        action = Action::Discard;
                    }
                    ui.add_space(8.0);
                    if ui.button("Cancel").clicked() {
                        action = Action::Cancel;
                    }
                });
            });
        });

    match action {
        Action::Save => {
            let mut dialog = world.resource_mut::<PrefabOpenConfirmDialog>();
            dialog.open = false;
            dialog.open_after_save = true;
            let name = dialog.prefab_name.clone();

            let path = world.resource::<SceneFile>().path.clone();
            if let Some(path) = path {
                world.write_message(SaveSceneEvent { path });
            } else {
                // No save path — open anyway (can't save untitled without a path)
                world.write_message(OpenPrefabEvent { prefab_name: name });
                world.resource_mut::<PrefabOpenConfirmDialog>().open_after_save = false;
            }
        }
        Action::Discard => {
            let mut dialog = world.resource_mut::<PrefabOpenConfirmDialog>();
            dialog.open = false;
            dialog.open_after_save = false;
            let name = std::mem::take(&mut dialog.prefab_name);

            // Clear modified flag so the re-sent event proceeds
            if let Some(mut sf) = world.get_resource_mut::<SceneFile>() {
                sf.clear_modified();
            }
            world.write_message(OpenPrefabEvent { prefab_name: name });
        }
        Action::Cancel => {
            let mut dialog = world.resource_mut::<PrefabOpenConfirmDialog>();
            dialog.open = false;
            dialog.open_after_save = false;
            dialog.prefab_name.clear();
        }
        Action::None => {}
    }
}

pub fn handle_close_prefab(
    mut events: MessageReader<ClosePrefabEvent>,
    mut commands: Commands,
    context: Option<Res<PrefabEditingContext>>,
) {
    for _event in events.read() {
        if context.is_none() {
            warn!("Not currently editing a prefab");
            continue;
        }

        commands.queue(ClosePrefabCommand);
    }
}
