//! Find object palette — search scene objects by name.
//!
//! While navigating the list, the highlighted entity is temporarily selected
//! and the camera looks at it. Closing the palette restores the original
//! selection and camera position; confirming keeps the new state.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_egui::egui;

use bevy_editor_game::GameEntity;

use crate::editor::{EditorCamera, FlyCamera};
use crate::scene::SceneEntity;
use crate::selection::Selected;
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::theme::{colors, window_frame};

use super::CommandPaletteState;

/// Saved camera state for restoring after palette close.
#[derive(Clone)]
pub(crate) struct FlyCameraSnapshot {
    pub fly: FlyCamera,
}

/// Entry for a scene object that implements PaletteItem
struct ObjectEntry {
    entity: Entity,
    name: String,
}

impl PaletteItem for ObjectEntry {
    fn label(&self) -> &str {
        &self.name
    }
}

/// Minimum distance from target when framing objects
const MIN_FRAME_DISTANCE: f32 = 5.0;
/// Padding multiplier for framing
const FRAME_PADDING: f32 = 1.5;

/// Draw the find object palette
pub(super) fn draw_find_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    commands: &mut Commands,
    scene_objects: &Query<(Entity, &Name), Or<(With<SceneEntity>, With<GameEntity>)>>,
    selected_entities: &Query<Entity, With<Selected>>,
    transforms: &Query<(&Transform, Option<&Collider>), Without<EditorCamera>>,
    camera_query: &mut Query<
        (&mut FlyCamera, &mut Transform, &Projection),
        (With<EditorCamera>, Without<Selected>),
    >,
) -> Result {
    // Build list of scene objects, sorted by name for stable ordering across frames
    // (Bevy query iteration order is non-deterministic)
    let mut objects: Vec<ObjectEntry> = scene_objects
        .iter()
        .map(|(entity, name)| ObjectEntry {
            entity,
            name: name.as_str().to_string(),
        })
        .collect();
    objects.sort_by(|a, b| a.name.cmp(&b.name).then(a.entity.cmp(&b.entity)));

    // Handle empty scene
    if objects.is_empty() {
        egui::Window::new("Find Object")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(window_frame(&ctx.style()))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([400.0, 100.0])
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No objects in scene")
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                });
                ui.add_space(20.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Esc")
                            .small()
                            .strong()
                            .color(colors::ACCENT_BLUE),
                    );
                    ui.label(
                        egui::RichText::new("to close")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.open = false;
        }
        return Ok(());
    }

    // On first frame, capture the original selection and camera state
    if state.just_opened {
        state.find_original_selection = selected_entities.iter().collect();
        state.find_prev_highlighted = None;

        if let Ok((ref fly_cam, ref cam_transform, _)) = camera_query.single_mut() {
            state.find_original_camera = Some((
                **cam_transform,
                FlyCameraSnapshot {
                    fly: (**fly_cam).clone(),
                },
            ));
        }
    }

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState::from_bridge(
        std::mem::take(&mut state.query),
        state.selected_index,
        state.just_opened,
    );

    let config = PaletteConfig {
        title: "FIND OBJECT",
        title_color: colors::ACCENT_CYAN,
        subtitle: "Search scene objects",
        hint_text: "Type to search...",
        action_label: "select",
        size: [400.0, 200.0],
        show_categories: false,
        preview_panel: None,
        anchor: egui::Align2::CENTER_BOTTOM,
        anchor_offset: [0.0, -32.0],
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &objects, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    // Determine the currently highlighted entity
    let filtered = fuzzy_filter(&objects, &state.query);
    let highlighted_entity = if filtered.is_empty() {
        None
    } else {
        let idx = state.selected_index.min(filtered.len() - 1);
        filtered.get(idx).map(|fi| fi.item.entity)
    };

    // If highlighted entity changed, temporarily select it and look at it
    if highlighted_entity != state.find_prev_highlighted {
        state.find_prev_highlighted = highlighted_entity;

        // Deselect all
        for entity in selected_entities.iter() {
            commands.entity(entity).remove::<Selected>();
        }

        if let Some(entity) = highlighted_entity {
            // Select the highlighted entity
            commands.entity(entity).insert(Selected);

            // Look at it
            if let Ok((transform, collider)) = transforms.get(entity) {
                look_at_entity(camera_query, transform, collider);
            }
        }
    }

    match result {
        PaletteResult::Selected(index) => {
            // Confirm selection — already selected via highlight, just close
            // Make sure the correct entity is selected (highlight may differ)
            if let Some(obj) = objects.get(index) {
                if highlighted_entity != Some(obj.entity) {
                    for entity in selected_entities.iter() {
                        commands.entity(entity).remove::<Selected>();
                    }
                    commands.entity(obj.entity).insert(Selected);
                    if let Ok((transform, collider)) = transforms.get(obj.entity) {
                        look_at_entity(camera_query, transform, collider);
                    }
                }
            }
            state.open = false;
            state.find_original_selection.clear();
            state.find_original_camera = None;
            state.find_prev_highlighted = None;
        }
        PaletteResult::Closed => {
            // Restore original selection
            for entity in selected_entities.iter() {
                commands.entity(entity).remove::<Selected>();
            }
            for entity in state.find_original_selection.drain(..) {
                commands.entity(entity).try_insert(Selected);
            }

            // Restore original camera position
            if let Some((orig_transform, snapshot)) = state.find_original_camera.take() {
                if let Ok((mut fly_cam, mut cam_transform, _)) = camera_query.single_mut() {
                    *cam_transform = orig_transform;
                    *fly_cam = snapshot.fly;
                }
            }

            state.find_prev_highlighted = None;
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Move the camera to look at a specific entity (replicates look_at_selected logic).
fn look_at_entity(
    camera_query: &mut Query<
        (&mut FlyCamera, &mut Transform, &Projection),
        (With<EditorCamera>, Without<Selected>),
    >,
    target_transform: &Transform,
    target_collider: Option<&Collider>,
) {
    let pos = target_transform.translation;
    let half_extents = target_collider
        .map(|c| {
            let aabb = c.aabb(pos, target_transform.rotation);
            let min: Vec3 = aabb.min.into();
            let max: Vec3 = aabb.max.into();
            (max - min) * 0.5
        })
        .unwrap_or(Vec3::splat(0.5));

    let center = pos;
    let max_extent = (half_extents * 2.0).max_element().max(1.0);

    for (mut fly_cam, mut camera_transform, projection) in camera_query.iter_mut() {
        let distance = match projection {
            Projection::Perspective(persp) => {
                let half_fov = persp.fov * 0.5;
                (max_extent * FRAME_PADDING) / half_fov.tan()
            }
            Projection::Orthographic(_) | Projection::Custom(_) => {
                max_extent * FRAME_PADDING * 2.0
            }
        };

        let distance = distance.max(MIN_FRAME_DISTANCE);
        let offset = Vec3::new(1.0, 0.7, 1.0).normalize() * distance;
        let new_pos = center + offset;

        camera_transform.translation = new_pos;
        camera_transform.look_at(center, Vec3::Y);

        let (yaw, pitch, _) = camera_transform.rotation.to_euler(EulerRot::YXZ);
        fly_cam.yaw = yaw;
        fly_cam.pitch = pitch;
    }
}
