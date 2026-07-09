//! Integration test: use polyplug_codegen library to generate Python bindings.

#![allow(clippy::expect_used)]

use polyplug_codegen::{GenerateConfig, Lang, Side};
use std::fs;
use std::path::Path;
use std::path::PathBuf;

mod cli_support;
use cli_support::cli_generate;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug_codegen")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn generate_python_bindings(api_toml: &Path, out_dir: &Path) {
    let config = GenerateConfig {
        api_toml: api_toml.to_path_buf(),
        lang: Lang::Python,
        side: Side::Host,
        out_dir: out_dir.to_path_buf(),
    };

    let output = cli_generate(&config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        fs::write(&file_path, &file.content).expect("failed to write generated file");
    }
}

#[test]
fn test_generate_python_files_exist() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_python");

    fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_python_bindings(&api_toml, &out_dir);

    let expected_files: &[&str] = &[
        "host/types.py",
        "host/types.pyi",
        "host/callers.py",
        "host/callers.pyi",
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
        "test_generate_python_files_exist: all {} files present ✓",
        expected_files.len()
    );
}

#[test]
fn test_python_codegen_generates_enum_types() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_python_enum");

    fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_python_bindings(&api_toml, &out_dir);

    let types_file: PathBuf = out_dir.join("host").join("types.py");
    let content: String = fs::read_to_string(&types_file).expect("read types file");

    assert!(
        content.contains("class PixelFormat(enum.IntEnum)"),
        "types.py must contain PixelFormat IntEnum: {}",
        types_file.display()
    );
    assert!(
        content.contains("class ImageFlags(enum.IntFlag)"),
        "types.py must contain ImageFlags IntFlag"
    );

    println!("test_python_codegen_generates_enum_types: all enum assertions passed ✓");
}
