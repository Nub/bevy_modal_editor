use std::path::PathBuf;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use egui_file_dialog::FileDialog;

use crate::editor::{EditorMode, EditorState, InsertObjectType, InsertState, StartInsertEvent};
use crate::scene::{LoadSceneEvent, SaveSceneEvent};

/// The type of file operation being performed
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileDialogOperation {
    LoadScene,
    SaveScene,
    /// Pick a GLTF file to insert
    InsertGltf,
    /// Pick a RON scene file to insert
    InsertScene,
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
        // Create a new dialog configured with the current path
        if let Some(path) = current_path {
            self.dialog = FileDialog::new().initial_directory(PathBuf::from(path));
        } else {
            self.dialog = FileDialog::new();
        }
        self.dialog.pick_file();
        self.operation = Some(FileDialogOperation::LoadScene);
    }

    /// Open the file dialog for saving a scene
    pub fn open_save_scene(&mut self, current_path: Option<&str>) {
        // Create a new dialog configured with the current path and filename
        if let Some(path) = current_path {
            let path_buf = PathBuf::from(path);
            let file_name = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("scene.ron");
            self.dialog = FileDialog::new()
                .initial_directory(path_buf.clone())
                .default_file_name(file_name);
        } else {
            self.dialog = FileDialog::new().default_file_name("scene.ron");
        }
        self.dialog.save_file();
        self.operation = Some(FileDialogOperation::SaveScene);
    }

    /// Open the file dialog for picking a GLTF file to insert
    pub fn open_insert_gltf(&mut self) {
        // Start in the assets directory if it exists
        let assets_dir = PathBuf::from("assets");
        if assets_dir.exists() {
            self.dialog = FileDialog::new().initial_directory(assets_dir);
        } else {
            self.dialog = FileDialog::new();
        }
        self.dialog.pick_file();
        self.operation = Some(FileDialogOperation::InsertGltf);
    }

    /// Open the file dialog for picking a RON scene file to insert
    pub fn open_insert_scene(&mut self) {
        self.dialog = FileDialog::new();
        self.dialog.pick_file();
        self.operation = Some(FileDialogOperation::InsertScene);
    }
}

pub struct FileDialogPlugin;

impl Plugin for FileDialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FileDialogState>()
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
            }
        }
    }

    Ok(())
}
