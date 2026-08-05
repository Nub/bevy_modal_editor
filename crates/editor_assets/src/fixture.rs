//! Deterministic GLB fixture (M4-D12): the barrel workflow needs a REAL binary
//! GLTF that Bevy's loader accepts, small enough to author byte-for-byte in
//! code. `barrel_glb(scale)` builds a closed box "barrel" (base on y=0) with
//! positions, normals, indices, and one material — different `scale` values
//! model the artist re-exporting the source asset (bytes change, identity
//! must survive).
//!
//! Probes and CI build fixtures from THIS function so the corpus never rots in
//! a binary blob nobody can regenerate.

/// A complete, valid GLB: one mesh (24 verts, 36 indices) shared by TWO nodes
/// (body + a thin "lid" child with its own TRS — real hierarchy for the
/// flatten-to-entities flow), one material, one scene. `scale` multiplies the
/// whole body uniformly.
pub fn barrel_glb(scale: f32) -> Vec<u8> {
    let r = 0.4 * scale; // half-width
    let h = 1.0 * scale; // height (base sits on y=0)

    // 6 faces × 4 corners, CCW from outside; flat normals.
    #[rustfmt::skip]
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, 1.0],  [[-r, 0.0, r], [r, 0.0, r], [r, h, r], [-r, h, r]]),
        ([0.0, 0.0, -1.0], [[r, 0.0, -r], [-r, 0.0, -r], [-r, h, -r], [r, h, -r]]),
        ([1.0, 0.0, 0.0],  [[r, 0.0, r], [r, 0.0, -r], [r, h, -r], [r, h, r]]),
        ([-1.0, 0.0, 0.0], [[-r, 0.0, -r], [-r, 0.0, r], [-r, h, r], [-r, h, -r]]),
        ([0.0, 1.0, 0.0],  [[-r, h, r], [r, h, r], [r, h, -r], [-r, h, -r]]),
        ([0.0, -1.0, 0.0], [[-r, 0.0, -r], [r, 0.0, -r], [r, 0.0, r], [-r, 0.0, r]]),
    ];

    let mut positions: Vec<u8> = Vec::new();
    let mut normals: Vec<u8> = Vec::new();
    let mut indices: Vec<u8> = Vec::new();
    for (face, (normal, corners)) in faces.iter().enumerate() {
        for corner in corners {
            for c in corner {
                positions.extend_from_slice(&c.to_le_bytes());
            }
            for c in normal {
                normals.extend_from_slice(&c.to_le_bytes());
            }
        }
        let base = (face * 4) as u16;
        for i in [0u16, 1, 2, 0, 2, 3] {
            indices.extend_from_slice(&(base + i).to_le_bytes());
        }
    }

    let pos_len = positions.len(); // 288
    let norm_len = normals.len(); // 288
    let idx_len = indices.len(); // 72
    let mut bin = positions;
    bin.extend_from_slice(&normals);
    bin.extend_from_slice(&indices);
    let bin_len = bin.len();

    let json = format!(
        concat!(
            r#"{{"asset":{{"version":"2.0","generator":"editor_assets fixture"}},"#,
            r#""scene":0,"scenes":[{{"nodes":[0]}}],"#,
            r#""nodes":[
{{"mesh":0,"name":"barrel","children":[1]}},
{{"mesh":0,"name":"lid","translation":[0.0,{max_y},0.0],"scale":[0.8,0.1,0.8]}}],"#,
            r#""meshes":[{{"name":"barrel","primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1}},"indices":2,"material":0}}]}}],"#,
            r#""materials":[{{"name":"barrel-wood","pbrMetallicRoughness":{{"baseColorFactor":[0.55,0.38,0.22,1.0],"metallicFactor":0.0,"roughnessFactor":0.8}}}}],"#,
            r#""accessors":[
{{"bufferView":0,"componentType":5126,"count":24,"type":"VEC3","min":[{min_x},0.0,{min_z}],"max":[{max_x},{max_y},{max_z}]}},
{{"bufferView":1,"componentType":5126,"count":24,"type":"VEC3"}},
{{"bufferView":2,"componentType":5123,"count":36,"type":"SCALAR"}}],"#,
            r#""bufferViews":[
{{"buffer":0,"byteOffset":0,"byteLength":{pos_len}}},
{{"buffer":0,"byteOffset":{pos_len},"byteLength":{norm_len}}},
{{"buffer":0,"byteOffset":{idx_off},"byteLength":{idx_len}}}],"#,
            r#""buffers":[{{"byteLength":{bin_len}}}]}}"#
        ),
        min_x = -r,
        min_z = -r,
        max_x = r,
        max_y = h,
        max_z = r,
        pos_len = pos_len,
        norm_len = norm_len,
        idx_off = pos_len + norm_len,
        idx_len = idx_len,
        bin_len = bin_len,
    );

    // GLB container: header + JSON chunk (space-padded) + BIN chunk (zero-padded).
    let mut json_bytes = json.into_bytes();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();

    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json_bytes);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);
    glb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::{builtin_validators, run_validators};
    use std::path::Path;

    // D12 fixture contract: the generated GLB parses as real GLTF, passes the
    // builtin validators CLEAN, and different scales produce different bytes
    // (re-export simulation) while staying valid.
    #[test]
    fn fixture_is_valid_gltf() {
        for scale in [1.0f32, 2.5] {
            let bytes = barrel_glb(scale);
            let glb = gltf::Glb::from_slice(&bytes).expect("container parses");
            let root = gltf::json::Root::from_slice(&glb.json).expect("json parses");
            assert_eq!(root.meshes.len(), 1);
            assert_eq!(root.nodes.len(), 2, "body + lid hierarchy");
            assert_eq!(root.accessors[0].count.0, 24);
            assert_eq!(root.accessors[2].count.0, 36);
            let bin = glb.bin.expect("has binary chunk");
            assert_eq!(bin.len() % 4, 0);

            let problems = run_validators(Path::new("barrel.glb"), &bytes, &builtin_validators());
            assert!(problems.is_empty(), "fixture validates clean: {problems:?}");
        }
        assert_ne!(
            barrel_glb(1.0),
            barrel_glb(2.5),
            "re-export changes bytes (identity test depends on it)"
        );
    }
}
