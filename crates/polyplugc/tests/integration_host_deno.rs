#![allow(clippy::expect_used)]

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
        println!("[SKIP] deno not found on PATH — skipping integration_host_deno tests");
        return;
    }

    let manifest_dir: std::path::PathBuf = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root: std::path::PathBuf = manifest_dir
        .parent()
        .expect("parent of crates/polyplug")
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let fixture_ts: std::path::PathBuf = workspace_root
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
