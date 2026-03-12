//! Integration test: run the showcase host binary and verify its output.
//!
//! This test crate is the crate root for the `integration_showcase` test binary.
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

#[test]
fn showcase_runs_and_produces_expected_output() {
    // Build the showcase-host binary first.
    let build_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--manifest-path")
        .arg("showcase/host/Cargo.toml")
        .arg("--target-dir")
        .arg("target")
        .current_dir(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("crates parent")
                .parent()
                .expect("workspace root"),
        )
        .status()
        .expect("failed to build showcase-host");
    assert!(build_status.success(), "showcase-host build failed");

    // Run the showcase host.
    let workspace_root: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates parent")
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let output: Output = Command::new(workspace_root.join("target/debug/showcase-host"))
        .current_dir(&workspace_root)
        .output()
        .expect("failed to run showcase-host");

    let stdout: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stdout);
    let stderr: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stderr);
    let combined: String = format!("{}{}", stdout, stderr);

    assert!(
        output.status.success(),
        "showcase-host exited with non-zero status: {}\nstdout: {}\nstderr: {}",
        output.status,
        stdout,
        stderr,
    );

    // Verify key output strings from both pipeline runs.
    assert!(
        combined.contains("=== polyplug showcase ==="),
        "missing showcase header; output:\n{combined}"
    );
    assert!(
        combined.contains("--- Run 1: C++ uppercase transformer ---"),
        "missing run 1 header; output:\n{combined}"
    );
    assert!(
        combined.contains("ALICE") || combined.contains("HELLO"),
        "missing uppercase output; output:\n{combined}"
    );
    assert!(
        combined.contains("--- Run 2: Lua reverse transformer ---"),
        "missing run 2 header; output:\n{combined}"
    );
    assert!(
        combined.contains("--- Error scenario: malformed input ---"),
        "missing error scenario; output:\n{combined}"
    );
    assert!(
        combined.contains("malformed CSV"),
        "missing malformed CSV error message; output:\n{combined}"
    );
    assert!(
        combined.contains("=== showcase complete ==="),
        "missing showcase footer; output:\n{combined}"
    );
}
