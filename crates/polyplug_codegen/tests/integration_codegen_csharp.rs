//! Integration test: use polyplug_codegen library to generate C# bindings
//! and assert all expected files are present.

#![allow(clippy::expect_used)]

use polyplug_codegen::{generate, GenerateConfig, Lang, Side};
use std::path::Path;
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Workspace root resolved from `CARGO_MANIFEST_DIR` (`crates/polyplug_codegen`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug_codegen")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Use polyplug_codegen::generate() to generate C# bindings.
fn generate_csharp_bindings(bundle_toml: &Path, out_dir: &Path) {
    let config = GenerateConfig {
        api_toml: bundle_toml.to_path_buf(),
        lang: Lang::CSharp,
        side: Side::Guest,
        out_dir: out_dir.to_path_buf(),
    };

    let output = generate(config).expect("polyplug_codegen::generate failed");

    // Write generated files to disk
    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }
}

// ─── Codegen file existence check (always runs) ──────────────────────────────

#[test]
fn test_generate_csharp_files_exist() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_csharp");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // Generate C# bindings using library API
    generate_csharp_bindings(&bundle_toml, &out_dir);

    // Assert expected files exist
    let expected_files: &[&str] = &[
        "guest/Types.cs",
        "guest/Contracts.cs",
        "guest/Vtables.cs",
        "guest/Init.cs",
        "guest/BundleConstants.cs",
        "manifest.toml",
    ];

    for rel_path in expected_files {
        let full_path: PathBuf = out_dir.join(rel_path);
        assert!(
            full_path.exists(),
            "Expected file not found: {}",
            full_path.display()
        );
    }

    println!(
        "test_generate_csharp_files_exist: all {} files present ✓",
        expected_files.len()
    );
}

#[test]
fn test_csharp_codegen_generates_enum_types() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_csharp_enum");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // Generate C# bindings using library API (host side for api.toml)
    let config = GenerateConfig {
        api_toml: api_toml.to_path_buf(),
        lang: Lang::CSharp,
        side: Side::Host,
        out_dir: out_dir.to_path_buf(),
    };

    let output = generate(config).expect("polyplug_codegen::generate failed");

    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    // Read host/Types.cs and assert enum content
    let types_file: PathBuf = out_dir.join("host").join("Types.cs");
    let content: String = std::fs::read_to_string(&types_file).expect("read types file");

    assert!(
        content.contains("public enum PixelFormat : uint"),
        "Types.cs must contain PixelFormat enum: {}",
        types_file.display()
    );
    assert!(
        content.contains("[Flags]"),
        "Types.cs must contain [Flags] attribute for bitflags"
    );
    assert!(
        content.contains("public enum ImageFlags : uint"),
        "Types.cs must contain ImageFlags enum"
    );

    println!("test_csharp_codegen_generates_enum_types: all enum assertions passed ✓");
}
