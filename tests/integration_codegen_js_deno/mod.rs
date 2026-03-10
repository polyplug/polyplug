//! Integration test: run polyplugc to generate js-deno bindings and assert all expected
//! files are present.
//!
//! This test crate is the crate root for the `integration_codegen_js_deno` test binary.
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

/// Run `polyplugc generate --bundle <bundle_toml> --lang js-deno --out <out_dir>`.
/// Returns the `Output` for inspection.
fn run_polyplugc_js_deno(bundle_toml: &Path, out_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .arg("generate")
        .arg("--bundle")
        .arg(bundle_toml)
        .arg("--lang")
        .arg("js-deno")
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

// ─── Codegen file existence check (always runs) ──────────────────────────────

#[test]
fn test_generate_js_deno_files_exist() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_js_deno");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // ── 1. Run polyplugc to generate js-deno bindings ─────────────────────────
    let output: Output = run_polyplugc_js_deno(&bundle_toml, &out_dir);
    assert!(
        output.status.success(),
        "polyplugc generate --lang js-deno failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── 2. Assert all expected files exist ────────────────────────────────────
    let expected_files: &[&str] = &[
        "guest/types.ts",
        "guest/contracts.ts",
        "guest/init.ts",
        "manifest.toml",
        "README.md",
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
        "test_generate_js_deno_files_exist: all {} files present in {} ✓",
        expected_files.len(),
        out_dir.display()
    );
}
