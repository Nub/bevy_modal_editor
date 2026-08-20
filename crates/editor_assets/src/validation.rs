//! Validation runner + built-in GLTF validators (M4-D2). The registry is the
//! extension point (`FeatureRegistry::validator`); this module runs whatever
//! the catalog holds against a source asset and returns `Problem`s — the
//! caller surfaces them (import report / problems panel). Never a silent pass.

use editor_api::prelude::ValidatorId;
use editor_api::validate::{Problem, Severity, ValidateCx, ValidatorDef};
use std::path::Path;

/// Run every applicable validator (by extension) against a source asset.
pub fn run_validators(source: &Path, bytes: &[u8], validators: &[ValidatorDef]) -> Vec<Problem> {
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    let cx = ValidateCx { source, bytes };
    validators
        .iter()
        .filter(|v| v.extensions.is_empty() || v.extensions.contains(&extension.as_str()))
        .flat_map(|v| (v.validate)(&cx))
        .collect()
}

/// The built-in minimum (gate D2): units sanity + missing materials for GLTF,
/// plus an any-asset emptiness check. Games register theirs on top.
pub fn builtin_validators() -> Vec<ValidatorDef> {
    vec![
        ValidatorDef {
            id: ValidatorId::new_static("asset.nonempty"),
            name: "Non-empty source",
            extensions: &[],
            validate: |cx| {
                if cx.bytes.is_empty() {
                    vec![Problem {
                        validator: ValidatorId::new_static("asset.nonempty"),
                        severity: Severity::Error,
                        message: format!("{} is empty", cx.source.display()),
                    }]
                } else {
                    Vec::new()
                }
            },
        },
        ValidatorDef {
            id: ValidatorId::new_static("gltf.units"),
            name: "GLTF units sanity",
            extensions: &["gltf", "glb"],
            validate: validate_gltf_units,
        },
        ValidatorDef {
            id: ValidatorId::new_static("gltf.materials"),
            name: "GLTF missing materials",
            extensions: &["gltf", "glb"],
            validate: validate_gltf_materials,
        },
    ]
}

/// Parse the JSON layer of a `.glb` or `.gltf`, shared by Validate and
/// Process: both stages inspect STRUCTURE, and there must be exactly one
/// answer to "is this file readable" across the pipeline.
pub(crate) fn parse_root(bytes: &[u8]) -> Result<gltf::json::Root, String> {
    let json_bytes: std::borrow::Cow<[u8]> = if bytes.starts_with(b"glTF") {
        match gltf::Glb::from_slice(bytes) {
            Ok(glb) => std::borrow::Cow::Owned(glb.json.into_owned()),
            Err(e) => return Err(format!("not a readable GLB: {e}")),
        }
    } else {
        std::borrow::Cow::Borrowed(bytes)
    };
    gltf::json::Root::from_slice(&json_bytes).map_err(|e| format!("not a readable GLTF: {e}"))
}

/// Structural parse (gltf::json layer): validators inspect STRUCTURE — full
/// buffer validation belongs to the Process stage, and demanding it here would
/// reject perfectly checkable test fixtures and partial exports.
fn parse_gltf(cx: &ValidateCx) -> Result<gltf::json::Root, Problem> {
    let json_bytes: std::borrow::Cow<[u8]> = if cx.bytes.starts_with(b"glTF") {
        match gltf::Glb::from_slice(cx.bytes) {
            Ok(glb) => std::borrow::Cow::Owned(glb.json.into_owned()),
            Err(e) => {
                return Err(Problem {
                    validator: ValidatorId::new_static("gltf.parse"),
                    severity: Severity::Error,
                    message: format!("{}: not a readable GLB: {e}", cx.source.display()),
                });
            }
        }
    } else {
        std::borrow::Cow::Borrowed(cx.bytes)
    };
    gltf::json::Root::from_slice(&json_bytes).map_err(|e| Problem {
        validator: ValidatorId::new_static("gltf.parse"),
        severity: Severity::Error,
        message: format!("{}: not a readable GLTF: {e}", cx.source.display()),
    })
}

/// Node scales wildly outside [0.01, 100] almost always mean a units mismatch
/// (cm-vs-m exports) — the classic "my prop is a building" import.
fn validate_gltf_units(cx: &ValidateCx) -> Vec<Problem> {
    let root = match parse_gltf(cx) {
        Ok(g) => g,
        Err(p) => return vec![p],
    };
    let mut problems = Vec::new();
    for node in &root.nodes {
        let Some(scale) = node.scale else { continue };
        if scale
            .iter()
            .any(|s| s.abs() > 100.0 || (s.abs() < 0.01 && s.abs() > 0.0))
        {
            problems.push(Problem {
                validator: ValidatorId::new_static("gltf.units"),
                severity: Severity::Warning,
                message: format!(
                    "node {:?} scale {:?} suggests a units mismatch (cm vs m export?)",
                    node.name.as_deref().unwrap_or("<unnamed>"),
                    scale
                ),
            });
        }
    }
    problems
}

fn validate_gltf_materials(cx: &ValidateCx) -> Vec<Problem> {
    let root = match parse_gltf(cx) {
        Ok(g) => g,
        Err(p) => return vec![p],
    };
    let mut problems = Vec::new();
    for mesh in &root.meshes {
        for (index, primitive) in mesh.primitives.iter().enumerate() {
            if primitive.material.is_none() {
                problems.push(Problem {
                    validator: ValidatorId::new_static("gltf.materials"),
                    severity: Severity::Warning,
                    message: format!(
                        "mesh {:?} primitive {index} has no material (default-material render)",
                        mesh.name.as_deref().unwrap_or("<unnamed>")
                    ),
                });
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_gltf(scale: f32, with_material: bool) -> String {
        let material_field = if with_material {
            r#","material":0"#
        } else {
            ""
        };
        let materials = if with_material {
            r#""materials":[{"name":"m"}],"#
        } else {
            ""
        };
        format!(
            r#"{{"asset":{{"version":"2.0"}},{materials}
"scenes":[{{"nodes":[0]}}],"scene":0,
"nodes":[{{"mesh":0,"scale":[{scale},{scale},{scale}],"name":"prop"}}],
"meshes":[{{"name":"prop-mesh","primitives":[{{"attributes":{{}}{material_field}}}]}}]}}"#
        )
    }

    // D2: registry runs by extension; unit and material validators catch the
    // classic import mistakes; clean assets pass clean.
    #[test]
    fn gltf_validators_catch_problems() {
        let validators = builtin_validators();
        let path = Path::new("prop.gltf");

        let bad = tiny_gltf(1000.0, false);
        let problems = run_validators(path, bad.as_bytes(), &validators);
        assert!(
            problems
                .iter()
                .any(|p| p.validator.as_str() == "gltf.units"),
            "units mismatch flagged: {problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p.validator.as_str() == "gltf.materials"),
            "missing material flagged"
        );

        let good = tiny_gltf(1.0, true);
        let problems = run_validators(path, good.as_bytes(), &validators);
        assert!(
            problems.is_empty(),
            "clean asset passes clean: {problems:?}"
        );

        // Non-gltf assets skip gltf validators entirely.
        let problems = run_validators(Path::new("readme.txt"), b"hi", &validators);
        assert!(problems.is_empty());

        // Any-asset validator: empty file is an ERROR.
        let problems = run_validators(Path::new("empty.png"), b"", &validators);
        assert!(matches!(
            problems.first().map(|p| p.severity),
            Some(Severity::Error)
        ));
    }
}
