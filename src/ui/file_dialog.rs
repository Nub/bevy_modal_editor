use std::path::PathBuf;

use bevy::prelude::*;
use bevy_egui::egui::Align2;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use egui_file_dialog::FileDialog;

use crate::editor::{EditorMode, EditorState, InsertObjectType, InsertState, StartInsertEvent};
use crate::scene::{LoadSceneEvent, SaveSceneEvent};

/// Create a centered file dialog with common settings
fn create_centered_dialog() -> FileDialog {
    FileDialog::new()
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_size([600.0, 400.0])
}

/// File extensions for scene files
fn scene_extensions() -> Vec<&'static str> {
    vec!["ron"]
}

/// File extensions for GLTF/GLB models
fn gltf_extensions() -> Vec<&'static str> {
    vec!["gltf", "glb"]
}

/// File extensions for image textures
fn image_extensions() -> Vec<&'static str> {
    vec!["png", "jpg", "jpeg", "hdr", "exr", "tga", "bmp"]
}

/// Which texture slot is being picked for
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextureSlot {
    BaseColor,
    NormalMap,
    MetallicRoughness,
    Emissive,
    Occlusion,
}

/// The type of file operation being performed
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileDialogOperation {
    LoadScene,
    SaveScene,
    /// Pick a GLTF file to insert
    InsertGltf,
    /// Pick a RON scene file to insert
    InsertScene,
    /// Pick a texture image for a material slot
    PickTexture { slot: TextureSlot, entity: Entity },
}

/// Result of a texture pick operation, consumed by the material editor
#[derive(Resource, Default)]
pub struct TexturePickResult(pub Option<TexturePickData>);

/// Data for a completed texture pick
pub struct TexturePickData {
    pub slot: TextureSlot,
    pub entity: Entity,
    pub path: String,
}

/// Resource managing the egui file dialog state
#[derive(Resource)]
pub struct FileDialogState {
    pub dialog: FileDialog,
    /// The current operation being performed, if any
    pub operation: Option<FileDialogOperation>,
}

impl Default for FileDialogState {
    fn default() -> Self {
        Self {
            dialog: FileDialog::new(),
            operation: None,
        }
    }
}

impl FileDialogState {
    /// Open the file dialog for loading a scene
    pub fn open_load_scene(&mut self, current_path: Option<&str>) {
        self.dialog = create_centered_dialog()
            .add_file_filter_extensions("Scene files", scene_extensions())
            .default_file_filter("Scene files");

        if let Some(path) = current_path {
            self.dialog = std::mem::take(&mut self.dialog).initial_directory(PathBuf::from(path));
        }

        self.dialog.pick_file();
        self.operation = Some(FileDialogOperation::LoadScene);
    }

    /// Open the file dialog for saving a scene
    pub fn open_save_scene(&mut self, current_path: Option<&str>) {
        self.dialog = create_centered_dialog()
            .add_file_filter_extensions("Scene files", scene_extensions())
            .default_file_filter("Scene files")
            .default_file_name("scene.ron");

        if let Some(path) = current_path {
            let path_buf = PathBuf::from(path);
            let file_name = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("scene.ron")
                .to_string();
            self.dialog = std::mem::take(&mut self.dialog)
                .initial_directory(path_buf)
                .default_file_name(&file_name);
        }

        self.dialog.save_file();
        self.operation = Some(FileDialogOperation::SaveScene);
    }

    /// Open the file dialog for picking a GLTF file to insert
    pub fn open_insert_gltf(&mut self) {
        self.dialog = create_centered_dialog()
            .add_file_filter_extensions("GLTF models", gltf_extensions())
            .default_file_filter("GLTF models");

        // Start in the assets directory if it exists
        let assets_dir = PathBuf::from("assets");
        if assets_dir.exists() {
            self.dialog = std::mem::take(&mut self.dialog).initial_directory(assets_dir);
        }

        self.dialog.pick_file();
        self.operation = Some(FileDialogOperation::InsertGltf);
    }

    /// Open the file dialog for picking a RON scene file to insert
    pub fn open_insert_scene(&mut self) {
        self.dialog = create_centered_dialog()
            .add_file_filter_extensions("Scene files", scene_extensions())
            .default_file_filter("Scene files");

        self.dialog.pick_file();
        self.operation = Some(FileDialogOperation::InsertScene);
    }

    /// Open the file dialog for picking a texture image
    pub fn open_pick_texture(&mut self, slot: TextureSlot, entity: Entity) {
        self.dialog = create_centered_dialog()
            .add_file_filter_extensions("Image files", image_extensions())
            .default_file_filter("Image files");

        // Start in the assets directory if it exists
        let assets_dir = PathBuf::from("assets");
        if assets_dir.exists() {
            self.dialog = std::mem::take(&mut self.dialog).initial_directory(assets_dir);
        }

        self.dialog.pick_file();
        self.operation = Some(FileDialogOperation::PickTexture { slot, entity });
    }
}

pub struct FileDialogPlugin;

impl Plugin for FileDialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FileDialogState>()
            .init_resource::<TexturePickResult>()
            .add_systems(EguiPrimaryContextPass, update_file_dialog);
    }
}

fn update_file_dialog(
    mut contexts: EguiContexts,
    mut state: ResMut<FileDialogState>,
    mut load_events: MessageWriter<LoadSceneEvent>,
    mut save_events: MessageWriter<SaveSceneEvent>,
    mut insert_events: MessageWriter<StartInsertEvent>,
    mut insert_state: ResMut<InsertState>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    editor_state: Res<EditorState>,
    mut texture_pick: ResMut<TexturePickResult>,
) -> Result {
    if !editor_state.ui_enabled {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Update the dialog
    state.dialog.update(ctx);

    // Check if a file was picked
    if let Some(path) = state.dialog.take_picked() {
        if let Some(operation) = state.operation.take() {
            match operation {
                FileDialogOperation::LoadScene => {
                    load_events.write(LoadSceneEvent {
                        path: path.to_string_lossy().to_string(),
                    });
                }
                FileDialogOperation::SaveScene => {
                    save_events.write(SaveSceneEvent {
                        path: path.to_string_lossy().to_string(),
                    });
                }
                FileDialogOperation::InsertGltf => {
                    // Convert absolute path to assets-relative path
                    let path_str = path.to_string_lossy().to_string();
                    let assets_relative = if let Some(idx) = path_str.find("/assets/") {
                        path_str[idx + 8..].to_string()
                    } else if let Some(idx) = path_str.find("assets/") {
                        path_str[idx + 7..].to_string()
                    } else {
                        // Just use the filename if we can't find assets folder
                        path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or(path_str)
                    };

                    // Set the GLTF path and trigger insert mode
                    insert_state.gltf_path = Some(assets_relative);
                    insert_events.write(StartInsertEvent {
                        object_type: InsertObjectType::Gltf,
                    });
                    next_mode.set(EditorMode::Insert);
                }
                FileDialogOperation::InsertScene => {
                    // Use the full path for scene files
                    let path_str = path.to_string_lossy().to_string();

                    // Set the scene path and trigger insert mode
                    insert_state.scene_path = Some(path_str);
                    insert_events.write(StartInsertEvent {
                        object_type: InsertObjectType::Scene,
                    });
                    next_mode.set(EditorMode::Insert);
                }
                FileDialogOperation::PickTexture { slot, entity } => {
                    // Convert absolute path to assets-relative path
                    let path_str = path.to_string_lossy().to_string();
                    let assets_relative = if let Some(idx) = path_str.find("/assets/") {
                        path_str[idx + 8..].to_string()
                    } else if let Some(idx) = path_str.find("assets/") {
                        path_str[idx + 7..].to_string()
                    } else {
                        path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or(path_str)
                    };

                    texture_pick.0 = Some(TexturePickData {
                        slot,
                        entity,
                        path: assets_relative,
                    });
                }
            }
        }
    }

    Ok(())
}
