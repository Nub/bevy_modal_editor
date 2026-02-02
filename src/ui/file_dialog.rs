use std::path::PathBuf;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use egui_file_dialog::FileDialog;

use crate::editor::EditorState;
use crate::scene::{LoadSceneEvent, SaveSceneEvent};

/// The type of file operation being performed
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileDialogOperation {
    LoadScene,
    SaveScene,
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
            }
        }
    }

    Ok(())
}
