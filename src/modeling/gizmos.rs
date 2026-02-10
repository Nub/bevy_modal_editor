//! Gizmo rendering for the mesh modeling tool.
//!
//! Draws grid overlays, selected face highlights, extrude previews,
//! freeform polygon outlines, and cut boundary highlights.

use bevy::prelude::*;

use crate::editor::EditorState;
use crate::gizmos::XRayGizmoConfig;
use crate::selection::Selected;

use super::edit_mesh::EditMesh;
use super::{MeshModelState, ModelOperation};

// Bevy-native colors matching the theme palette
const HIGHLIGHT_ORANGE: Color = Color::srgb(0.808, 0.569, 0.341);
const PREVIEW_CYAN: Color = Color::srgb(0.306, 0.788, 0.839);
const FREEFORM_GREEN: Color = Color::srgb(0.306, 0.788, 0.690);
const CUT_RED: Color = Color::srgb(1.0, 0.2, 0.2);
const CLOSE_HINT: Color = Color::srgba(0.2, 1.0, 0.4, 0.5);

/// Draw gizmos for the mesh modeling tool: selected faces, grid overlay, operation previews.
pub fn draw_model_gizmos(
    mut gizmos: Gizmos<XRayGizmoConfig>,
    model_state: Res<MeshModelState>,
    editor_state: Res<EditorState>,
    selected_query: Query<&GlobalTransform, With<Selected>>,
) {
    if !editor_state.gizmos_visible {
        return;
    }

    let Some(ref edit_mesh) = model_state.edit_mesh else {
        return;
    };

    let Some(target) = model_state.target_entity else {
        return;
    };

    let Ok(global_transform) = selected_query.get(target) else {
        return;
    };

    // Draw selected faces
    draw_selected_faces(&mut gizmos, edit_mesh, global_transform, &model_state);

    // Draw operation-specific overlays
    match model_state.pending_operation {
        ModelOperation::Extrude if !model_state.selected_faces.is_empty() => {
            draw_extrude_preview(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
        ModelOperation::Cut if !model_state.selected_faces.is_empty() => {
            draw_cut_boundary(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
        _ => {}
    }

    // Draw freeform polygon in progress
    if model_state.drawing_freeform && !model_state.freeform_points.is_empty() {
        draw_freeform_polygon(&mut gizmos, &model_state);
    }
}

/// Highlight edges of selected faces in accent orange.
fn draw_selected_faces(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    for &fi in &state.selected_faces {
        if fi >= mesh.triangles.len() {
            continue;
        }
        let [a, b, c] = mesh.triangles[fi];
        let p0 = transform.transform_point(mesh.positions[a as usize]);
        let p1 = transform.transform_point(mesh.positions[b as usize]);
        let p2 = transform.transform_point(mesh.positions[c as usize]);

        gizmos.line(p0, p1, HIGHLIGHT_ORANGE);
        gizmos.line(p1, p2, HIGHLIGHT_ORANGE);
        gizmos.line(p2, p0, HIGHLIGHT_ORANGE);
    }
}

/// Draw ghost edges showing where extruded geometry would end up.
fn draw_extrude_preview(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    if state.extrude_distance.abs() < 1e-6 {
        return;
    }

    // Compute extrude direction
    let extrude_normal = {
        let mut sum = Vec3::ZERO;
        for &fi in &state.selected_faces {
            if fi < mesh.triangles.len() {
                sum += mesh.face_normal(fi) * mesh.face_area(fi);
            }
        }
        sum.normalize_or_zero()
    };

    let offset = extrude_normal * state.extrude_distance;

    // Draw offset edges of selected faces
    for &fi in &state.selected_faces {
        if fi >= mesh.triangles.len() {
            continue;
        }
        let [a, b, c] = mesh.triangles[fi];
        let p0 = transform.transform_point(mesh.positions[a as usize] + offset);
        let p1 = transform.transform_point(mesh.positions[b as usize] + offset);
        let p2 = transform.transform_point(mesh.positions[c as usize] + offset);

        gizmos.line(p0, p1, PREVIEW_CYAN);
        gizmos.line(p1, p2, PREVIEW_CYAN);
        gizmos.line(p2, p0, PREVIEW_CYAN);
    }

    // Draw connecting lines from boundary vertices to their offset positions
    let boundary_verts = mesh.boundary_vertices(&state.selected_faces);
    for &v in &boundary_verts {
        let p = transform.transform_point(mesh.positions[v as usize]);
        let p_offset = transform.transform_point(mesh.positions[v as usize] + offset);
        gizmos.line(p, p_offset, PREVIEW_CYAN);
    }
}

/// Draw red boundary lines showing where the cut will happen.
fn draw_cut_boundary(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    let boundary = mesh.boundary_edges(&state.selected_faces);

    for edge in &boundary {
        let p0 = transform.transform_point(mesh.positions[edge.0 as usize]);
        let p1 = transform.transform_point(mesh.positions[edge.1 as usize]);
        gizmos.line(p0, p1, CUT_RED);
    }
}

/// Draw freeform polygon segments on the surface.
fn draw_freeform_polygon(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    state: &MeshModelState,
) {
    for i in 0..state.freeform_points.len().saturating_sub(1) {
        gizmos.line(state.freeform_points[i], state.freeform_points[i + 1], FREEFORM_GREEN);
    }

    // Draw closing indicator if near first point
    if state.freeform_points.len() >= 3 {
        let first = state.freeform_points[0];
        let last = *state.freeform_points.last().unwrap();
        gizmos.line(last, first, CLOSE_HINT);
    }
}
