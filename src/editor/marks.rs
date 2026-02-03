use bevy::prelude::*;
use bevy_egui::EguiContexts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{EditorCamera, EditorState, FlyCamera};
use crate::utils::should_process_input;

/// Orthographic scale when switching to ortho mode for axis views
const ORTHO_SCALE: f32 = 0.005;
/// Distance from origin for orthographic axis views
const ORTHO_DISTANCE: f32 = 1000.0;

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
        use std::f32::consts::{FRAC_PI_2, PI};

        let mut marks = HashMap::new();

        // 1 = X axis (Right view, looking from +X toward origin)
        marks.insert(
            "1".to_string(),
            CameraMark {
                name: "1".to_string(),
                position: Vec3::new(ORTHO_DISTANCE, 0.0, 0.0),
                yaw: -FRAC_PI_2,
                pitch: 0.0,
            },
        );

        // -1 = -X axis (Left view, looking from -X toward origin)
        marks.insert(
            "-1".to_string(),
            CameraMark {
                name: "-1".to_string(),
                position: Vec3::new(-ORTHO_DISTANCE, 0.0, 0.0),
                yaw: FRAC_PI_2,
                pitch: 0.0,
            },
        );

        // 2 = Y axis (Top view, looking from +Y toward origin)
        marks.insert(
            "2".to_string(),
            CameraMark {
                name: "2".to_string(),
                position: Vec3::new(0.0, ORTHO_DISTANCE, 0.0),
                yaw: 0.0,
                pitch: -FRAC_PI_2 + 0.001,
            },
        );

        // -2 = -Y axis (Bottom view, looking from -Y toward origin)
        marks.insert(
            "-2".to_string(),
            CameraMark {
                name: "-2".to_string(),
                position: Vec3::new(0.0, -ORTHO_DISTANCE, 0.0),
                yaw: 0.0,
                pitch: FRAC_PI_2 - 0.001,
            },
        );

        // 3 = Z axis (Front view, looking from +Z toward origin)
        marks.insert(
            "3".to_string(),
            CameraMark {
                name: "3".to_string(),
                position: Vec3::new(0.0, 0.0, ORTHO_DISTANCE),
                yaw: 0.0,
                pitch: 0.0,
            },
        );

        // -3 = -Z axis (Back view, looking from -Z toward origin)
        marks.insert(
            "-3".to_string(),
            CameraMark {
                name: "-3".to_string(),
                position: Vec3::new(0.0, 0.0, -ORTHO_DISTANCE),
                yaw: PI,
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
            .add_message::<JumpToMarkOrthoEvent>()
            .add_message::<JumpToLastPositionEvent>()
            .add_systems(
                Update,
                (
                    handle_set_mark,
                    handle_jump_to_mark,
                    handle_jump_to_mark_ortho,
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

/// Message to jump to mark and optionally switch to orthographic
#[derive(Message)]
pub struct JumpToMarkOrthoEvent {
    pub name: String,
}

/// Handle keyboard shortcuts for marks (in View mode)
/// Backtick (`) to jump to last position
/// 1/2/3 = orthographic axis views (X/Y/Z), Shift+1/2/3 = inverted axis views (-X/-Y/-Z)
/// 4-9 = jump to marks, Shift+4-9 = set marks
fn handle_mark_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<super::EditorMode>>,
    editor_state: Res<EditorState>,
    mut set_events: MessageWriter<SetCameraMarkEvent>,
    mut jump_events: MessageWriter<JumpToMarkEvent>,
    mut ortho_events: MessageWriter<JumpToMarkOrthoEvent>,
    mut last_events: MessageWriter<JumpToLastPositionEvent>,
    mut contexts: EguiContexts,
) {
    // Only handle in View mode
    if *mode.get() != super::EditorMode::View {
        return;
    }

    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    // Backtick to jump to last position
    if keyboard.just_pressed(KeyCode::Backquote) {
        last_events.write(JumpToLastPositionEvent);
        return;
    }

    // 1/2/3 are special: orthographic axis views
    // Without shift: positive axis (+X, +Y, +Z)
    // With shift: negative axis (-X, -Y, -Z)
    let axis_keys = [
        (KeyCode::Digit1, "1", "-1"),
        (KeyCode::Digit2, "2", "-2"),
        (KeyCode::Digit3, "3", "-3"),
    ];

    for (key, pos_name, neg_name) in axis_keys {
        if keyboard.just_pressed(key) {
            let name = if shift { neg_name } else { pos_name };
            ortho_events.write(JumpToMarkOrthoEvent {
                name: name.to_string(),
            });
            return;
        }
    }

    // 4-9: regular marks (jump without shift, set with shift)
    let number_keys = [
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

/// Handle jumping to mark and switching to orthographic
fn handle_jump_to_mark_ortho(
    mut events: MessageReader<JumpToMarkOrthoEvent>,
    mut marks: ResMut<CameraMarks>,
    mut camera_query: Query<(&mut Transform, &mut FlyCamera, &mut Projection), With<EditorCamera>>,
) {
    for event in events.read() {
        // Store current position before jumping
        if let Ok((transform, fly_cam, _)) = camera_query.single() {
            marks.store_last_position(transform.translation, fly_cam.yaw, fly_cam.pitch);
        }

        if let Some(mark) = marks.get_mark(&event.name).cloned() {
            if let Ok((mut transform, mut fly_cam, mut projection)) = camera_query.single_mut() {
                transform.translation = mark.position;
                fly_cam.yaw = mark.yaw;
                fly_cam.pitch = mark.pitch;
                transform.rotation =
                    Quat::from_euler(EulerRot::YXZ, fly_cam.yaw, fly_cam.pitch, 0.0);

                // Switch to orthographic
                fly_cam.fov_degrees = 0.0;
                fly_cam.ortho_scale = ORTHO_SCALE;
                *projection = Projection::Orthographic(OrthographicProjection {
                    scale: ORTHO_SCALE,
                    ..OrthographicProjection::default_3d()
                });

                info!("Jumped to orthographic view: {}", event.name);
            }
        } else {
            warn!("Mark not found: {}", event.name);
        }
    }
}
