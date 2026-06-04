//! Integration tests for Deno host library (FFI bindings).
//!
//! Tests that the Deno FFI bindings in sdks/js/host/ work correctly
//! with the polyplug native library.

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

const POLYPLUG_SO: &str = env!("POLYPLUG_SO");
const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");

fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_deno_host_lib_integration() {
    if !deno_available() {
        eprintln!("[SKIP] deno not found on PATH");
        return;
    }

    if POLYPLUG_SO.is_empty() || !PathBuf::from(POLYPLUG_SO).exists() {
        panic!(
            "polyplug shared library not found at {POLYPLUG_SO:?} - it must be built before this test. Run: cargo build -p polyplug"
        );
    }

    let manifest_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("parent of tests/integration")
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let fixture_ts: PathBuf = workspace_root
        .join("tests")
        .join("fixtures")
        .join("deno_host_test.ts");

    let output: std::process::Output = Command::new("deno")
        .arg("run")
        .arg("--allow-ffi")
        .arg("--allow-env")
        .arg("--allow-read")
        .arg(&fixture_ts)
        .env("POLYPLUG_SO", POLYPLUG_SO)
        .env("TEST_PLUGIN_DIR", TEST_PLUGIN_DIR)
        .output()
        .expect("failed to spawn deno");

    let stdout: &str = core::str::from_utf8(&output.stdout).unwrap_or("");
    let stderr: &str = core::str::from_utf8(&output.stderr).unwrap_or("");

    assert!(
        output.status.success(),
        "deno test exited with non-zero status.\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr,
    );
}
