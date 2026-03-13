//! Integration test: use polyplug_codegen library to generate JS/Deno bindings.

#![allow(clippy::expect_used)]

use polyplug_codegen::{GenerateConfig, Lang, Side, generate};
use std::path::Path;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug_codegen")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn generate_js_deno_bindings(api_toml: &Path, out_dir: &Path) {
    let config = GenerateConfig {
        api_toml: api_toml.to_path_buf(),
        lang: Lang::JsDeno,
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
}

#[test]
fn test_generate_js_deno_files_exist() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_js_deno");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_js_deno_bindings(&api_toml, &out_dir);

    let expected_files: &[&str] = &["host/types.ts", "host/callers.ts"];

    for rel_path in expected_files {
        let full_path: PathBuf = out_dir.join(rel_path);
        assert!(
            full_path.exists(),
            "Expected file not found: {}",
            full_path.display()
        );
    }

    println!(
        "test_generate_js_deno_files_exist: all {} files present ✓",
        expected_files.len()
    );
}
