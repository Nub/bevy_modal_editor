//! Asset identity (M4-D1): every imported source asset gets a `.import.ron`
//! sidecar — UUID (stable across re-imports; THE reference target), blake3
//! content hash (staleness detection for the Process stage), pipeline version
//! (migration gate). The sidecar name avoids Bevy's own `.meta` namespace.
//!
//! Contract (pinned by tests):
//! - first import assigns a fresh UUID and writes the sidecar
//! - re-import PRESERVES the UUID and refreshes the content hash
//! - a sidecar from a FUTURE pipeline refuses loudly (never guessed at)
//! - a corrupt sidecar is an error, never silently regenerated (that would
//!   mint a new UUID and orphan every reference)

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const IMPORT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetIdentity {
    pub format_version: u32,
    /// THE stable reference target. Never changes once assigned.
    pub uuid: Uuid,
    /// blake3 of the source bytes at last import (hex).
    pub content_hash: String,
    /// Bumped when import semantics change; migration chains hang off this.
    pub pipeline_version: u32,
}

impl Default for AssetIdentity {
    fn default() -> Self {
        Self {
            format_version: IMPORT_FORMAT_VERSION,
            uuid: Uuid::nil(),
            content_hash: String::new(),
            pipeline_version: 1,
        }
    }
}

#[derive(Debug)]
pub enum ImportError {
    Io(std::io::Error),
    /// Sidecar exists but cannot be understood — regenerating would orphan
    /// references, so this is fatal and loud.
    CorruptSidecar { path: PathBuf, message: String },
    FutureVersion { found: u32, supported: u32 },
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "asset import io: {e}"),
            Self::CorruptSidecar { path, message } => write!(
                f,
                "corrupt import sidecar {} ({message}) — fix or delete it DELIBERATELY \
                 (deleting mints a new identity and orphans references)",
                path.display()
            ),
            Self::FutureVersion { found, supported } => write!(
                f,
                "import sidecar format {found} is newer than supported {supported} — \
                 upgrade the editor"
            ),
        }
    }
}
impl std::error::Error for ImportError {}

pub fn sidecar_path(source: &Path) -> PathBuf {
    let mut name = source.file_name().unwrap_or_default().to_os_string();
    name.push(".import.ron");
    source.with_file_name(name)
}

/// Read an existing identity without importing. `Ok(None)` = never imported.
pub fn read_identity(source: &Path) -> Result<Option<AssetIdentity>, ImportError> {
    let sidecar = sidecar_path(source);
    let text = match std::fs::read_to_string(&sidecar) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ImportError::Io(e)),
    };
    let identity: AssetIdentity = ron::from_str(&text).map_err(|e| {
        ImportError::CorruptSidecar { path: sidecar.clone(), message: e.to_string() }
    })?;
    if identity.format_version > IMPORT_FORMAT_VERSION {
        return Err(ImportError::FutureVersion {
            found: identity.format_version,
            supported: IMPORT_FORMAT_VERSION,
        });
    }
    Ok(Some(identity))
}

/// Import (or re-import) a source asset: assigns identity on first sight,
/// PRESERVES the UUID and refreshes the hash on every later import.
pub fn import_file(source: &Path) -> Result<AssetIdentity, ImportError> {
    let bytes = std::fs::read(source).map_err(ImportError::Io)?;
    let content_hash = blake3::hash(&bytes).to_hex().to_string();

    let mut identity = match read_identity(source)? {
        Some(existing) => existing,
        None => AssetIdentity { uuid: Uuid::new_v4(), ..Default::default() },
    };
    identity.format_version = IMPORT_FORMAT_VERSION;
    identity.content_hash = content_hash;

    let text = ron::ser::to_string_pretty(&identity, ron::ser::PrettyConfig::default())
        .map_err(|e| ImportError::CorruptSidecar {
            path: sidecar_path(source),
            message: e.to_string(),
        })?;
    // Atomic sidecar write (same discipline as scenes/materials).
    let sidecar = sidecar_path(source);
    let tmp = sidecar.with_extension("ron.tmp");
    std::fs::write(&tmp, &text).map_err(ImportError::Io)?;
    std::fs::rename(&tmp, &sidecar).map_err(ImportError::Io)?;
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    // D1: identity assigned once, preserved across re-imports, hash follows
    // content, future versions refuse, corruption is loud.
    #[test]
    fn identity_contract() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("barrel.glb");
        std::fs::write(&source, b"glb-bytes-v1").unwrap();

        let first = import_file(&source).unwrap();
        assert_ne!(first.uuid, Uuid::nil());
        assert!(sidecar_path(&source).exists());

        // Re-import unchanged: same uuid, same hash.
        let second = import_file(&source).unwrap();
        assert_eq!(second, first);

        // Artist re-exports: hash changes, UUID SURVIVES (references intact).
        std::fs::write(&source, b"glb-bytes-v2").unwrap();
        let third = import_file(&source).unwrap();
        assert_eq!(third.uuid, first.uuid, "uuid survives re-import");
        assert_ne!(third.content_hash, first.content_hash);

        // Future sidecar refuses.
        std::fs::write(
            sidecar_path(&source),
            "(format_version: 99, uuid: \"6a2f6b6e-9d5c-4f4b-8f34-3f2a2b1c0d9e\", \
             content_hash: \"x\", pipeline_version: 1)",
        )
        .unwrap();
        assert!(matches!(
            import_file(&source),
            Err(ImportError::FutureVersion { found: 99, .. })
        ));

        // Corruption is loud, never silently regenerated.
        std::fs::write(sidecar_path(&source), "not ron at all {{{").unwrap();
        assert!(matches!(import_file(&source), Err(ImportError::CorruptSidecar { .. })));
    }
}
