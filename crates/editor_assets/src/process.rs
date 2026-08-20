//! Process stage (M4-D3): deterministic, hash-keyed, cached asset processing.
//!
//! Cache key = blake3(source bytes) x processor id x processor VERSION — same
//! input + same version is a hit (never re-run); a version bump or content
//! change misses automatically. Outputs write atomically into the cache dir.
//!
//! NOTE (ledger #10): Bevy's `AssetProcessor` is the designated engine for
//! app-integrated processing once the editor adopts processed-mode assets at
//! cook time (D12 packaging). This runner implements the SAME contract
//! (deterministic, hash-keyed, version-invalidated) so that migration is
//! mechanical; processors registered through `editor_api` carry over as-is.

use editor_api::pipeline::{ProcessCx, ProcessorDef};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ProcessError {
    Io(std::io::Error),
    /// The processor itself failed — surfaced with its id and message.
    Processor {
        id: String,
        message: String,
    },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "process io: {e}"),
            Self::Processor { id, message } => write!(f, "processor {id} failed: {message}"),
        }
    }
}
impl std::error::Error for ProcessError {}

pub struct ProcessOutcome {
    pub output: PathBuf,
    pub cache_hit: bool,
}

/// The cache path an input/processor pair resolves to (stable, inspectable).
pub fn cache_path(cache_dir: &Path, def: &ProcessorDef, source_bytes: &[u8]) -> PathBuf {
    let content = blake3::hash(source_bytes).to_hex();
    cache_dir.join(format!("{}-v{}-{}.bin", def.id, def.version, content))
}

/// Run one processor over one source, through the cache.
pub fn process_asset(
    source: &Path,
    bytes: &[u8],
    def: &ProcessorDef,
    cache_dir: &Path,
) -> Result<ProcessOutcome, ProcessError> {
    let output = cache_path(cache_dir, def, bytes);
    if output.exists() {
        return Ok(ProcessOutcome {
            output,
            cache_hit: true,
        });
    }
    // A processor is third-party code that runs at STARTUP (imports fire once
    // on boot). One panicking on one malformed asset would make the project
    // unopenable, with no way in to remove the asset — so a panic is caught and
    // reported exactly like a returned error.
    let produced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (def.process)(&ProcessCx { source, bytes })
    }))
    .map_err(|_| ProcessError::Processor {
        id: def.id.to_string(),
        message: "panicked".into(),
    })?
    .map_err(|message| ProcessError::Processor {
        id: def.id.to_string(),
        message,
    })?;
    std::fs::create_dir_all(cache_dir).map_err(ProcessError::Io)?;
    // Process-unique: two editors open on one project would otherwise rename
    // each other's half-written file into place.
    let tmp = output.with_extension(format!("{}.bin.tmp", std::process::id()));
    std::fs::write(&tmp, &produced).map_err(ProcessError::Io)?;
    std::fs::rename(&tmp, &output).map_err(ProcessError::Io)?;
    Ok(ProcessOutcome {
        output,
        cache_hit: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_api::prelude::ProcessorId;

    fn upper_processor(version: u32) -> ProcessorDef {
        ProcessorDef {
            id: ProcessorId::new_static("test.upper"),
            name: "Uppercase",
            version,
            extensions: &["txt"],
            process: |cx| Ok(cx.bytes.to_ascii_uppercase()),
        }
    }

    // D3: deterministic byte-same outputs, cache hits on repeat, invalidation
    // by content change AND by processor version bump.
    #[test]
    fn deterministic_cached_invalidated() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let source = Path::new("note.txt");
        let def = upper_processor(1);

        let first = process_asset(source, b"hello", &def, &cache).unwrap();
        assert!(!first.cache_hit);
        let bytes_one = std::fs::read(&first.output).unwrap();

        let second = process_asset(source, b"hello", &def, &cache).unwrap();
        assert!(second.cache_hit, "same input + version = cache hit");
        assert_eq!(second.output, first.output);
        assert_eq!(
            std::fs::read(&second.output).unwrap(),
            bytes_one,
            "byte-same"
        );

        let changed = process_asset(source, b"world", &def, &cache).unwrap();
        assert!(!changed.cache_hit, "content change misses");
        assert_ne!(changed.output, first.output);

        let bumped = upper_processor(2);
        let rebuilt = process_asset(source, b"hello", &bumped, &cache).unwrap();
        assert!(!rebuilt.cache_hit, "version bump invalidates");
        assert_ne!(rebuilt.output, first.output);
        assert_eq!(std::fs::read(&rebuilt.output).unwrap(), bytes_one);
    }
}
