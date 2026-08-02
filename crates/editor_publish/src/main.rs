//! `editor publish` (M3-C9, spec §"minimal publish"): produce a runnable zip of
//! the game WITHOUT the editor — failing LOUDLY at the first gate. Single
//! profile, no gates beyond boot; the full pipeline rigor (profiles, cook steps,
//! platform matrices) is M4.
//!
//! Gates, in order:
//!   1. release build of `template_game` with NO editor feature
//!   2. stripping verification (A6): no editor crate in the dependency graph
//!   3. package: binary + level.ron + materials.ron -> target/publish/*.zip
//!   4. (--boot-check) launch the packaged binary briefly and require it to
//!      survive startup — skipped in headless CI

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("publish") => {
            let boot_check = args.iter().any(|a| a == "--boot-check");
            if let Err(message) = publish(boot_check) {
                eprintln!("\npublish FAILED: {message}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("usage: editor publish [--boot-check]");
            std::process::exit(2);
        }
    }
}

fn workspace_root() -> PathBuf {
    // This binary lives in target/<profile>/; the tool is always run via
    // `cargo run -p editor_publish`, so the manifest dir's grandparent is the root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn publish(boot_check: bool) -> Result<(), String> {
    let root = workspace_root();

    // ---- Gate 1: the editor-less release build ----------------------------
    println!("gate 1/4: release build (no editor feature)…");
    let status = Command::new("cargo")
        .current_dir(&root)
        .args(["build", "--release", "-p", "template_game"])
        .status()
        .map_err(|e| format!("cargo build failed to start: {e}"))?;
    if !status.success() {
        return Err("release build failed (gate 1)".into());
    }

    // ---- Gate 2: stripping verification (A6) ------------------------------
    println!("gate 2/4: editor stripping verification…");
    let tree = Command::new("cargo")
        .current_dir(&root)
        .args(["tree", "-p", "template_game", "--no-default-features", "-e", "normal"])
        .output()
        .map_err(|e| format!("cargo tree failed to start: {e}"))?;
    if !tree.status.success() {
        return Err("cargo tree failed (gate 2)".into());
    }
    let graph = String::from_utf8_lossy(&tree.stdout);
    for leak in ["editor_api", "editor_core", "editor_scene", "editor_ui"] {
        if graph.lines().any(|line| line.contains(leak)) {
            return Err(format!(
                "editor crate `{leak}` leaked into the editor-less dependency graph (gate 2 / A6)"
            ));
        }
    }

    // ---- Gate 3: package --------------------------------------------------
    println!("gate 3/4: packaging…");
    let binary = root.join("target/release/template_game");
    if !binary.exists() {
        return Err(format!("built binary missing at {}", binary.display()));
    }
    let out_dir = root.join("target/publish");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    // Nix dev shells may not carry `git`; try the system one before giving up.
    let sha = ["git", "/usr/bin/git"]
        .iter()
        .find_map(|git| {
            Command::new(git)
                .current_dir(&root)
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_else(|| "nogit".into());
    let artifact = out_dir.join(format!(
        "template_game-{}-{sha}.zip",
        env!("CARGO_PKG_VERSION")
    ));

    let file = std::fs::File::create(&artifact).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipWriter::new(file);
    let executable_options: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);
    let data_options: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

    archive
        .start_file("template_game", executable_options)
        .map_err(|e| e.to_string())?;
    archive
        .write_all(&std::fs::read(&binary).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for data in ["level.ron", "materials.ron"] {
        let path = root.join(data);
        if path.exists() {
            archive.start_file(data, data_options).map_err(|e| e.to_string())?;
            archive
                .write_all(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
    }
    archive.finish().map_err(|e| e.to_string())?;
    let size = std::fs::metadata(&artifact).map_err(|e| e.to_string())?.len();
    println!("packaged {} ({:.1} MiB)", artifact.display(), size as f64 / (1024.0 * 1024.0));

    // ---- Gate 4 (optional): boot ------------------------------------------
    if boot_check {
        println!("gate 4/4: boot check…");
        let stage = out_dir.join("boot-check");
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
        let file = std::fs::File::open(&artifact).map_err(|e| e.to_string())?;
        let mut unzip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        unzip.extract(&stage).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                stage.join("template_game"),
                std::fs::Permissions::from_mode(0o755),
            )
            .map_err(|e| e.to_string())?;
        }
        let mut child = Command::new(stage.join("template_game"))
            .current_dir(&stage)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("boot failed to start: {e}"))?;
        std::thread::sleep(std::time::Duration::from_secs(6));
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok().map(|_| buf)
                    })
                    .unwrap_or_default();
                return Err(format!(
                    "packaged binary exited during boot ({status}) — gate 4\n{stderr}"
                ));
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                println!("boot check passed (survived 6s)");
            }
        }
    } else {
        println!("gate 4/4: boot check skipped (pass --boot-check to run)");
    }

    println!("\npublish OK: {}", artifact.display());
    Ok(())
}
