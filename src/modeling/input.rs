//! Keyboard and mouse input handling for the mesh modeling tool.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;

use crate::commands::TakeSnapshotCommand;
use crate::editor::{EditorCamera, EditorMode, EditorState};
use crate::scene::SceneEntity;
use crate::selection::Selected;
use crate::utils::should_process_input;

use super::cut::cut_faces;
use super::edit_mesh::EditMesh;
use super::extrude::extrude_faces;
use super::marker::EditMeshMarker;
use super::selection::{
    freeform_select, pick_face, surface_group_select, uv_grid_select, world_grid_select,
    world_to_local_ray,
};
use super::{GridType, MeshModelState, ModelOperation};

/// Handle keyboard input in Model mode.
pub fn handle_model_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut model_state: ResMut<MeshModelState>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Escape: cancel operation or exit mode
    if keyboard.just_pressed(KeyCode::Escape) {
        if model_state.drawing_freeform {
            model_state.drawing_freeform = false;
            model_state.freeform_points.clear();
            return;
        }
        if model_state.pending_operation != ModelOperation::Select {
            model_state.pending_operation = ModelOperation::Select;
            model_state.extrude_distance = 0.0;
            model_state.extrude_drag_origin = None;
            model_state.extrude_drag_normal = None;
            return;
        }
        next_mode.set(EditorMode::View);
        return;
    }

    // Grid type switching
    if keyboard.just_pressed(KeyCode::Digit1) {
        model_state.grid_type = GridType::WorldSpace;
        info!("Grid: World Space");
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        model_state.grid_type = GridType::SurfaceSpace;
        info!("Grid: Surface Space");
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        model_state.grid_type = GridType::UVSpace;
        info!("Grid: UV Space");
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        model_state.grid_type = GridType::Freeform;
        model_state.drawing_freeform = false;
        model_state.freeform_points.clear();
        info!("Grid: Freeform");
    }

    // X-ray toggle
    if keyboard.just_pressed(KeyCode::KeyX) {
        model_state.xray_selection = !model_state.xray_selection;
        info!(
            "X-ray selection: {}",
            if model_state.xray_selection { "ON" } else { "OFF" }
        );
    }

    // Operation switching
    if keyboard.just_pressed(KeyCode::KeyQ) {
        model_state.pending_operation = ModelOperation::Extrude;
        model_state.extrude_distance = 0.0;
        info!("Operation: Extrude");
    } else if keyboard.just_pressed(KeyCode::KeyW) {
        model_state.pending_operation = ModelOperation::Cut;
        info!("Operation: Cut");
    }

    // Select all (A)
    if keyboard.just_pressed(KeyCode::KeyA) {
        let face_count = model_state.edit_mesh.as_ref().map(|m| m.face_count());
        if let Some(count) = face_count {
            let all: std::collections::HashSet<_> = (0..count).collect();
            model_state.selected_faces = all;
            info!("Selected all {} faces", count);
        }
    }

    // Invert selection (Tab)
    if keyboard.just_pressed(KeyCode::Tab) {
        let face_count = model_state.edit_mesh.as_ref().map(|m| m.face_count());
        if let Some(count) = face_count {
            let current = model_state.selected_faces.clone();
            let inverted: std::collections::HashSet<_> = (0..count)
                .filter(|fi| !current.contains(fi))
                .collect();
            model_state.selected_faces = inverted;
            info!("Inverted selection");
        }
    }

    // Grid size adjustment (+/-)
    if keyboard.just_pressed(KeyCode::Equal) || keyboard.just_pressed(KeyCode::NumpadAdd) {
        match model_state.grid_type {
            GridType::WorldSpace => {
                model_state.world_grid_size = (model_state.world_grid_size * 2.0).min(8.0);
                info!("World grid size: {}", model_state.world_grid_size);
            }
            GridType::UVSpace => {
                model_state.uv_grid_size = (model_state.uv_grid_size * 2.0).min(1.0);
                info!("UV grid size: {}", model_state.uv_grid_size);
            }
            _ => {}
        }
    }
    if keyboard.just_pressed(KeyCode::Minus) || keyboard.just_pressed(KeyCode::NumpadSubtract) {
        match model_state.grid_type {
            GridType::WorldSpace => {
                model_state.world_grid_size = (model_state.world_grid_size * 0.5).max(0.125);
                info!("World grid size: {}", model_state.world_grid_size);
            }
            GridType::UVSpace => {
                model_state.uv_grid_size = (model_state.uv_grid_size * 0.5).max(0.01);
                info!("UV grid size: {}", model_state.uv_grid_size);
            }
            _ => {}
        }
    }
}

/// Handle mouse click for face selection and freeform point placement.
pub fn handle_model_click(
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut model_state: ResMut<MeshModelState>,
    selected_query: Query<(&GlobalTransform, &Mesh3d), With<Selected>>,
    meshes: Res<Assets<Mesh>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    let Some(target) = model_state.target_entity else {
        return;
    };

    let Ok((entity_transform, mesh_handle)) = selected_query.get(target) else {
        return;
    };

    // Ensure we have an EditMesh loaded
    let edit_mesh = if let Some(ref m) = model_state.edit_mesh {
        m.clone()
    } else {
        // Try to create from the entity's current mesh
        let Some(mesh) = meshes.get(&mesh_handle.0) else {
            return;
        };
        let Some(em) = EditMesh::from_bevy_mesh(mesh) else {
            return;
        };
        model_state.edit_mesh = Some(em.clone());
        em
    };

    // Transform ray to local space
    let (local_origin, local_dir) = world_to_local_ray(entity_transform, ray.origin, *ray.direction);

    let Some(hit) = pick_face(&edit_mesh, local_origin, local_dir, model_state.xray_selection) else {
        return;
    };

    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    // Freeform mode: place polygon points
    if model_state.grid_type == GridType::Freeform {
        let world_point = entity_transform.transform_point(hit.point);
        if model_state.drawing_freeform {
            // Check if clicking near first point to close
            if model_state.freeform_points.len() >= 3 {
                let first = model_state.freeform_points[0];
                if world_point.distance(first) < 0.3 {
                    // Close polygon and select faces
                    let screen_polygon: Vec<Vec2> = model_state
                        .freeform_points
                        .iter()
                        .filter_map(|&p| camera.world_to_viewport(camera_transform, p).ok())
                        .collect();
                    let selected = freeform_select(
                        &edit_mesh,
                        entity_transform,
                        &screen_polygon,
                        camera,
                        camera_transform,
                    );
                    if shift {
                        model_state.selected_faces.extend(selected);
                    } else {
                        model_state.selected_faces = selected;
                    }
                    model_state.drawing_freeform = false;
                    model_state.freeform_points.clear();
                    info!("Freeform selection: {} faces", model_state.selected_faces.len());
                    return;
                }
            }
            model_state.freeform_points.push(world_point);
        } else {
            model_state.drawing_freeform = true;
            model_state.freeform_points.clear();
            model_state.freeform_points.push(world_point);
        }
        return;
    }

    // Grid-based selection
    let new_selection = match model_state.grid_type {
        GridType::WorldSpace => {
            let world_point = entity_transform.transform_point(hit.point);
            world_grid_select(&edit_mesh, entity_transform, model_state.world_grid_size, world_point)
        }
        GridType::SurfaceSpace => {
            surface_group_select(&edit_mesh, hit.face, model_state.surface_angle_threshold)
        }
        GridType::UVSpace => {
            uv_grid_select(&edit_mesh, hit.face, model_state.uv_grid_size)
        }
        GridType::Freeform => unreachable!(),
    };

    if shift {
        model_state.selected_faces.extend(new_selection);
    } else {
        model_state.selected_faces = new_selection;
    }

    info!("Selected {} faces", model_state.selected_faces.len());
}

/// Handle Enter to confirm operations (extrude/cut) or close freeform polygon.
pub fn handle_model_confirm(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut model_state: ResMut<MeshModelState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mesh_query: Query<(&Mesh3d, Option<&Name>), With<Selected>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }

    let Some(target) = model_state.target_entity else {
        return;
    };

    let Some(ref edit_mesh) = model_state.edit_mesh.clone() else {
        return;
    };

    if model_state.selected_faces.is_empty() {
        return;
    }

    match model_state.pending_operation {
        ModelOperation::Extrude => {
            if model_state.extrude_distance.abs() < 1e-6 {
                return;
            }

            // Take snapshot for undo
            commands.queue(TakeSnapshotCommand {
                description: "Extrude faces".to_string(),
            });

            let new_mesh = extrude_faces(
                edit_mesh,
                &model_state.selected_faces,
                model_state.extrude_distance,
                model_state.extrude_angle,
            );

            // Update the marker and mesh
            let marker = EditMeshMarker::from_edit_mesh(&new_mesh);
            let bevy_mesh = new_mesh.to_bevy_mesh();
            let collider = marker.to_collider();
            let mesh_handle = meshes.add(bevy_mesh);

            if let Ok(mut entity_commands) = commands.get_entity(target) {
                entity_commands.insert((
                    EditMeshMarker::from_edit_mesh(&new_mesh),
                    Mesh3d(mesh_handle),
                    collider,
                ));
            }

            model_state.edit_mesh = Some(new_mesh);
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            model_state.extrude_distance = 0.0;
            model_state.extrude_drag_origin = None;
            model_state.extrude_drag_normal = None;
            info!("Extrusion applied");
        }
        ModelOperation::Cut => {
            // Take snapshot for undo
            commands.queue(TakeSnapshotCommand {
                description: "Cut mesh".to_string(),
            });

            let (remaining, cut_out) = cut_faces(edit_mesh, &model_state.selected_faces);

            if cut_out.triangles.is_empty() || remaining.triangles.is_empty() {
                info!("Cut produced empty geometry, skipping");
                return;
            }

            // Update original entity with remaining mesh
            let remaining_marker = EditMeshMarker::from_edit_mesh(&remaining);
            let remaining_bevy = remaining.to_bevy_mesh();
            let remaining_collider = remaining_marker.to_collider();
            let remaining_handle = meshes.add(remaining_bevy);

            // Get the material and transform info before spawning
            let entity_name = mesh_query
                .get(target)
                .ok()
                .and_then(|(_, name)| name.map(|n| n.as_str().to_string()))
                .unwrap_or_else(|| "Mesh".to_string());

            if let Ok(mut entity_commands) = commands.get_entity(target) {
                entity_commands.insert((
                    EditMeshMarker::from_edit_mesh(&remaining),
                    Mesh3d(remaining_handle),
                    remaining_collider,
                ));
            }

            // Spawn new entity with the cut-out portion
            let cut_marker = EditMeshMarker::from_edit_mesh(&cut_out);
            let cut_bevy = cut_out.to_bevy_mesh();
            let cut_collider = cut_marker.to_collider();
            let cut_handle = meshes.add(cut_bevy);

            commands.spawn((
                SceneEntity,
                Name::new(format!("{} (cut)", entity_name)),
                EditMeshMarker::from_edit_mesh(&cut_out),
                Mesh3d(cut_handle),
                cut_collider,
                RigidBody::Static,
            ));

            model_state.edit_mesh = Some(remaining);
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            info!("Cut applied — new entity spawned");
        }
        ModelOperation::Select => {}
    }
}

/// Handle mouse drag for extrude distance adjustment.
///
/// Projects the mouse cursor onto the extrude normal axis in world space,
/// so dragging up/down along the normal intuitively controls distance.
pub fn handle_extrude_drag(
    mouse_button: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    selected_query: Query<&GlobalTransform, With<Selected>>,
    mut model_state: ResMut<MeshModelState>,
) {
    if model_state.pending_operation != ModelOperation::Extrude {
        return;
    }
    if model_state.selected_faces.is_empty() {
        return;
    }

    if !mouse_button.pressed(MouseButton::Left) {
        // Clear drag state on release
        model_state.extrude_drag_origin = None;
        model_state.extrude_drag_normal = None;
        return;
    }

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };
    let Some(target) = model_state.target_entity else {
        return;
    };
    let Ok(entity_transform) = selected_query.get(target) else {
        return;
    };

    // Compute extrude normal and origin on first drag frame
    if model_state.extrude_drag_origin.is_none() {
        let Some(ref edit_mesh) = model_state.edit_mesh else {
            return;
        };

        // Area-weighted average normal of selected faces (in local space)
        let mut normal_sum = Vec3::ZERO;
        let mut centroid_sum = Vec3::ZERO;
        let mut count = 0;
        for &fi in &model_state.selected_faces {
            if fi < edit_mesh.triangles.len() {
                normal_sum += edit_mesh.face_normal(fi) * edit_mesh.face_area(fi);
                centroid_sum += edit_mesh.face_center(fi);
                count += 1;
            }
        }
        if count == 0 {
            return;
        }

        let local_normal = normal_sum.normalize_or_zero();
        let local_centroid = centroid_sum / count as f32;

        // Transform to world space
        let world_origin = entity_transform.transform_point(local_centroid);
        let world_normal_tip = entity_transform.transform_point(local_centroid + local_normal);
        let world_normal = (world_normal_tip - world_origin).normalize_or_zero();

        model_state.extrude_drag_origin = Some(world_origin);
        model_state.extrude_drag_normal = Some(world_normal);
        model_state.extrude_drag_baseline = model_state.extrude_distance;
        return;
    }

    let origin = model_state.extrude_drag_origin.unwrap();
    let normal = model_state.extrude_drag_normal.unwrap();

    // Project cursor ray onto the extrude normal line
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        return;
    };

    // Find closest point on the extrude line to the camera ray
    // Line 1: P = origin + t * normal
    // Line 2: Q = ray.origin + s * ray.direction
    // Minimize |P - Q|^2
    let d1 = normal;
    let d2 = *ray.direction;
    let r = ray.origin - origin;

    let a = d1.dot(d1);
    let b = d1.dot(d2);
    let c = d2.dot(d2);
    let d = d1.dot(r);
    let _e = d2.dot(r);

    let denom = a * c - b * b;
    if denom.abs() < 1e-8 {
        return; // Lines are parallel
    }

    let t = (b * _e - c * d) / denom;

    model_state.extrude_distance = model_state.extrude_drag_baseline + t;
}
