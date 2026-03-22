//! CLI argument validation tests for `polyplugc`.
//!
//! Tests cover:
//!   - Missing `--api` / `--bundle` flag in `generate`, `validate`, and `pack` subcommands
//!   - Unknown / unsupported `--lang` value
//!   - Valid language aliases (`cpp`, `c++`, `csharp`, `c#`, `python`, `py`)
//!   - Conflicting flags (`--api` + `--bundle` together)
//!   - Non-existent paths for `--api` / `--bundle`
//!   - Missing required `--out` flag in `generate` and `pack`

#![allow(clippy::expect_used)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Workspace root resolved from `CARGO_MANIFEST_DIR` (`crates/polyplugc`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplugc")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Path to the canonical `test_api.toml` fixture.
fn test_api_toml() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_api.toml")
}

/// Path to the canonical `test_bundle.toml` fixture.
fn test_bundle_toml() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_bundle.toml")
}

/// Run `polyplugc` with the given arguments and return the full `Output`.
fn run_polyplugc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .args(args)
        .output()
        .expect("failed to spawn polyplugc")
}

/// Assert the process exited with a non-zero status and that stderr contains
/// the expected substring.
fn assert_failure_contains(output: &Output, needle: &str) {
    assert!(
        !output.status.success(),
        "expected failure but process succeeded.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr: String = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout: String = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        stderr.contains(needle) || stdout.contains(needle),
        "expected output to contain {:?}\nstdout: {}\nstderr: {}",
        needle,
        stdout,
        stderr,
    );
}

/// Assert the process exited successfully.
fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success but process failed.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── generate: missing --api / --bundle ───────────────────────────────────────

#[test]
fn generate_missing_api_and_bundle_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_generate_missing_api_bundle");
    let output: Output = run_polyplugc(&[
        "generate",
        "--lang",
        "rust",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    assert_failure_contains(&output, "Must specify --api or --bundle");
}

// ─── generate: invalid --lang ─────────────────────────────────────────────────

#[test]
fn generate_invalid_lang_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_generate_invalid_lang");
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "generate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--lang",
        "cobol",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    assert_failure_contains(&output, "Unknown language");
}

#[test]
fn generate_invalid_lang_empty_string_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_generate_invalid_lang_empty");
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "generate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--lang",
        "",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    assert_failure_contains(&output, "Unknown language");
}

// ─── generate: language aliases ───────────────────────────────────────────────

/// Table-driven alias test: each alias must be accepted by `parse_lang`.
/// We only verify that polyplugc does not fail with "Unknown language" —
/// subsequent codegen errors for a non-existent `--out` are fine.
fn assert_lang_alias_accepted(alias: &str) {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli_val_alias_{alias}"));
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "generate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--lang",
        alias,
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    // The alias must NOT produce an "Unknown language" error.
    let stderr: String = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout: String = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        !stderr.contains("Unknown language") && !stdout.contains("Unknown language"),
        "alias {:?} was rejected as unknown language\nstdout: {}\nstderr: {}",
        alias,
        stdout,
        stderr,
    );
}

#[test]
fn generate_lang_alias_cpp_accepted() {
    assert_lang_alias_accepted("cpp");
}

#[test]
fn generate_lang_alias_cpp_plus_accepted() {
    assert_lang_alias_accepted("c++");
}

#[test]
fn generate_lang_alias_csharp_accepted() {
    assert_lang_alias_accepted("csharp");
}

#[test]
fn generate_lang_alias_c_hash_accepted() {
    assert_lang_alias_accepted("c#");
}

#[test]
fn generate_lang_alias_python_accepted() {
    assert_lang_alias_accepted("python");
}

#[test]
fn generate_lang_alias_py_accepted() {
    assert_lang_alias_accepted("py");
}

#[test]
fn generate_lang_alias_lua_accepted() {
    assert_lang_alias_accepted("lua");
}

#[test]
fn generate_lang_alias_js_quickjs_accepted() {
    assert_lang_alias_accepted("js-quickjs");
}

// ─── generate: conflicting --api and --bundle ─────────────────────────────────

#[test]
fn generate_api_and_bundle_conflict_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_generate_conflict");
    let api_toml: PathBuf = test_api_toml();
    let bundle_toml: PathBuf = test_bundle_toml();
    let output: Output = run_polyplugc(&[
        "generate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--bundle",
        bundle_toml.to_str().expect("bundle_toml utf8"),
        "--lang",
        "rust",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    // clap enforces conflicts_with so this must fail.
    assert_failure_contains(&output, "cannot be used with");
}

// ─── generate: non-existent path ─────────────────────────────────────────────

#[test]
fn generate_nonexistent_api_path_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_generate_nonexistent_api");
    let output: Output = run_polyplugc(&[
        "generate",
        "--api",
        "/nonexistent/path/to/api.toml",
        "--lang",
        "rust",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    assert!(
        !output.status.success(),
        "expected failure for non-existent --api path\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── generate: missing --out ──────────────────────────────────────────────────

#[test]
fn generate_missing_out_fails() {
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "generate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--lang",
        "rust",
        // no --out
    ]);
    // clap's `required = true` on --out means this must fail.
    assert!(
        !output.status.success(),
        "expected failure when --out is omitted\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── validate: missing --api / --bundle ───────────────────────────────────────

#[test]
fn validate_missing_api_and_bundle_fails() {
    let output: Output = run_polyplugc(&["validate"]);
    assert_failure_contains(&output, "Must specify --api or --bundle");
}

// ─── validate: conflicting flags ──────────────────────────────────────────────

#[test]
fn validate_api_and_bundle_conflict_fails() {
    let api_toml: PathBuf = test_api_toml();
    let bundle_toml: PathBuf = test_bundle_toml();
    let output: Output = run_polyplugc(&[
        "validate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--bundle",
        bundle_toml.to_str().expect("bundle_toml utf8"),
    ]);
    assert_failure_contains(&output, "cannot be used with");
}

// ─── validate: non-existent path ─────────────────────────────────────────────

#[test]
fn validate_nonexistent_api_path_fails() {
    let output: Output = run_polyplugc(&["validate", "--api", "/nonexistent/path/to/api.toml"]);
    assert!(
        !output.status.success(),
        "expected failure for non-existent --api path\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── validate: valid api.toml ─────────────────────────────────────────────────

#[test]
fn validate_valid_api_toml_succeeds() {
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "validate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
    ]);
    assert_success(&output);
    let stdout: String = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.contains("OK"), "expected OK in stdout: {stdout}");
}

// ─── pack: missing --api / --bundle ───────────────────────────────────────────

#[test]
fn pack_missing_api_and_bundle_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_pack_missing_manifest");
    let output: Output = run_polyplugc(&[
        "pack",
        "--lang",
        "rust",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    assert_failure_contains(&output, "Must specify --api or --bundle");
}

// ─── pack: invalid --lang ─────────────────────────────────────────────────────

#[test]
fn pack_invalid_lang_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_pack_invalid_lang");
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "pack",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--lang",
        "fortran",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    assert_failure_contains(&output, "Unknown language");
}

// ─── pack: missing --out ──────────────────────────────────────────────────────

#[test]
fn pack_missing_out_fails() {
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "pack",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--lang",
        "rust",
        // no --out
    ]);
    assert!(
        !output.status.success(),
        "expected failure when --out is omitted\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── pack: non-existent path ──────────────────────────────────────────────────

#[test]
fn pack_nonexistent_api_path_fails() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_val_pack_nonexistent_api");
    let output: Output = run_polyplugc(&[
        "pack",
        "--api",
        "/nonexistent/path/to/api.toml",
        "--lang",
        "rust",
        "--out",
        out_dir.to_str().expect("out_dir utf8"),
    ]);
    assert!(
        !output.status.success(),
        "expected failure for non-existent --api path\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── unknown subcommand ───────────────────────────────────────────────────────

#[test]
fn unknown_subcommand_fails() {
    let output: Output = run_polyplugc(&["frobnicate"]);
    assert!(
        !output.status.success(),
        "expected failure for unknown subcommand\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── no arguments at all ──────────────────────────────────────────────────────

#[test]
fn no_arguments_fails() {
    let output: Output = run_polyplugc(&[]);
    assert!(
        !output.status.success(),
        "expected failure when no arguments given\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
