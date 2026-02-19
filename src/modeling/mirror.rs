//! Mirror mesh across an axis-aligned plane.
//!
//! Duplicates all geometry and reflects positions across the chosen axis,
//! flipping triangle winding to preserve correct face orientation.

use super::edit_mesh::EditMesh;

/// Axis to mirror across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorAxis {
    X,
    Y,
    Z,
}

impl MirrorAxis {
    pub fn display_name(&self) -> &'static str {
        match self {
            MirrorAxis::X => "X",
            MirrorAxis::Y => "Y",
            MirrorAxis::Z => "Z",
        }
    }
}

/// Mirror the entire mesh across an axis-aligned plane through the origin.
///
/// The result contains both the original and the mirrored geometry, merged
/// into a single mesh. The mirrored triangles have their winding reversed
/// so normals face outward correctly.
pub fn mirror_mesh(mesh: &EditMesh, axis: MirrorAxis) -> EditMesh {
    let vert_count = mesh.positions.len() as u32;

    // Start with a copy of the original geometry
    let mut positions = mesh.positions.clone();
    let mut normals = mesh.normals.clone();
    let mut uvs = mesh.uvs.clone();
    let mut triangles = mesh.triangles.clone();

    // Add mirrored vertices
    for i in 0..mesh.positions.len() {
        let mut pos = mesh.positions[i];
        let mut nor = mesh.normals[i];
        match axis {
            MirrorAxis::X => {
                pos.x = -pos.x;
                nor.x = -nor.x;
            }
            MirrorAxis::Y => {
                pos.y = -pos.y;
                nor.y = -nor.y;
            }
            MirrorAxis::Z => {
                pos.z = -pos.z;
                nor.z = -nor.z;
            }
        }
        positions.push(pos);
        normals.push(nor);
        uvs.push(mesh.uvs[i]);
    }

    // Add mirrored triangles with reversed winding
    for tri in &mesh.triangles {
        triangles.push([
            tri[0] + vert_count,
            tri[2] + vert_count, // swap 1 and 2 to reverse winding
            tri[1] + vert_count,
        ]);
    }

    EditMesh {
        positions,
        normals,
        uvs,
        triangles,
    }
}
