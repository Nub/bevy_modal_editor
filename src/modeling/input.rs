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

use super::bevel::bevel_edges;
use super::boolean::boolean_op;
use super::bridge::bridge_selected_edges;
use super::cut::cut_faces;
use super::delete::{delete_faces, dissolve_edges, dissolve_vertices};
use super::edit_mesh::EditMesh;
use super::edge_loop::{insert_edge_loop, select_edge_loop};
use super::extrude::extrude_faces;
use super::fill_hole::fill_holes;
use super::half_edge::HalfEdgeMesh;
use super::inset::inset_faces;
use super::marker::EditMeshMarker;
use super::mirror::mirror_mesh;
use super::plane_cut::plane_cut;
use super::push_pull::push_pull_faces;
use super::remesh::remesh;
use super::selection::{
    expand_to_face_groups, freeform_select, pick_edge, pick_face, pick_vertex,
    surface_group_select, uv_grid_select, world_grid_select, world_to_local_ray,
};
use super::catmull_clark::catmull_clark_subdivide;
use super::normals::{auto_smooth_normals_with_hard_edges, flat_normals};
use super::select_ops;
use super::simplify::simplify_mesh;
use super::snap::snap_faces_to_grid;
use super::smooth::{smooth_mesh, subdivide_mesh};
use super::uv_project::project_uvs_faces;
use super::uv_seam::toggle_seam_he;
use super::uv_unwrap::unwrap_uvs;
use super::weld::weld_vertices;
use super::{ElementSelection, GridType, MeshModelState, ModelOperation, SelectionMode};

/// Handle keyboard input in Model mode.
pub fn handle_model_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut model_state: ResMut<MeshModelState>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Suppress keyboard shortcuts while right-click is held (camera is flying)
    if mouse_button.pressed(MouseButton::Right) {
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
            model_state.extrude_drag_screen_start = None;
            model_state.extrude_drag_screen_dir = None;
            return;
        }
        next_mode.set(EditorMode::View);
        return;
    }

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    // Selection mode switching (A/S/D) — only without Ctrl
    if !ctrl && keyboard.just_pressed(KeyCode::KeyA) {
        model_state.selection_mode = SelectionMode::Vertex;
        model_state.element_selection.clear();
        model_state.selected_faces.clear();
        info!("Selection: Vertex");
    } else if keyboard.just_pressed(KeyCode::KeyS) {
        model_state.selection_mode = SelectionMode::Edge;
        model_state.element_selection.clear();
        model_state.selected_faces.clear();
        info!("Selection: Edge");
    } else if keyboard.just_pressed(KeyCode::KeyD) {
        model_state.selection_mode = SelectionMode::Face;
        model_state.element_selection.clear();
        model_state.selected_faces.clear();
        info!("Selection: Face");
    }

    // Grid type cycling (4 key cycles through grid types when in Face mode)
    if keyboard.just_pressed(KeyCode::Digit4) {
        model_state.grid_type = match model_state.grid_type {
            GridType::WorldSpace => GridType::SurfaceSpace,
            GridType::SurfaceSpace => GridType::UVSpace,
            GridType::UVSpace => GridType::Freeform,
            GridType::Freeform => GridType::WorldSpace,
        };
        model_state.drawing_freeform = false;
        model_state.freeform_points.clear();
        info!("Grid: {}", model_state.grid_type.display_name());
    }

    // X-ray toggle
    if keyboard.just_pressed(KeyCode::KeyX) {
        model_state.xray_selection = !model_state.xray_selection;
        info!(
            "X-ray selection: {}",
            if model_state.xray_selection { "ON" } else { "OFF" }
        );
    }

    // Operation switching (Q=cut, E=extrude, W=bevel, R=inset, P=push/pull, F=bridge, G=weld, L=edge loop)
    if keyboard.just_pressed(KeyCode::KeyE) {
        model_state.pending_operation = ModelOperation::Extrude;
        model_state.extrude_distance = 0.0;
        info!("Operation: Extrude");
    } else if keyboard.just_pressed(KeyCode::KeyQ) {
        model_state.pending_operation = ModelOperation::Cut;
        info!("Operation: Cut");
    } else if keyboard.just_pressed(KeyCode::KeyR) {
        model_state.pending_operation = ModelOperation::Inset;
        info!("Operation: Inset");
    } else if keyboard.just_pressed(KeyCode::KeyW) {
        model_state.pending_operation = ModelOperation::Bevel;
        info!("Operation: Bevel");
    } else if keyboard.just_pressed(KeyCode::KeyP) {
        model_state.pending_operation = ModelOperation::PushPull;
        model_state.push_pull_distance = 0.0;
        info!("Operation: Push/Pull");
    } else if keyboard.just_pressed(KeyCode::KeyF) {
        model_state.pending_operation = ModelOperation::Bridge;
        info!("Operation: Bridge");
    } else if keyboard.just_pressed(KeyCode::KeyG) {
        model_state.pending_operation = ModelOperation::Weld;
        info!("Operation: Weld");
    } else if keyboard.just_pressed(KeyCode::KeyL) {
        model_state.pending_operation = ModelOperation::EdgeLoop;
        info!("Operation: Edge Loop");
    } else if keyboard.just_pressed(KeyCode::KeyU) {
        model_state.pending_operation = ModelOperation::UvProject;
        info!("Operation: UV Project");
    }

    // T key: toggle seam on selected edge (Edge mode only)
    if keyboard.just_pressed(KeyCode::KeyT) && model_state.selection_mode == SelectionMode::Edge {
        let edges_to_toggle: Vec<u32> = match &model_state.element_selection {
            ElementSelection::Edges(e) => e.iter().copied().collect(),
            _ => Vec::new(),
        };
        if !edges_to_toggle.is_empty() {
            if let Some(ref he_mesh) = model_state.half_edge_mesh.clone() {
                for he_id in &edges_to_toggle {
                    toggle_seam_he(&mut model_state.uv_seams, he_mesh, *he_id);
                }
                info!("Toggled {} seam edges", edges_to_toggle.len());
            }
        }
    }

    // V key: toggle UV editor panel
    if keyboard.just_pressed(KeyCode::KeyV) {
        model_state.show_uv_editor = !model_state.show_uv_editor;
        info!(
            "UV Editor: {}",
            if model_state.show_uv_editor { "ON" } else { "OFF" }
        );
    }

    // Delete key: immediate deletion of selected elements
    if keyboard.just_pressed(KeyCode::Delete) || keyboard.just_pressed(KeyCode::Backspace) {
        model_state.delete_requested = true;
    }

    // Hard edge toggle (H key in Edge mode)
    if keyboard.just_pressed(KeyCode::KeyH) && model_state.selection_mode == SelectionMode::Edge {
        let edges_to_toggle: Vec<u32> = match &model_state.element_selection {
            ElementSelection::Edges(e) => e.iter().copied().collect(),
            _ => Vec::new(),
        };
        if !edges_to_toggle.is_empty() {
            if let Some(ref he_mesh) = model_state.half_edge_mesh.clone() {
                for &he_id in &edges_to_toggle {
                    let (from, to) = he_mesh.edge_vertices(he_id);
                    super::normals::toggle_hard_edge(&mut model_state.hard_edges, from, to);
                }
                info!("Toggled {} hard edges", edges_to_toggle.len());
            }
        }
    }

    // Selection grow/shrink (Ctrl+= / Ctrl+-)
    if ctrl && keyboard.just_pressed(KeyCode::Equal) {
        match model_state.selection_mode {
            SelectionMode::Face => {
                if let Some(ref edit_mesh) = model_state.edit_mesh {
                    model_state.selected_faces =
                        select_ops::grow_face_selection(edit_mesh, &model_state.selected_faces);
                    info!("Grew selection to {} faces", model_state.selected_faces.len());
                }
            }
            SelectionMode::Vertex => {
                if let Some(ref he_mesh) = model_state.half_edge_mesh {
                    if let ElementSelection::Vertices(ref verts) = model_state.element_selection {
                        let grown = select_ops::grow_vertex_selection(he_mesh, verts);
                        info!("Grew selection to {} vertices", grown.len());
                        model_state.element_selection = ElementSelection::Vertices(grown);
                    }
                }
            }
            SelectionMode::Edge => {
                if let Some(ref he_mesh) = model_state.half_edge_mesh {
                    if let ElementSelection::Edges(ref edges) = model_state.element_selection {
                        let grown = select_ops::grow_edge_selection(he_mesh, edges);
                        info!("Grew selection to {} edges", grown.len());
                        model_state.element_selection = ElementSelection::Edges(grown);
                    }
                }
            }
        }
        return;
    }
    if ctrl && keyboard.just_pressed(KeyCode::Minus) {
        match model_state.selection_mode {
            SelectionMode::Face => {
                if let Some(ref edit_mesh) = model_state.edit_mesh {
                    model_state.selected_faces =
                        select_ops::shrink_face_selection(edit_mesh, &model_state.selected_faces);
                    info!("Shrunk selection to {} faces", model_state.selected_faces.len());
                }
            }
            SelectionMode::Vertex => {
                if let Some(ref he_mesh) = model_state.half_edge_mesh {
                    if let ElementSelection::Vertices(ref verts) = model_state.element_selection {
                        let shrunk = select_ops::shrink_vertex_selection(he_mesh, verts);
                        info!("Shrunk selection to {} vertices", shrunk.len());
                        model_state.element_selection = ElementSelection::Vertices(shrunk);
                    }
                }
            }
            SelectionMode::Edge => {
                if let Some(ref he_mesh) = model_state.half_edge_mesh {
                    if let ElementSelection::Edges(ref edges) = model_state.element_selection {
                        let shrunk = select_ops::shrink_edge_selection(he_mesh, edges);
                        info!("Shrunk selection to {} edges", shrunk.len());
                        model_state.element_selection = ElementSelection::Edges(shrunk);
                    }
                }
            }
        }
        return;
    }

    // Select linked (Ctrl+L)
    if ctrl && keyboard.just_pressed(KeyCode::KeyL) {
        if model_state.selection_mode == SelectionMode::Face {
            if let Some(ref edit_mesh) = model_state.edit_mesh {
                model_state.selected_faces =
                    select_ops::select_linked_faces(edit_mesh, &model_state.selected_faces);
                info!("Selected linked: {} faces", model_state.selected_faces.len());
            }
        }
        return;
    }

    // Select all (Ctrl+A)
    if ctrl && keyboard.just_pressed(KeyCode::KeyA) {
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

/// Handle mouse click for element selection (vertex/edge/face) and freeform point placement.
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

    // Don't change selection when an operation is active — clicks are for the operation
    if model_state.pending_operation != ModelOperation::Select {
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

    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);

    // Route based on selection mode
    match model_state.selection_mode {
        SelectionMode::Vertex => {
            // Vertex picking: screen-space proximity
            if let Some(hit) = pick_vertex(
                &edit_mesh,
                entity_transform,
                cursor_position,
                camera,
                camera_transform,
                20.0, // max screen distance in pixels
            ) {
                if shift {
                    let set = match &mut model_state.element_selection {
                        ElementSelection::Vertices(v) => v,
                        other => {
                            *other = ElementSelection::Vertices(std::collections::HashSet::new());
                            match other {
                                ElementSelection::Vertices(v) => v,
                                _ => unreachable!(),
                            }
                        }
                    };
                    if set.contains(&hit.vertex) {
                        set.remove(&hit.vertex);
                    } else {
                        set.insert(hit.vertex);
                    }
                } else {
                    let mut set = std::collections::HashSet::new();
                    set.insert(hit.vertex);
                    model_state.element_selection = ElementSelection::Vertices(set);
                }
                info!("Selected vertex {}", hit.vertex);
            }
        }
        SelectionMode::Edge => {
            // Edge picking: screen-space proximity to edge segments
            if let Some(ref he_mesh) = model_state.half_edge_mesh {
                if let Some(hit) = pick_edge(
                    he_mesh,
                    entity_transform,
                    cursor_position,
                    camera,
                    camera_transform,
                    15.0, // max screen distance in pixels
                    model_state.xray_selection,
                ) {
                    if alt {
                        // Alt+Click: select edge loop through the clicked edge
                        let loop_edges = select_edge_loop(he_mesh, hit.half_edge);
                        let set: std::collections::HashSet<u32> = if shift {
                            // Add loop to existing selection
                            let mut existing = match &model_state.element_selection {
                                ElementSelection::Edges(e) => e.clone(),
                                _ => std::collections::HashSet::new(),
                            };
                            existing.extend(loop_edges.iter().copied());
                            existing
                        } else {
                            loop_edges.into_iter().collect()
                        };
                        info!("Selected edge loop ({} edges)", set.len());
                        model_state.element_selection = ElementSelection::Edges(set);
                    } else if shift {
                        let set = match &mut model_state.element_selection {
                            ElementSelection::Edges(e) => e,
                            other => {
                                *other = ElementSelection::Edges(std::collections::HashSet::new());
                                match other {
                                    ElementSelection::Edges(e) => e,
                                    _ => unreachable!(),
                                }
                            }
                        };
                        if set.contains(&hit.half_edge) {
                            set.remove(&hit.half_edge);
                        } else {
                            set.insert(hit.half_edge);
                        }
                        info!("Selected edge (half-edge {})", hit.half_edge);
                    } else {
                        let mut set = std::collections::HashSet::new();
                        set.insert(hit.half_edge);
                        model_state.element_selection = ElementSelection::Edges(set);
                        info!("Selected edge (half-edge {})", hit.half_edge);
                    }
                }
            }
        }
        SelectionMode::Face => {
            // Transform ray to local space
            let (local_origin, local_dir) =
                world_to_local_ray(entity_transform, ray.origin, *ray.direction);

            let Some(hit) =
                pick_face(&edit_mesh, local_origin, local_dir, model_state.xray_selection)
            else {
                return;
            };

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
                                .filter_map(|&p| {
                                    camera.world_to_viewport(camera_transform, p).ok()
                                })
                                .collect();
                            let raw = freeform_select(
                                &edit_mesh,
                                entity_transform,
                                &screen_polygon,
                                camera,
                                camera_transform,
                            );
                            let selected = expand_to_face_groups(&edit_mesh, &raw);
                            if shift {
                                model_state.selected_faces.extend(selected);
                            } else {
                                model_state.selected_faces = selected;
                            }
                            model_state.drawing_freeform = false;
                            model_state.freeform_points.clear();
                            info!(
                                "Freeform selection: {} faces",
                                model_state.selected_faces.len()
                            );
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

            // Grid-based face selection
            let new_selection = match model_state.grid_type {
                GridType::WorldSpace => {
                    // WorldSpace grid: select triangles in the clicked grid cell.
                    // No face-group expansion — the grid IS the selection unit,
                    // letting you pick sub-regions of large faces for operations.
                    let world_point = entity_transform.transform_point(hit.point);
                    world_grid_select(
                        &edit_mesh,
                        entity_transform,
                        model_state.world_grid_size,
                        world_point,
                        hit.face,
                    )
                }
                GridType::SurfaceSpace => {
                    // SurfaceSpace: flood-fill by normal → expand to full logical faces
                    let raw = surface_group_select(&edit_mesh, hit.face, model_state.surface_angle_threshold);
                    expand_to_face_groups(&edit_mesh, &raw)
                }
                GridType::UVSpace => {
                    // UV grid: select triangles in the UV cell, no face-group expansion
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
    }
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
    // Check UI confirm button first (works even when egui has focus)
    let ui_confirm = model_state.confirm_requested;
    if ui_confirm {
        model_state.confirm_requested = false;
    }

    // Enter key only when input is not captured by UI
    let enter_pressed = should_process_input(&editor_state, &mut contexts)
        && keyboard.just_pressed(KeyCode::Enter);

    if !enter_pressed && !ui_confirm {
        return;
    }

    let Some(target) = model_state.target_entity else {
        return;
    };

    let Some(ref edit_mesh) = model_state.edit_mesh.clone() else {
        return;
    };

    // Face-based operations require face selection; edge/vertex ops check their own selection
    let face_ops = matches!(
        model_state.pending_operation,
        ModelOperation::Extrude | ModelOperation::Cut | ModelOperation::Inset | ModelOperation::PushPull
    );
    if face_ops && model_state.selected_faces.is_empty() {
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

            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            model_state.extrude_distance = 0.0;
            model_state.extrude_drag_screen_start = None;
            model_state.extrude_drag_screen_dir = None;
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
            apply_mesh_to_entity(&remaining, target, &mut commands, &mut meshes);

            // Get the material and transform info before spawning
            let entity_name = mesh_query
                .get(target)
                .ok()
                .and_then(|(_, name)| name.map(|n| n.as_str().to_string()))
                .unwrap_or_else(|| "Mesh".to_string());

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

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&remaining);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(remaining);
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            info!("Cut applied — new entity spawned");
        }
        ModelOperation::Inset => {
            if model_state.inset_distance.abs() < 1e-6 {
                return;
            }

            commands.queue(TakeSnapshotCommand {
                description: "Inset faces".to_string(),
            });

            let new_mesh = inset_faces(
                edit_mesh,
                &model_state.selected_faces,
                model_state.inset_distance,
            );

            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            info!("Inset applied");
        }
        ModelOperation::Bevel => {
            if model_state.bevel_width.abs() < 1e-6 {
                return;
            }

            // Bevel works on edges — get selected edges from element_selection
            let selected_edges = match &model_state.element_selection {
                ElementSelection::Edges(e) => e.clone(),
                _ => {
                    info!("Bevel requires edge selection (mode 2)");
                    return;
                }
            };

            if selected_edges.is_empty() {
                return;
            }

            let Some(ref he_mesh) = model_state.half_edge_mesh else {
                return;
            };

            commands.queue(TakeSnapshotCommand {
                description: "Bevel edges".to_string(),
            });

            let new_he = bevel_edges(he_mesh, &selected_edges, model_state.bevel_width);
            let new_mesh = new_he.to_edit_mesh();

            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            model_state.half_edge_mesh = Some(new_he);
            model_state.edit_mesh = Some(new_mesh);
            model_state.element_selection.clear();
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            info!("Bevel applied");
        }
        ModelOperation::PushPull => {
            if model_state.push_pull_distance.abs() < 1e-6 {
                return;
            }

            commands.queue(TakeSnapshotCommand {
                description: "Push/pull faces".to_string(),
            });

            let new_mesh = push_pull_faces(
                edit_mesh,
                &model_state.selected_faces,
                model_state.push_pull_distance,
            );

            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            model_state.push_pull_distance = 0.0;
            info!("Push/pull applied");
        }
        ModelOperation::Bridge => {
            // Bridge requires edge selection with boundary loops
            let selected_edges = match &model_state.element_selection {
                ElementSelection::Edges(e) => e.clone(),
                _ => {
                    info!("Bridge requires edge selection");
                    return;
                }
            };

            if selected_edges.is_empty() {
                return;
            }

            let Some(ref he_mesh) = model_state.half_edge_mesh else {
                return;
            };

            commands.queue(TakeSnapshotCommand {
                description: "Bridge edge loops".to_string(),
            });

            let Some(new_he) = bridge_selected_edges(he_mesh, &selected_edges) else {
                info!("Bridge failed — need two boundary loops");
                return;
            };
            let new_mesh = new_he.to_edit_mesh();

            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            model_state.half_edge_mesh = Some(new_he);
            model_state.edit_mesh = Some(new_mesh);
            model_state.element_selection.clear();
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            info!("Bridge applied");
        }
        ModelOperation::Weld => {
            // Weld uses vertex selection — get selected vertices
            let selected_verts = match &model_state.element_selection {
                ElementSelection::Vertices(v) if v.len() >= 2 => v.clone(),
                _ => {
                    info!("Weld requires at least 2 vertices selected");
                    return;
                }
            };

            commands.queue(TakeSnapshotCommand {
                description: "Weld vertices".to_string(),
            });

            let new_mesh = weld_vertices(edit_mesh, &selected_verts, model_state.weld_threshold);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.element_selection.clear();
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            info!("Weld applied");
        }
        ModelOperation::EdgeLoop => {
            // Edge loop insert requires a single edge selected as the "cut across" reference
            let selected_edges = match &model_state.element_selection {
                ElementSelection::Edges(e) => e.clone(),
                _ => {
                    info!("Edge loop insert requires edge selection");
                    return;
                }
            };

            let start_he = match selected_edges.iter().next() {
                Some(&he) => he,
                None => return,
            };

            let Some(ref he_mesh) = model_state.half_edge_mesh else {
                return;
            };

            commands.queue(TakeSnapshotCommand {
                description: "Insert edge loop".to_string(),
            });

            let new_he = insert_edge_loop(he_mesh, start_he);
            let new_mesh = new_he.to_edit_mesh();

            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            model_state.half_edge_mesh = Some(new_he);
            model_state.edit_mesh = Some(new_mesh);
            model_state.element_selection.clear();
            model_state.selected_faces.clear();
            model_state.pending_operation = ModelOperation::Select;
            info!("Edge loop inserted");
        }
        ModelOperation::Mirror => {
            commands.queue(TakeSnapshotCommand {
                description: "Mirror mesh".to_string(),
            });

            let new_mesh = mirror_mesh(edit_mesh, model_state.mirror_axis);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Mirror applied ({:?})", model_state.mirror_axis);
        }
        ModelOperation::Smooth => {
            commands.queue(TakeSnapshotCommand {
                description: "Smooth mesh".to_string(),
            });

            let new_mesh = smooth_mesh(
                edit_mesh,
                model_state.smooth_iterations,
                model_state.smooth_factor,
            );
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Smooth applied");
        }
        ModelOperation::Subdivide => {
            commands.queue(TakeSnapshotCommand {
                description: "Subdivide mesh".to_string(),
            });

            let new_mesh = subdivide_mesh(edit_mesh);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Subdivide applied");
        }
        ModelOperation::FillHoles => {
            commands.queue(TakeSnapshotCommand {
                description: "Fill holes".to_string(),
            });

            let new_mesh = fill_holes(edit_mesh);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Holes filled");
        }
        ModelOperation::PlaneCut => {
            commands.queue(TakeSnapshotCommand {
                description: "Plane cut".to_string(),
            });

            // Cut along the chosen axis through the mesh center
            let center = edit_mesh
                .positions
                .iter()
                .copied()
                .sum::<Vec3>()
                / edit_mesh.positions.len().max(1) as f32;
            let normal = match model_state.plane_cut_axis {
                super::mirror::MirrorAxis::X => Vec3::X,
                super::mirror::MirrorAxis::Y => Vec3::Y,
                super::mirror::MirrorAxis::Z => Vec3::Z,
            };

            let (front, back) = plane_cut(edit_mesh, center, normal);

            if front.triangles.is_empty() || back.triangles.is_empty() {
                info!("Plane cut produced empty geometry, skipping");
            } else {
                // Update original entity with front half
                apply_mesh_to_entity(&front, target, &mut commands, &mut meshes);

                // Spawn new entity for back half
                let back_marker = EditMeshMarker::from_edit_mesh(&back);
                let back_bevy = back.to_bevy_mesh();
                let back_collider = back_marker.to_collider();
                let back_handle = meshes.add(back_bevy);

                commands.spawn((
                    SceneEntity,
                    Name::new("Mesh (cut)"),
                    back_marker,
                    Mesh3d(back_handle),
                    back_collider,
                    avian3d::prelude::RigidBody::Static,
                ));

                let he_mesh = HalfEdgeMesh::from_edit_mesh(&front);
                model_state.half_edge_mesh = Some(he_mesh);
                model_state.edit_mesh = Some(front);
                info!("Plane cut applied");
            }
            model_state.pending_operation = ModelOperation::Select;
        }
        ModelOperation::Simplify => {
            commands.queue(TakeSnapshotCommand {
                description: "Simplify mesh".to_string(),
            });

            let new_mesh = simplify_mesh(edit_mesh, model_state.simplify_ratio);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Simplify applied (ratio: {})", model_state.simplify_ratio);
        }
        ModelOperation::Remesh => {
            commands.queue(TakeSnapshotCommand {
                description: "Remesh".to_string(),
            });

            let new_mesh = remesh(edit_mesh, model_state.remesh_edge_length);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Remesh applied (target edge: {})", model_state.remesh_edge_length);
        }
        ModelOperation::Boolean => {
            // Boolean requires a second mesh entity — not yet wired for entity picking
            info!("Boolean operations require a second mesh entity (not yet implemented)");
        }
        ModelOperation::UvProject => {
            commands.queue(TakeSnapshotCommand {
                description: "UV Project".to_string(),
            });

            let faces: std::collections::HashSet<_> = if model_state.selected_faces.is_empty() {
                (0..edit_mesh.triangles.len()).collect()
            } else {
                model_state.selected_faces.clone()
            };

            let new_mesh = project_uvs_faces(
                edit_mesh,
                &faces,
                model_state.uv_projection,
                model_state.uv_projection_axis,
                model_state.uv_projection_scale,
            );
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("UV projection applied ({:?})", model_state.uv_projection);
        }
        ModelOperation::UvUnwrap => {
            commands.queue(TakeSnapshotCommand {
                description: "UV Unwrap".to_string(),
            });

            let new_mesh = unwrap_uvs(edit_mesh, &model_state.uv_seams);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("UV unwrap applied");
        }
        ModelOperation::AutoSmooth => {
            commands.queue(TakeSnapshotCommand {
                description: "Auto smooth normals".to_string(),
            });

            let new_mesh = auto_smooth_normals_with_hard_edges(
                edit_mesh,
                model_state.auto_smooth_angle,
                &model_state.hard_edges,
            );
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Auto smooth applied ({}°)", model_state.auto_smooth_angle);
        }
        ModelOperation::FlatNormals => {
            commands.queue(TakeSnapshotCommand {
                description: "Flat normals".to_string(),
            });

            let new_mesh = flat_normals(edit_mesh);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Flat normals applied");
        }
        ModelOperation::CatmullClark => {
            commands.queue(TakeSnapshotCommand {
                description: "Catmull-Clark subdivide".to_string(),
            });

            let new_mesh = catmull_clark_subdivide(edit_mesh);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Catmull-Clark subdivide applied");
        }
        ModelOperation::SnapToGrid => {
            commands.queue(TakeSnapshotCommand {
                description: "Snap to grid".to_string(),
            });

            let faces: std::collections::HashSet<_> = if model_state.selected_faces.is_empty() {
                (0..edit_mesh.triangles.len()).collect()
            } else {
                model_state.selected_faces.clone()
            };

            let new_mesh = snap_faces_to_grid(edit_mesh, &faces, model_state.snap_grid_size);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.pending_operation = ModelOperation::Select;
            info!("Snap to grid applied");
        }
        ModelOperation::Select => {}
    }
}

/// Handle Delete key: immediately delete selected elements.
pub fn handle_model_delete(
    mut model_state: ResMut<MeshModelState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    if !model_state.delete_requested {
        return;
    }
    model_state.delete_requested = false;

    let Some(target) = model_state.target_entity else {
        return;
    };

    match model_state.selection_mode {
        SelectionMode::Face => {
            if model_state.selected_faces.is_empty() {
                return;
            }
            let Some(ref edit_mesh) = model_state.edit_mesh.clone() else {
                return;
            };

            commands.queue(TakeSnapshotCommand {
                description: "Delete faces".to_string(),
            });

            let new_mesh = delete_faces(edit_mesh, &model_state.selected_faces);
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            let he_mesh = HalfEdgeMesh::from_edit_mesh(&new_mesh);
            model_state.half_edge_mesh = Some(he_mesh);
            model_state.edit_mesh = Some(new_mesh);
            model_state.selected_faces.clear();
            info!("Deleted faces");
        }
        SelectionMode::Edge => {
            let selected_edges = match &model_state.element_selection {
                ElementSelection::Edges(e) if !e.is_empty() => e.clone(),
                _ => return,
            };
            let Some(ref he_mesh) = model_state.half_edge_mesh else {
                return;
            };

            commands.queue(TakeSnapshotCommand {
                description: "Dissolve edges".to_string(),
            });

            let new_he = dissolve_edges(he_mesh, &selected_edges);
            let new_mesh = new_he.to_edit_mesh();
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            model_state.half_edge_mesh = Some(new_he);
            model_state.edit_mesh = Some(new_mesh);
            model_state.element_selection.clear();
            info!("Dissolved edges");
        }
        SelectionMode::Vertex => {
            let selected_verts = match &model_state.element_selection {
                ElementSelection::Vertices(v) if !v.is_empty() => v.clone(),
                _ => return,
            };
            let Some(ref he_mesh) = model_state.half_edge_mesh else {
                return;
            };

            commands.queue(TakeSnapshotCommand {
                description: "Dissolve vertices".to_string(),
            });

            let new_he = dissolve_vertices(he_mesh, &selected_verts);
            let new_mesh = new_he.to_edit_mesh();
            apply_mesh_to_entity(&new_mesh, target, &mut commands, &mut meshes);

            model_state.half_edge_mesh = Some(new_he);
            model_state.edit_mesh = Some(new_mesh);
            model_state.element_selection.clear();
            info!("Dissolved vertices");
        }
    }
}

/// Handle mouse drag for extrude distance adjustment.
///
/// Uses screen-space projection: the extrude normal is projected to a 2D
/// direction on screen, and mouse movement along that direction controls
/// the extrude distance. This feels natural regardless of camera angle.
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
        model_state.extrude_drag_screen_start = None;
        model_state.extrude_drag_screen_dir = None;
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

    // Initialize drag on first frame
    if model_state.extrude_drag_screen_start.is_none() {
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

        // Project the extrude origin and a point 1 unit along the normal to screen space
        let world_origin = entity_transform.transform_point(local_centroid);
        let world_tip = entity_transform.transform_point(local_centroid + local_normal);

        let Ok(screen_origin) = camera.world_to_viewport(camera_transform, world_origin) else {
            return;
        };
        let Ok(screen_tip) = camera.world_to_viewport(camera_transform, world_tip) else {
            return;
        };

        let screen_delta = screen_tip - screen_origin;
        let pixels_per_unit = screen_delta.length();
        if pixels_per_unit < 1.0 {
            return; // Normal is perpendicular to screen — can't drag meaningfully
        }

        model_state.extrude_drag_screen_start = Some(cursor_pos);
        model_state.extrude_drag_screen_dir = Some(screen_delta / pixels_per_unit);
        model_state.extrude_drag_pixels_per_unit = pixels_per_unit;
        // Reset distance at drag start so each drag begins from zero
        model_state.extrude_distance = 0.0;
        return;
    }

    let start = model_state.extrude_drag_screen_start.unwrap();
    let dir = model_state.extrude_drag_screen_dir.unwrap();
    let ppu = model_state.extrude_drag_pixels_per_unit;

    // Project mouse delta onto the screen-space normal direction
    let mouse_delta = cursor_pos - start;
    let screen_distance = mouse_delta.dot(dir);

    // Convert screen pixels to world units
    model_state.extrude_distance = screen_distance / ppu;
}

/// Apply an EditMesh to an entity: update marker, Mesh3d, and Collider.
fn apply_mesh_to_entity(
    new_mesh: &EditMesh,
    entity: Entity,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
) {
    let marker = EditMeshMarker::from_edit_mesh(new_mesh);
    let bevy_mesh = new_mesh.to_bevy_mesh();
    let collider = marker.to_collider();
    let mesh_handle = meshes.add(bevy_mesh);

    if let Ok(mut entity_commands) = commands.get_entity(entity) {
        entity_commands.insert((marker, Mesh3d(mesh_handle), collider));
    }
}
