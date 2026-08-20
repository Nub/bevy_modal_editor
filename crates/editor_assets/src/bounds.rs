//! `gltf.bounds` — the first real processor (spec §6 Process: "collider
//! generation … recorded in meta", in its smallest correct form).
//!
//! **What the editor cannot currently know:** how big an asset is until it has
//! been loaded AND spawned. Three places already want the answer and give up
//! without it — `piece_bounds` in prefab authoring, socket generation, and the
//! socket snap, each of which quietly does nothing when the model has not
//! finished loading. "Select the wall you just imported, generate sockets, get
//! nothing" is the behaviour that falls out.
//!
//! **Why it can be answered at import for free:** glTF REQUIRES `min`/`max` on
//! POSITION accessors, so the bounding box of every primitive is already in the
//! JSON chunk — the same chunk the validators parse, and the same bytes the
//! cache is keyed on. No buffer decode, no new dependency, and — unlike mesh
//! optimisation or anything that reads external `.bin` buffers — it is honest
//! under a cache key that hashes ONE file.
//!
//! Node transforms are applied on the way down, so the record is in the same
//! space the editor spawns the model into: the derived subtree is parented at
//! identity, which makes these numbers root-local without conversion.

use crate::{ProcessCx, ProcessorDef};
use editor_api::prelude::ProcessorId;
use serde::{Deserialize, Serialize};

/// The id the model library looks for when caching bounds in memory.
pub const BOUNDS_PROCESSOR: &str = "gltf.bounds";

/// An axis-aligned box in the model's own scene space, plus the counts a
/// budget rule and an asset browser both want.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct ModelBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Triangles across every primitive — indices/3, or positions/3 when the
    /// primitive is not indexed.
    pub triangles: usize,
    /// False when at least one primitive could not be measured (a POSITION
    /// accessor without `min`/`max`, which the spec forbids but exporters
    /// still ship). The box is then a lower bound, and saying so is the
    /// difference between a small number and a wrong one.
    pub complete: bool,
}

impl ModelBounds {
    pub fn size(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}

pub fn processor() -> ProcessorDef {
    ProcessorDef {
        id: ProcessorId::new_static(BOUNDS_PROCESSOR),
        name: "Model Bounds",
        // Bump to invalidate every cached record — the traversal changing IS a
        // content change as far as anything reading these numbers is concerned.
        version: 1,
        extensions: &["glb", "gltf"],
        process: |cx| {
            let bounds = measure(cx)?;
            ron::ser::to_string_pretty(&bounds, ron::ser::PrettyConfig::default())
                .map(String::into_bytes)
                .map_err(|e| format!("cannot serialize bounds: {e}"))
        },
    }
}

fn measure(cx: &ProcessCx) -> Result<ModelBounds, String> {
    let root = crate::validation::parse_root(cx.bytes)
        .map_err(|e| format!("{}: {e}", cx.source.display()))?;
    Ok(scene_bounds(&root))
}

/// Union of every reachable primitive's box, each pushed through its node chain.
pub fn scene_bounds(root: &gltf::json::Root) -> ModelBounds {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut triangles = 0usize;
    let mut complete = true;
    let mut measured = false;

    // Every scene's roots — a file with no `scenes` array still has nodes worth
    // measuring, so fall back to walking them all rather than reporting nothing.
    let roots: Vec<usize> = if root.scenes.is_empty() {
        (0..root.nodes.len()).collect()
    } else {
        root.scenes
            .iter()
            .flat_map(|scene| scene.nodes.iter().map(|index| index.value()))
            .collect()
    };
    let mut stack: Vec<(usize, [f32; 16])> =
        roots.into_iter().map(|index| (index, IDENTITY)).collect();
    // Depth cap: a malformed file can describe a cycle, and an import that
    // hangs the editor is worse than one that reports a partial box.
    let mut visits = 0usize;
    while let Some((index, parent)) = stack.pop() {
        visits += 1;
        if visits > 10_000 {
            complete = false;
            break;
        }
        let Some(node) = root.nodes.get(index) else {
            continue;
        };
        let world = multiply(parent, local_matrix(node));
        if let Some(mesh_index) = node.mesh
            && let Some(mesh) = root.meshes.get(mesh_index.value())
        {
            for primitive in &mesh.primitives {
                triangles += triangle_count(root, primitive);
                match primitive_box(root, primitive) {
                    Some((local_min, local_max)) => {
                        measured = true;
                        for corner in corners(local_min, local_max) {
                            let point = transform(world, corner);
                            for axis in 0..3 {
                                min[axis] = min[axis].min(point[axis]);
                                max[axis] = max[axis].max(point[axis]);
                            }
                        }
                    }
                    None => complete = false,
                }
            }
        }
        for child in node.children.iter().flatten() {
            stack.push((child.value(), world));
        }
    }
    if !measured {
        return ModelBounds {
            min: [0.0; 3],
            max: [0.0; 3],
            triangles,
            complete: false,
        };
    }
    ModelBounds {
        min,
        max,
        triangles,
        complete,
    }
}

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// Column-major, as glTF stores them.
fn local_matrix(node: &gltf::json::scene::Node) -> [f32; 16] {
    if let Some(matrix) = node.matrix {
        return matrix;
    }
    let t = node.translation.unwrap_or([0.0; 3]);
    let r = node.rotation.map(|q| q.0).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let s = node.scale.unwrap_or([1.0; 3]);
    let (x, y, z, w) = (r[0], r[1], r[2], r[3]);
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    [
        (1.0 - 2.0 * (yy + zz)) * s[0],
        (2.0 * (xy + wz)) * s[0],
        (2.0 * (xz - wy)) * s[0],
        0.0,
        (2.0 * (xy - wz)) * s[1],
        (1.0 - 2.0 * (xx + zz)) * s[1],
        (2.0 * (yz + wx)) * s[1],
        0.0,
        (2.0 * (xz + wy)) * s[2],
        (2.0 * (yz - wx)) * s[2],
        (1.0 - 2.0 * (xx + yy)) * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

fn multiply(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = (0..4)
                .map(|k| a[k * 4 + row] * b[column * 4 + k])
                .sum::<f32>();
        }
    }
    out
}

fn transform(m: [f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn corners(min: [f32; 3], max: [f32; 3]) -> [[f32; 3]; 8] {
    [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [min[0], max[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [min[0], max[1], max[2]],
        [max[0], max[1], max[2]],
    ]
}

fn position_accessor<'a>(
    root: &'a gltf::json::Root,
    primitive: &gltf::json::mesh::Primitive,
) -> Option<&'a gltf::json::Accessor> {
    let index = primitive.attributes.iter().find_map(|(semantic, index)| {
        matches!(
            semantic,
            gltf::json::validation::Checked::Valid(gltf::json::mesh::Semantic::Positions)
        )
        .then_some(*index)
    })?;
    root.accessors.get(index.value())
}

fn primitive_box(
    root: &gltf::json::Root,
    primitive: &gltf::json::mesh::Primitive,
) -> Option<([f32; 3], [f32; 3])> {
    let accessor = position_accessor(root, primitive)?;
    let min = triple(accessor.min.as_ref()?)?;
    let max = triple(accessor.max.as_ref()?)?;
    Some((min, max))
}

fn triple(value: &gltf::json::Value) -> Option<[f32; 3]> {
    let array = value.as_array()?;
    if array.len() < 3 {
        return None;
    }
    Some([
        array[0].as_f64()? as f32,
        array[1].as_f64()? as f32,
        array[2].as_f64()? as f32,
    ])
}

fn triangle_count(root: &gltf::json::Root, primitive: &gltf::json::mesh::Primitive) -> usize {
    let count = match primitive.indices {
        Some(index) => root.accessors.get(index.value()).map(|a| a.count.0),
        None => position_accessor(root, primitive).map(|a| a.count.0),
    };
    count.unwrap_or(0) as usize / 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn measure_bytes(bytes: &[u8]) -> ModelBounds {
        let root = crate::validation::parse_root(bytes).unwrap();
        scene_bounds(&root)
    }

    /// The fixture is a unit-ish barrel scaled by the export factor: the box
    /// has to GROW with it, or the number is decorative.
    #[test]
    fn bounds_follow_the_export_scale() {
        let small = measure_bytes(&crate::fixture::barrel_glb(1.0));
        let large = measure_bytes(&crate::fixture::barrel_glb(2.5));
        assert!(small.complete, "the fixture declares min/max");
        assert!(small.triangles > 0, "and has triangles");
        let (a, b) = (small.size(), large.size());
        for axis in 0..3 {
            assert!(
                b[axis] > a[axis] * 2.0,
                "axis {axis} grew with the re-export: {a:?} -> {b:?}"
            );
        }
        assert_eq!(
            small.triangles, large.triangles,
            "a re-export at another scale is the same mesh"
        );
    }

    /// Determinism is the processor contract (`editor_api::pipeline`): same
    /// bytes, byte-identical output, or the cache is lying every time it hits.
    #[test]
    fn the_record_is_byte_identical_across_runs() {
        let def = processor();
        let bytes = crate::fixture::barrel_glb(1.0);
        let cx = ProcessCx {
            source: Path::new("barrel.glb"),
            bytes: &bytes,
        };
        let first = (def.process)(&cx).unwrap();
        let second = (def.process)(&cx).unwrap();
        assert_eq!(first, second);
        let parsed: ModelBounds = ron::de::from_bytes(&first).unwrap();
        assert_eq!(parsed, measure_bytes(&bytes), "and round-trips");
    }

    /// A file the parser rejects is a processor ERROR, which the ingest stage
    /// turns into a problem — never a silently empty box that reads as "this
    /// model is zero-sized".
    #[test]
    fn an_unreadable_model_fails_loudly() {
        let def = processor();
        let outcome = (def.process)(&ProcessCx {
            source: Path::new("broken.glb"),
            bytes: b"not a glb",
        });
        assert!(outcome.is_err());
    }
}
