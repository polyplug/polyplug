//! Integration test: run polyplugc to generate Lua bindings and assert all expected
//! files are present, optionally run a syntax check with luajit.
//!
//! This test crate is the crate root for the `integration_codegen_lua` test binary.
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

/// Run `polyplugc generate --bundle <bundle_toml> --lang lua --out <out_dir>`.
/// Returns the `Output` for inspection.
fn run_polyplugc_lua(bundle_toml: &Path, out_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .arg("generate")
        .arg("--bundle")
        .arg(bundle_toml)
        .arg("--lang")
        .arg("lua")
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

// ─── Codegen file existence check (always runs) ──────────────────────────────

#[test]
fn test_generate_lua_files_exist() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_lua");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // ── 1. Run polyplugc to generate Lua bindings ──────────────────────────────
    let output: Output = run_polyplugc_lua(&bundle_toml, &out_dir);
    assert!(
        output.status.success(),
        "polyplugc generate --lang lua failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── 2. Assert all expected files exist ────────────────────────────────────
    let expected_files: &[&str] = &[
        "guest/types.lua",
        "guest/contracts.lua",
        "guest/init.lua",
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
        "test_generate_lua_files_exist: all {} files present in {} ✓",
        expected_files.len(),
        out_dir.display()
    );

    // ── 3. Attempt luajit syntax check (skip if luajit not found) ─────────────
    let luajit_version_result: std::io::Result<std::process::Output> =
        Command::new("luajit").args(["--version"]).output();

    if let Ok(version_out) = luajit_version_result {
        if version_out.status.success() {
            let types_lua: PathBuf = out_dir.join("guest").join("types.lua");
            let syntax_result: std::process::Output = Command::new("luajit")
                .arg("-b")
                .arg(&types_lua)
                .arg("/dev/null")
                .output()
                .expect("luajit failed to run");

            assert!(
                syntax_result.status.success(),
                "luajit syntax check failed for types.lua:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&syntax_result.stdout),
                String::from_utf8_lossy(&syntax_result.stderr),
            );

            println!("test_generate_lua_files_exist: luajit syntax check passed ✓");
        } else {
            eprintln!("skipping luajit syntax check: luajit --version returned non-zero");
        }
    } else {
        eprintln!("skipping luajit syntax check: luajit not found");
    }
}

// ─── Enum types codegen test ─────────────────────────────────────────────────

#[test]
fn test_lua_codegen_generates_enum_types() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_lua_enum");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // ── 2. Run polyplugc to generate Lua bindings ───────────────────────────────
    let output: Output = run_polyplugc_lua(&bundle_toml, &out_dir);
    assert!(
        output.status.success(),
        "polyplugc generate --lang lua failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── 3. Read guest/types.lua and assert enum content ─────────────────────────
    let types_file: PathBuf = out_dir.join("guest").join("types.lua");
    let content: String = std::fs::read_to_string(&types_file).expect("read types file");

    assert!(
        content.contains("local PixelFormat = {"),
        "types.lua must contain local PixelFormat = {{"
    );
    assert!(
        content.contains("bit.lshift"),
        "types.lua must contain bit.lshift"
    );

    println!("test_lua_codegen_generates_enum_types: all enum assertions passed ✓");
}
