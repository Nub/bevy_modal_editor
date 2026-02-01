use bevy::prelude::*;
use bevy_egui::EguiContexts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{EditorCamera, EditorState, FlyCamera};

/// A saved camera position and orientation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraMark {
    pub name: String,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

/// Resource storing all camera marks
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct CameraMarks {
    pub marks: HashMap<String, CameraMark>,
    /// Last camera position before a jump (for quick return)
    #[serde(skip)]
    pub last_position: Option<CameraMark>,
}

impl Default for CameraMarks {
    fn default() -> Self {
        use std::f32::consts::FRAC_PI_2;
        const DISTANCE: f32 = 10.0;

        let mut marks = HashMap::new();

        // 1 = X axis (Right view)
        marks.insert(
            "1".to_string(),
            CameraMark {
                name: "1".to_string(),
                position: Vec3::new(DISTANCE, 0.0, 0.0),
                yaw: -FRAC_PI_2,
                pitch: 0.0,
            },
        );

        // 2 = Y axis (Top view)
        marks.insert(
            "2".to_string(),
            CameraMark {
                name: "2".to_string(),
                position: Vec3::new(0.0, DISTANCE, 0.0),
                yaw: 0.0,
                pitch: -FRAC_PI_2 + 0.001,
            },
        );

        // 3 = Z axis (Front view)
        marks.insert(
            "3".to_string(),
            CameraMark {
                name: "3".to_string(),
                position: Vec3::new(0.0, 0.0, DISTANCE),
                yaw: 0.0,
                pitch: 0.0,
            },
        );

        Self {
            marks,
            last_position: None,
        }
    }
}

impl CameraMarks {
    /// Set a mark at the current camera position
    pub fn set_mark(&mut self, name: String, position: Vec3, yaw: f32, pitch: f32) {
        self.marks.insert(
            name.clone(),
            CameraMark {
                name,
                position,
                yaw,
                pitch,
            },
        );
    }

    /// Get a mark by name
    pub fn get_mark(&self, name: &str) -> Option<&CameraMark> {
        self.marks.get(name)
    }

    /// Remove a mark
    pub fn remove_mark(&mut self, name: &str) -> Option<CameraMark> {
        self.marks.remove(name)
    }

    /// Store current position as last position (for quick return)
    pub fn store_last_position(&mut self, position: Vec3, yaw: f32, pitch: f32) {
        self.last_position = Some(CameraMark {
            name: "last".to_string(),
            position,
            yaw,
            pitch,
        });
    }
}

/// Message to set a camera mark
#[derive(Message)]
pub struct SetCameraMarkEvent {
    pub name: String,
}

/// Message to jump to a camera mark
#[derive(Message)]
pub struct JumpToMarkEvent {
    pub name: String,
}

/// Message to jump to last position
#[derive(Message)]
pub struct JumpToLastPositionEvent;

pub struct CameraMarksPlugin;

impl Plugin for CameraMarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMarks>()
            .add_message::<SetCameraMarkEvent>()
            .add_message::<JumpToMarkEvent>()
            .add_message::<JumpToLastPositionEvent>()
            .add_systems(
                Update,
                (
                    handle_set_mark,
                    handle_jump_to_mark,
                    handle_jump_to_last,
                    handle_mark_shortcuts,
                ),
            );
    }
}

/// Handle setting a camera mark
fn handle_set_mark(
    mut events: MessageReader<SetCameraMarkEvent>,
    mut marks: ResMut<CameraMarks>,
    camera_query: Query<(&Transform, &FlyCamera), With<EditorCamera>>,
) {
    for event in events.read() {
        if let Ok((transform, fly_cam)) = camera_query.single() {
            marks.set_mark(
                event.name.clone(),
                transform.translation,
                fly_cam.yaw,
                fly_cam.pitch,
            );
            info!("Set camera mark: {}", event.name);
        }
    }
}

/// Handle jumping to a camera mark
fn handle_jump_to_mark(
    mut events: MessageReader<JumpToMarkEvent>,
    mut marks: ResMut<CameraMarks>,
    mut camera_query: Query<(&mut Transform, &mut FlyCamera), With<EditorCamera>>,
) {
    for event in events.read() {
        // Store current position before jumping
        if let Ok((transform, fly_cam)) = camera_query.single() {
            marks.store_last_position(transform.translation, fly_cam.yaw, fly_cam.pitch);
        }

        if let Some(mark) = marks.get_mark(&event.name).cloned() {
            if let Ok((mut transform, mut fly_cam)) = camera_query.single_mut() {
                transform.translation = mark.position;
                fly_cam.yaw = mark.yaw;
                fly_cam.pitch = mark.pitch;
                transform.rotation =
                    Quat::from_euler(EulerRot::YXZ, fly_cam.yaw, fly_cam.pitch, 0.0);
                info!("Jumped to mark: {}", event.name);
            }
        } else {
            warn!("Mark not found: {}", event.name);
        }
    }
}

/// Handle jumping to last position
fn handle_jump_to_last(
    mut events: MessageReader<JumpToLastPositionEvent>,
    mut marks: ResMut<CameraMarks>,
    mut camera_query: Query<(&mut Transform, &mut FlyCamera), With<EditorCamera>>,
) {
    for _ in events.read() {
        if let Some(last) = marks.last_position.clone() {
            // Store current as new last before jumping
            if let Ok((transform, fly_cam)) = camera_query.single() {
                let current = CameraMark {
                    name: "last".to_string(),
                    position: transform.translation,
                    yaw: fly_cam.yaw,
                    pitch: fly_cam.pitch,
                };

                if let Ok((mut transform, mut fly_cam)) = camera_query.single_mut() {
                    transform.translation = last.position;
                    fly_cam.yaw = last.yaw;
                    fly_cam.pitch = last.pitch;
                    transform.rotation =
                        Quat::from_euler(EulerRot::YXZ, fly_cam.yaw, fly_cam.pitch, 0.0);
                }

                marks.last_position = Some(current);
                info!("Jumped to last position");
            }
        }
    }
}

/// Handle keyboard shortcuts for marks (in View mode)
/// Backtick (`) to jump to last position
/// Number keys 1-9 to jump to marks "1"-"9"
/// Shift + number to set marks "1"-"9"
fn handle_mark_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<super::EditorMode>>,
    editor_state: Res<EditorState>,
    mut set_events: MessageWriter<SetCameraMarkEvent>,
    mut jump_events: MessageWriter<JumpToMarkEvent>,
    mut last_events: MessageWriter<JumpToLastPositionEvent>,
    mut contexts: EguiContexts,
) {
    // Don't handle when editor is disabled
    if !editor_state.editor_active {
        return;
    }

    // Only handle in View mode
    if *mode.get() != super::EditorMode::View {
        return;
    }

    // Don't handle shortcuts when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    // Backtick to jump to last position
    if keyboard.just_pressed(KeyCode::Backquote) {
        last_events.write(JumpToLastPositionEvent);
        return;
    }

    // Number keys for quick marks
    let number_keys = [
        (KeyCode::Digit1, "1"),
        (KeyCode::Digit2, "2"),
        (KeyCode::Digit3, "3"),
        (KeyCode::Digit4, "4"),
        (KeyCode::Digit5, "5"),
        (KeyCode::Digit6, "6"),
        (KeyCode::Digit7, "7"),
        (KeyCode::Digit8, "8"),
        (KeyCode::Digit9, "9"),
    ];

    for (key, name) in number_keys {
        if keyboard.just_pressed(key) {
            if shift {
                set_events.write(SetCameraMarkEvent {
                    name: name.to_string(),
                });
            } else {
                jump_events.write(JumpToMarkEvent {
                    name: name.to_string(),
                });
            }
            return;
        }
    }
}
