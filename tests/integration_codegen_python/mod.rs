//! Integration test: run polyplugc to generate Python bindings and assert all expected
//! files are present.
//!
//! This test crate is the crate root for the `integration_codegen_python` test binary.
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Workspace root resolved from `CARGO_MANIFEST_DIR` (`crates/polyplug`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Run `polyplugc generate --bundle <bundle_toml> --lang python --out <out_dir>`.
/// Returns the `Output` for inspection.
fn run_polyplugc_python(bundle_toml: &Path, out_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .arg("generate")
        .arg("--bundle")
        .arg(bundle_toml)
        .arg("--lang")
        .arg("python")
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

// ─── Codegen file existence check (always runs) ──────────────────────────────

#[test]
fn test_generate_python_files_exist() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_python");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // ── 1. Run polyplugc to generate Python bindings ───────────────────────────
    let output: Output = run_polyplugc_python(&bundle_toml, &out_dir);
    assert!(
        output.status.success(),
        "polyplugc generate --lang python failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── 2. Assert all expected files exist ────────────────────────────────────
    let expected_files: &[&str] = &[
        "guest/types.py",
        "guest/types.pyi",
        "guest/contracts.py",
        "guest/contracts.pyi",
        "guest/init.py",
        "manifest.toml",
    ];
    for file in expected_files {
        let path: PathBuf = out_dir.join(file);
        assert!(
            path.exists(),
            "expected generated file not found: {}",
            path.display()
        );
    }

    println!(
        "test_generate_python_files_exist: all {} files present in {} ✓",
        expected_files.len(),
        out_dir.display()
    );

    // ── 3. Check python3 availability (skip actual import check, just note it) ─
    let python_version_result: std::io::Result<std::process::Output> =
        Command::new("python3").args(["--version"]).output();

    if let Ok(version_out) = python_version_result {
        if version_out.status.success() {
            println!("python available, skipping import check");
        } else {
            eprintln!("skipping python check: python3 --version returned non-zero");
        }
    } else {
        eprintln!("skipping python check: python3 not found");
    }
}
