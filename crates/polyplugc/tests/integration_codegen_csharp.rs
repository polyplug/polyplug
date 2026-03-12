//! Integration test: run polyplugc to generate C# bindings and assert all expected
//! files are present, optionally compile with dotnet build.
//!
//! This test crate is the crate root for the `integration_codegen_csharp` test binary.
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

/// Run `polyplugc generate --bundle <bundle_toml> --lang csharp --out <out_dir>`.
/// Returns the `Output` for inspection.
fn run_polyplugc_csharp(bundle_toml: &Path, out_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .arg("generate")
        .arg("--bundle")
        .arg(bundle_toml)
        .arg("--lang")
        .arg("csharp")
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

// ─── Codegen file existence check (always runs) ──────────────────────────────

#[test]
fn test_generate_csharp_files_exist() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_csharp");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // ── 1. Run polyplugc to generate C# bindings ──────────────────────────────
    let output: Output = run_polyplugc_csharp(&bundle_toml, &out_dir);
    assert!(
        output.status.success(),
        "polyplugc generate --lang csharp failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── 2. Assert all expected files exist ────────────────────────────────────
    let expected_files: &[&str] = &[
        "guest/Types.cs",
        "guest/Contracts.cs",
        "guest/Vtables.cs",
        "guest/Init.cs",
        "guest/BundleConstants.cs",
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
        "test_generate_csharp_files_exist: all {} files present in {} ✓",
        expected_files.len(),
        out_dir.display()
    );

    // ── 3. Attempt dotnet build (skip if dotnet not found or no .csproj exists) ──
    let dotnet_version_result: std::io::Result<std::process::Output> =
        Command::new("dotnet").args(["--version"]).output();

    if let Ok(version_out) = dotnet_version_result {
        if version_out.status.success() {
            let guest_dir: PathBuf = out_dir.join("guest");
            let has_csproj: bool = std::fs::read_dir(&guest_dir)
                .map(|mut rd| {
                    rd.any(|e| {
                        e.ok()
                            .and_then(|de| de.path().extension().map(|ext| ext == "csproj"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            if has_csproj {
                let build_result: std::process::Output = Command::new("dotnet")
                    .arg("build")
                    .arg(&guest_dir)
                    .output()
                    .expect("dotnet build failed to run");

                assert!(
                    build_result.status.success(),
                    "dotnet build failed:\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&build_result.stdout),
                    String::from_utf8_lossy(&build_result.stderr),
                );

                println!("test_generate_csharp_files_exist: dotnet build succeeded ✓");
            } else {
                eprintln!("skipping dotnet build check: no .csproj found in guest/");
            }
        } else {
            eprintln!("skipping dotnet build check: dotnet --version returned non-zero");
        }
    } else {
        eprintln!("skipping dotnet build check: dotnet not found");
    }
}

// ─── Enum types codegen test ─────────────────────────────────────────────────

#[test]
fn test_csharp_codegen_generates_enum_types() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_csharp_enum");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // ── 2. Run polyplugc to generate C# bindings ───────────────────────────────
    let output: Output = run_polyplugc_csharp(&bundle_toml, &out_dir);
    assert!(
        output.status.success(),
        "polyplugc generate --lang csharp failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── 3. Read guest/Types.cs and assert enum content ─────────────────────────
    let types_file: PathBuf = out_dir.join("guest").join("Types.cs");
    let content: String = std::fs::read_to_string(&types_file).expect("read types file");

    assert!(
        content.contains("public enum PixelFormat"),
        "Types.cs must contain public enum PixelFormat"
    );
    assert!(content.contains("[Flags]"), "Types.cs must contain [Flags]");

    println!("test_csharp_codegen_generates_enum_types: all enum assertions passed ✓");
}
