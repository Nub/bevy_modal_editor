//! Gizmo rendering for the mesh modeling tool.
//!
//! Draws grid overlays, selected face highlights, extrude previews,
//! freeform polygon outlines, and cut boundary highlights.

use bevy::prelude::*;

use crate::editor::EditorState;
use crate::gizmos::XRayGizmoConfig;
use crate::selection::Selected;

use super::edit_mesh::EditMesh;
use super::half_edge::HalfEdgeMesh;
use super::{ElementSelection, MeshModelState, ModelOperation, SelectionMode};

// Bevy-native colors matching the theme palette
const HIGHLIGHT_ORANGE: Color = Color::srgb(0.808, 0.569, 0.341);
const PREVIEW_CYAN: Color = Color::srgb(0.306, 0.788, 0.839);
const FREEFORM_GREEN: Color = Color::srgb(0.306, 0.788, 0.690);
const CUT_RED: Color = Color::srgb(1.0, 0.2, 0.2);
const CLOSE_HINT: Color = Color::srgba(0.2, 1.0, 0.4, 0.5);
const VERTEX_COLOR: Color = Color::srgb(1.0, 0.85, 0.3);
const EDGE_COLOR: Color = Color::srgb(0.4, 0.8, 1.0);
const SEAM_COLOR: Color = Color::srgb(0.9, 0.2, 0.2);
const HARD_EDGE_COLOR: Color = Color::srgb(0.2, 0.6, 1.0);
const VERTEX_SIZE: f32 = 0.02;

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

    // Draw element selection based on mode
    match model_state.selection_mode {
        SelectionMode::Vertex => {
            draw_selected_vertices(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
        SelectionMode::Edge => {
            if let Some(ref he_mesh) = model_state.half_edge_mesh {
                draw_selected_edges(&mut gizmos, he_mesh, global_transform, &model_state);
            }
        }
        SelectionMode::Face => {
            draw_selected_faces(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
    }

    // Draw operation-specific overlays
    match model_state.pending_operation {
        ModelOperation::Extrude if !model_state.selected_faces.is_empty() => {
            draw_extrude_preview(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
        ModelOperation::Cut if !model_state.selected_faces.is_empty() => {
            draw_cut_boundary(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
        ModelOperation::Inset if !model_state.selected_faces.is_empty() => {
            draw_inset_preview(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
        ModelOperation::PushPull if !model_state.selected_faces.is_empty() => {
            draw_push_pull_preview(&mut gizmos, edit_mesh, global_transform, &model_state);
        }
        _ => {}
    }

    // Draw seam edges
    if !model_state.uv_seams.is_empty() {
        draw_seam_edges(&mut gizmos, edit_mesh, global_transform, &model_state);
    }

    // Draw hard edges
    if !model_state.hard_edges.is_empty() {
        draw_hard_edges(&mut gizmos, edit_mesh, global_transform, &model_state);
    }

    // Draw freeform polygon in progress
    if model_state.drawing_freeform && !model_state.freeform_points.is_empty() {
        draw_freeform_polygon(&mut gizmos, &model_state);
    }
}

/// Highlight the outline boundary of selected faces in accent orange.
///
/// Only draws edges at the perimeter of the selection, not internal
/// triangle edges — so a selected quad shows 4 edges, not 5.
fn draw_selected_faces(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    let boundary = mesh.boundary_edges(&state.selected_faces);

    for edge in &boundary {
        let p0 = transform.transform_point(mesh.positions[edge.0 as usize]);
        let p1 = transform.transform_point(mesh.positions[edge.1 as usize]);
        gizmos.line(p0, p1, HIGHLIGHT_ORANGE);
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

    // Draw offset boundary of selected faces (outline only, no internal triangle edges)
    let boundary = mesh.boundary_edges(&state.selected_faces);
    for edge in &boundary {
        let p0 = transform.transform_point(mesh.positions[edge.0 as usize] + offset);
        let p1 = transform.transform_point(mesh.positions[edge.1 as usize] + offset);
        gizmos.line(p0, p1, PREVIEW_CYAN);

        // Draw connecting lines from boundary vertices to their offset positions
        let orig0 = transform.transform_point(mesh.positions[edge.0 as usize]);
        let orig1 = transform.transform_point(mesh.positions[edge.1 as usize]);
        gizmos.line(orig0, p0, PREVIEW_CYAN);
        gizmos.line(orig1, p1, PREVIEW_CYAN);
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

/// Draw selected vertices as small spheres.
fn draw_selected_vertices(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    let selected = match &state.element_selection {
        ElementSelection::Vertices(v) => v,
        _ => return,
    };

    for &vi in selected {
        if (vi as usize) < mesh.positions.len() {
            let world_pos = transform.transform_point(mesh.positions[vi as usize]);
            gizmos.sphere(world_pos, VERTEX_SIZE, VERTEX_COLOR);
        }
    }
}

/// Draw selected edges as highlighted lines.
fn draw_selected_edges(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    he_mesh: &HalfEdgeMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    let selected = match &state.element_selection {
        ElementSelection::Edges(e) => e,
        _ => return,
    };

    for &he_id in selected {
        if (he_id as usize) < he_mesh.half_edges.len() {
            let (from, to) = he_mesh.edge_vertices(he_id);
            let p0 = transform.transform_point(he_mesh.vertices[from as usize].position);
            let p1 = transform.transform_point(he_mesh.vertices[to as usize].position);
            gizmos.line(p0, p1, EDGE_COLOR);
            // Draw a second line slightly offset for thickness
            let up = (p1 - p0).cross(transform.forward().as_vec3()).normalize_or_zero() * 0.003;
            gizmos.line(p0 + up, p1 + up, EDGE_COLOR);
            gizmos.line(p0 - up, p1 - up, EDGE_COLOR);
        }
    }
}

/// Draw inset preview: inner face outlines offset toward centroids.
fn draw_inset_preview(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    if state.inset_distance.abs() < 1e-6 {
        return;
    }

    let inset_frac = state.inset_distance.clamp(0.0, 0.99);

    for &fi in &state.selected_faces {
        if fi >= mesh.triangles.len() {
            continue;
        }
        let [a, b, c] = mesh.triangles[fi];
        let p0 = mesh.positions[a as usize];
        let p1 = mesh.positions[b as usize];
        let p2 = mesh.positions[c as usize];
        let center = (p0 + p1 + p2) / 3.0;

        let q0 = transform.transform_point(p0.lerp(center, inset_frac));
        let q1 = transform.transform_point(p1.lerp(center, inset_frac));
        let q2 = transform.transform_point(p2.lerp(center, inset_frac));

        gizmos.line(q0, q1, PREVIEW_CYAN);
        gizmos.line(q1, q2, PREVIEW_CYAN);
        gizmos.line(q2, q0, PREVIEW_CYAN);

        // Draw connecting lines from original to inset vertices
        let wp0 = transform.transform_point(p0);
        let wp1 = transform.transform_point(p1);
        let wp2 = transform.transform_point(p2);
        gizmos.line(wp0, q0, PREVIEW_CYAN);
        gizmos.line(wp1, q1, PREVIEW_CYAN);
        gizmos.line(wp2, q2, PREVIEW_CYAN);
    }
}

/// Draw UV seam edges as red highlights.
fn draw_seam_edges(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    for &(a, b) in &state.uv_seams {
        let a = a as usize;
        let b = b as usize;
        if a < mesh.positions.len() && b < mesh.positions.len() {
            let p0 = transform.transform_point(mesh.positions[a]);
            let p1 = transform.transform_point(mesh.positions[b]);
            gizmos.line(p0, p1, SEAM_COLOR);
        }
    }
}

/// Draw hard edges as blue highlights.
fn draw_hard_edges(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    for edge in &state.hard_edges {
        let a = edge.0 as usize;
        let b = edge.1 as usize;
        if a < mesh.positions.len() && b < mesh.positions.len() {
            let p0 = transform.transform_point(mesh.positions[a]);
            let p1 = transform.transform_point(mesh.positions[b]);
            gizmos.line(p0, p1, HARD_EDGE_COLOR);
        }
    }
}

/// Draw push/pull preview: offset boundary along individual face normals.
fn draw_push_pull_preview(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    mesh: &EditMesh,
    transform: &GlobalTransform,
    state: &MeshModelState,
) {
    if state.push_pull_distance.abs() < 1e-6 {
        return;
    }

    // Compute per-vertex average offset from adjacent selected face normals
    let mut vert_offset: Vec<Vec3> = vec![Vec3::ZERO; mesh.positions.len()];
    let mut vert_count: Vec<u32> = vec![0; mesh.positions.len()];
    for &fi in &state.selected_faces {
        if fi >= mesh.triangles.len() {
            continue;
        }
        let normal = mesh.face_normal(fi);
        for &v in &mesh.triangles[fi] {
            vert_offset[v as usize] += normal * state.push_pull_distance;
            vert_count[v as usize] += 1;
        }
    }
    for i in 0..vert_offset.len() {
        if vert_count[i] > 1 {
            vert_offset[i] /= vert_count[i] as f32;
        }
    }

    // Draw only boundary edges of the offset selection
    let boundary = mesh.boundary_edges(&state.selected_faces);
    for edge in &boundary {
        let p0 = transform.transform_point(
            mesh.positions[edge.0 as usize] + vert_offset[edge.0 as usize],
        );
        let p1 = transform.transform_point(
            mesh.positions[edge.1 as usize] + vert_offset[edge.1 as usize],
        );
        gizmos.line(p0, p1, PREVIEW_CYAN);
    }
}
