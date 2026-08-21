//! Architectural fitness (spec §8 guardrail 4).
//!
//! The guardrails in §8 were enforced by hand for a long time, which means they
//! were enforced whenever somebody remembered. These are the ones a machine can
//! check, written as TESTS rather than CI-only greps so they run on every
//! `cargo test` and fail where the work is happening.
//!
//! Each rule below cost something to learn. They are not style preferences.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // tests/ -> editor_api -> crates -> workspace
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("the workspace root is two levels above this crate")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn editor_crates(root: &Path) -> Vec<PathBuf> {
    let mut crates = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        return crates;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("editor_"))
        {
            crates.push(path);
        }
    }
    crates
}

/// Bindings are DATA (spec §8, "no side door"). A key read straight from
/// `ButtonInput` is a binding nobody can remap, that which-key cannot show and a
/// macro cannot replay. Modifier reads (shift-click, the held-key fly camera)
/// are a different thing and stay allowed; `just_pressed` is the one that means
/// "somebody bound a key here".
#[test]
fn no_editor_crate_binds_a_key_outside_the_resolver() {
    let root = workspace_root();
    let mut offenders = Vec::new();
    for krate in editor_crates(&root) {
        let mut files = Vec::new();
        rust_files(&krate.join("src"), &mut files);
        for file in files {
            if file.ends_with("resolver.rs") {
                continue; // the one place bindings are resolved
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for (number, line) in text.lines().enumerate() {
                if line.contains("just_pressed(KeyCode") && !line.trim_start().starts_with("//") {
                    offenders.push(format!("{}:{}", file.display(), number + 1));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "keys are bound through ActionDef, not read directly: {offenders:#?}"
    );
}

/// `game_framework` never depends on an editor crate (spec §2). A game that
/// needs the editor to run is not a game with an editor, it is an editor.
#[test]
fn the_game_framework_owes_the_editor_nothing() {
    let manifest = workspace_root().join("crates/game_framework/Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("game_framework has a manifest");
    let offenders: Vec<&str> = text
        .lines()
        .filter(|line| line.trim_start().starts_with("editor_"))
        .collect();
    assert!(
        offenders.is_empty(),
        "game_framework must not depend on the editor: {offenders:#?}"
    );
}

/// And the reverse: an editor crate that knows about a specific game — or about
/// the game framework — is an editor for exactly one game. The probe that
/// verifies a game reacted has to do it by reflection for this reason.
#[test]
fn no_editor_crate_depends_on_a_game() {
    let root = workspace_root();
    let mut offenders = Vec::new();
    for krate in editor_crates(&root) {
        let manifest = krate.join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim_start();
            if line.starts_with("template_game") || line.starts_with("game_framework") {
                offenders.push(format!("{}: {line}", manifest.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the editor must not depend on a game: {offenders:#?}"
    );
}

/// The editor is a compile-time opt-in, never a release optimisation (spec §1):
/// with the feature off, zero editor code exists in the artifact. That only
/// holds while every editor dependency is optional.
#[test]
fn the_reference_game_keeps_the_editor_optional() {
    let manifest = workspace_root().join("crates/template_game/Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("template_game has a manifest");
    let offenders: Vec<&str> = text
        .lines()
        .filter(|line| line.trim_start().starts_with("editor_"))
        .filter(|line| !line.contains("optional = true"))
        .collect();
    assert!(
        offenders.is_empty(),
        "every editor dependency of a game is optional: {offenders:#?}"
    );
}

/// Probes dispatch on a frame number. Two arms with the SAME number is not a
/// compile error — the second is simply unreachable — so a check can stop
/// running while the suite stays green. That happened: a rename check went
/// unnoticed for several commits behind a duplicated arm.
#[test]
fn no_probe_has_two_arms_for_the_same_frame() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root.join("crates/editor_ui/src"), &mut files);
    let mut offenders = Vec::new();
    for file in files {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        if !name.starts_with("probe_") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut seen: Vec<u32> = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim_start();
            // `1234 => ...`, but not `1234..=1250 =>` which is a range arm.
            let Some((head, _)) = trimmed.split_once("=>") else {
                continue;
            };
            let head = head.trim();
            let Ok(frame) = head.parse::<u32>() else {
                continue;
            };
            if seen.contains(&frame) {
                offenders.push(format!("{name}: frame {frame} twice"));
            }
            seen.push(frame);
        }
    }
    assert!(
        offenders.is_empty(),
        "a shadowed probe arm silently stops testing something: {offenders:#?}"
    );
}

/// `Visibility` belongs to the EDITOR, not to the level (spec §9,
/// `editor_core::hide`).
///
/// Hiding is a view on the level, not part of it: it has one writer, it lifts
/// whenever the editor is inactive, and it must never reach `level.ron` — or a
/// game ships missing its floor because someone hid it on a Friday. Registering
/// the type as an editor component would quietly put it in the save set, and it
/// is exactly the sort of registration that looks harmless in a diff.
///
/// Every workspace crate, not just `editor_*`: a GAME registers components too,
/// including bevy's own types.
#[test]
fn nothing_registers_visibility_as_a_saved_component() {
    let root = workspace_root();
    let mut offenders = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        panic!("no crates directory");
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&path.join("src"), &mut files);
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for (number, line) in text.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for banned in [
                    "component::<Visibility>",
                    "component::<InheritedVisibility>",
                    "component::<ViewVisibility>",
                ] {
                    if line.contains(banned) {
                        offenders.push(format!("{}:{}", file.display(), number + 1));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "visibility is editor-owned and must never be saved: {offenders:#?}"
    );
}
