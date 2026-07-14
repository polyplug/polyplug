//! Shared test helper: drive the `polyplugc` binary and reconstruct a
//! `GenerateOutput` from the files it writes.
//!
//! `polyplugc` is a bin-only CLI (CLAUDE.md Rule 21) — it exports no library, so
//! tests exercise it exactly the way a real consumer does: by running the
//! compiled binary. `cli_generate` runs `polyplugc generate …`, then reads the
//! emitted files back into the same `GenerateOutput` shape the old in-process
//! `generate()` returned, so a test body that iterates `output.files` is
//! unchanged.

#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use polyplug_codegen::{GenerateConfig, GenerateOutput, GeneratedFile, OutputPartition, Side};

/// Locate the compiled `polyplugc` binary.
///
/// In `polyplugc`'s own test crate Cargo sets `CARGO_BIN_EXE_polyplugc`. For an
/// external test crate (e.g. `integration`) it is not set, so fall back to the
/// binary that sits next to the test executable in the target dir, building it
/// on demand if a lone `cargo test -p <crate>` has not produced it yet.
pub fn polyplugc_bin() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_polyplugc") {
        return PathBuf::from(p);
    }
    let mut dir: PathBuf = env::current_exe().expect("current_exe for test binary");
    dir.pop(); // strip test binary file name -> .../deps
    if dir.ends_with("deps") {
        dir.pop(); // -> .../<profile>
    }
    let mut bin: PathBuf = dir.join("polyplugc");
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    if !bin.exists() {
        let _ = Command::new(env!("CARGO"))
            .args(["build", "-p", "polyplugc"])
            .status();
    }
    bin
}

/// Run `polyplugc generate` for `config`, writing to `out_dir`, and return its
/// emitted files as a `GenerateOutput` or a diagnostic when generation fails.
///
/// The returned paths are relative to `out_dir` (matching the old in-process
/// output), and the reconstructed output preserves the configuration's immutable
/// language and layout metadata.
pub fn cli_generate(config: &GenerateConfig, out_dir: &Path) -> Result<GenerateOutput, String> {
    fs::create_dir_all(out_dir).map_err(|e| format!("create output dir failed: {e}"))?;

    let flag: &str = match config.side {
        Side::Host => "--api",
        Side::Guest => "--bundle",
    };
    let output: Output = Command::new(polyplugc_bin())
        .arg("generate")
        .arg(flag)
        .arg(&config.api_toml)
        .arg("--lang")
        .arg(config.lang.as_str())
        .arg("--out")
        .arg(out_dir)
        .output()
        .map_err(|e| format!("failed to spawn polyplugc: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "polyplugc generate failed ({}):\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }

    let mut files: Vec<GeneratedFile> = Vec::new();
    collect_files(out_dir, out_dir, &mut files);
    Ok(GenerateOutput::from_files(
        config.lang,
        config.layout.clone(),
        files,
    ))
}

/// Recursively collect every file under `dir` into `files`, with paths relative
/// to `root`.
fn collect_files(root: &Path, dir: &Path, files: &mut Vec<GeneratedFile>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else if let Ok(rel) = path.strip_prefix(root) {
            let content: String = fs::read_to_string(&path).unwrap_or_default();
            files.push(GeneratedFile {
                path: rel.to_path_buf(),
                content,
                force_regenerate: false,
                partition: OutputPartition::Bindings,
                references: Vec::new(),
            });
        }
    }
}
