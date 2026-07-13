//! Integration test: use polyplug_codegen library to generate JS/QuickJS bindings.

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

fn generate_js_quickjs_bindings(api_toml: &Path, out_dir: &Path) {
    let config = GenerateConfig {
        api_toml: api_toml.to_path_buf(),
        lang: Lang::JsQuickJs,
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
fn test_generate_js_quickjs_files_exist() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_js_quickjs");

    fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_js_quickjs_bindings(&api_toml, &out_dir);

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
        "test_generate_js_quickjs_files_exist: all {} files present ✓",
        expected_files.len()
    );
}

/// Regression: the JS host caller's function-index guard must be VM-safe.
///
/// A VM-dispatch interface reports `functionCount() == 0` (the VM routes by
/// `fn_id` itself), so a bare `fn_id >= functionCount()` guard rejects *every*
/// call into a Lua/JS/Python guest. The guard must only fire when the interface
/// reports a real (native) count, i.e. it must be gated on `functionCount() > 0`.
#[test]
fn test_js_host_caller_function_count_guard_is_vm_safe() {
    let root: PathBuf = workspace_root();
    // examples/api.toml has string→string contract methods (e.g. decode) that
    // emit the function-index guard; the minimal test fixture does not.
    let api_toml: PathBuf = root.join("examples").join("api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("js_quickjs_vm_safe_guard");

    fs::create_dir_all(&out_dir).expect("create out_dir");
    generate_js_quickjs_bindings(&api_toml, &out_dir);

    let callers: String = fs::read_to_string(out_dir.join("host/callers.ts"))
        .expect("read generated host/callers.ts");

    // The caller must emit at least one function-index guard.
    assert!(
        callers.contains(">= view.functionCount()"),
        "expected a function-index guard in the generated JS host caller"
    );

    // Every guard line must be gated on a positive (native) count, so VM
    // interfaces (count 0) are never wrongly rejected.
    for line in callers.lines() {
        if line.contains(">= view.functionCount()") {
            assert!(
                line.contains("view.functionCount() > 0 &&"),
                "JS host caller guard is not VM-safe (rejects VM guests): {}",
                line.trim()
            );
        }
    }

    println!("test_js_host_caller_function_count_guard_is_vm_safe: guard is VM-safe ✓");
}
