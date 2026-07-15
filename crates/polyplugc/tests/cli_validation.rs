//! CLI argument validation tests for `polyplugc`.
//!
//! Tests cover:
//!   - Missing `--api` / `--bundle` flag in `generate` and `validate` subcommands
//!   - Unknown / unsupported `--lang` value
//!   - Valid language aliases (`cpp`, `c++`, `csharp`, `c#`, `python`, `py`)
//!   - Conflicting flags (`--api` + `--bundle`, `--api` + `--bundle-dir`)
//!   - Non-existent paths for `--api` / `--bundle` / `--bundle-dir`
//!   - Missing required `--out` flag in `generate`

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use tempfile::tempdir;

#[derive(Clone, Copy)]
struct LanguageLayoutCase {
    lang: &'static str,
    domain_import: &'static str,
    guest_contracts_import: &'static str,
}

const LANGUAGE_LAYOUT_CASES: [LanguageLayoutCase; 6] = [
    LanguageLayoutCase {
        lang: "rust",
        domain_import: "shared::domain",
        guest_contracts_import: "shared::guest_contracts",
    },
    LanguageLayoutCase {
        lang: "cpp",
        domain_import: "guest/domain.hpp",
        guest_contracts_import: "guest/guest_contracts.hpp",
    },
    LanguageLayoutCase {
        lang: "csharp",
        domain_import: "Shared.Domain",
        guest_contracts_import: "Shared.GuestContracts",
    },
    LanguageLayoutCase {
        lang: "python",
        domain_import: "shared.domain",
        guest_contracts_import: "shared.guest_contracts",
    },
    LanguageLayoutCase {
        lang: "lua",
        domain_import: "shared.domain",
        guest_contracts_import: "shared.guest_contracts",
    },
    LanguageLayoutCase {
        lang: "js-quickjs",
        domain_import: "@test/javascript-domain",
        guest_contracts_import: "@test/javascript-contracts",
    },
];

struct SplitGenerationRequest<'a> {
    manifest_flag: &'a str,
    manifest_path: &'a Path,
    internal: bool,
    language: LanguageLayoutCase,
    bindings_root: &'a Path,
    domain_root: &'a Path,
    guest_contracts_root: &'a Path,
}

fn run_polyplugc_owned(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .args(args)
        .output()
        .expect("failed to spawn polyplugc")
}

fn split_generation_args(request: &SplitGenerationRequest<'_>) -> Vec<String> {
    let mut args = vec![
        "generate".to_owned(),
        request.manifest_flag.to_owned(),
        request.manifest_path.display().to_string(),
    ];
    if request.internal {
        args.push("--internal".to_owned());
    }
    args.extend([
        "--lang".to_owned(),
        request.language.lang.to_owned(),
        "--out".to_owned(),
        request.bindings_root.display().to_string(),
        "--domain-types-out".to_owned(),
        request.domain_root.display().to_string(),
        "--domain-types-import".to_owned(),
        request.language.domain_import.to_owned(),
        "--guest-contracts-out".to_owned(),
        request.guest_contracts_root.display().to_string(),
        "--guest-contracts-import".to_owned(),
        request.language.guest_contracts_import.to_owned(),
    ]);
    args
}

fn directory_has_file(root: &Path) -> bool {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            path.is_file() || (path.is_dir() && directory_has_file(&path))
        })
}

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

#[test]
fn generate_internal_rust_profile_uses_bundle_namespace_without_artifact_fields() {
    let temp = tempdir().expect("create temporary directory");
    let api_path = temp.path().join("api.toml");
    let bundle_path = temp.path().join("bundle.toml");
    let out_dir = temp.path().join("out");
    fs::write(
        &api_path,
        "[[guest_contract]]\nname = \"cli.profile\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write API");
    fs::write(
        &bundle_path,
        "[bundle]\nname = \"cli_internal_bundle\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"cli_provider\"\nimplements = [\"cli.profile@1.0\"]\n",
    )
    .expect("write artifactless bundle");

    let output = run_polyplugc(&[
        "generate",
        "--bundle",
        bundle_path.to_str().expect("bundle path utf8"),
        "--internal",
        "--lang",
        "rust",
        "--out",
        out_dir.to_str().expect("output path utf8"),
    ]);
    assert_success(&output);
    assert!(
        out_dir
            .join("internal/cli_internal_bundle-d1f3f480817f82c8/guest/init.rs")
            .is_file(),
        "internal CLI profile must emit guest provider bindings in its namespace"
    );
    assert!(
        out_dir
            .join("internal/cli_internal_bundle-d1f3f480817f82c8/host/host_callers.rs")
            .is_file(),
        "internal CLI profile must emit matching host caller bindings in its namespace"
    );
}

#[test]
fn generate_internal_rust_profile_partitions_declarations_by_cli_layout() {
    let temp = tempdir().expect("create temporary directory");
    let api_path = temp.path().join("api.toml");
    let bundle_path = temp.path().join("bundle.toml");
    let bindings = temp.path().join("bindings");
    let domain = temp.path().join("domain");
    let contracts = temp.path().join("contracts");
    fs::write(
        &api_path,
        "[[types]]\nname = \"State\"\nfields = [{ name = \"value\", type = \"u32\" }]\n\n[[guest_contract]]\nname = \"cli.profile\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"State\"\n",
    )
    .expect("write API");
    fs::write(
        &bundle_path,
        "[bundle]\nname = \"cli_internal_bundle\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"cli_provider\"\nimplements = [\"cli.profile@1.0\"]\n",
    )
    .expect("write artifactless bundle");

    let output = run_polyplugc(&[
        "generate",
        "--bundle",
        bundle_path.to_str().expect("bundle path utf8"),
        "--internal",
        "--lang",
        "rust",
        "--out",
        bindings.to_str().expect("binding path utf8"),
        "--domain-types-out",
        domain.to_str().expect("domain path utf8"),
        "--domain-types-import",
        "common::domain",
        "--guest-contracts-out",
        contracts.to_str().expect("contracts path utf8"),
        "--guest-contracts-import",
        "common::contracts",
    ]);
    assert_success(&output);
    let namespace = "internal/cli_internal_bundle-d1f3f480817f82c8/guest";
    assert!(domain.join(namespace).join("domain.rs").is_file());
    let contracts_file = contracts.join(namespace).join("guest_contracts.rs");
    assert!(contracts_file.is_file());
    let contracts_source = fs::read_to_string(contracts_file).expect("read contract declarations");
    assert!(
        contracts_source.contains("fn value(&self) -> Result<common::domain::State, GuestError>;")
    );
    assert!(!contracts_source.contains("use common::domain::*;"));
    let root_file = bindings.join(namespace).join("mod.rs");
    let root_source = fs::read_to_string(root_file).expect("read guest root");
    assert!(root_source.contains("pub use common::domain;"));
    assert!(root_source.contains("pub use common::contracts as guest_contracts;"));
    let interfaces = bindings.join(namespace).join("interfaces.rs");
    assert!(interfaces.is_file());
    assert!(!bindings.join(namespace).join("domain.rs").exists());
    assert!(!bindings.join(namespace).join("guest_contracts.rs").exists());
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
    assert_failure_contains(&output, "Must specify --api, --bundle, or --bundle-dir");
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

// ─── validate: --bundle-dir conflicts with --api / --bundle ───────────────────

#[test]
fn validate_bundle_dir_conflicts_with_api() {
    let api_toml: PathBuf = test_api_toml();
    let output: Output = run_polyplugc(&[
        "validate",
        "--api",
        api_toml.to_str().expect("api_toml utf8"),
        "--bundle-dir",
        "/tmp/some-dir",
    ]);
    assert!(
        !output.status.success(),
        "--api and --bundle-dir must conflict\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── validate: --bundle-dir on a non-existent dir fails ───────────────────────

#[test]
fn validate_bundle_dir_nonexistent_fails() {
    let output: Output = run_polyplugc(&[
        "validate",
        "--bundle-dir",
        "/nonexistent/path/to/bundle-dir",
    ]);
    assert!(
        !output.status.success(),
        "validate --bundle-dir on a missing dir must fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── generate: semantic output-layout flags ───────────────────────────────────

#[test]
fn generate_domain_types_out_requires_matching_import() {
    let temp = tempdir().expect("create temporary directory");
    let api = test_api_toml();
    let out = temp.path().join("bindings");
    let domain = temp.path().join("domain");
    let output = run_polyplugc(&[
        "generate",
        "--api",
        api.to_str().expect("api path utf8"),
        "--lang",
        "rust",
        "--out",
        out.to_str().expect("out path utf8"),
        "--domain-types-out",
        domain.to_str().expect("domain path utf8"),
    ]);
    assert_failure_contains(&output, "--domain-types-import");
}

#[test]
fn generate_rejects_invalid_rust_partition_import() {
    let temp = tempdir().expect("create temporary directory");
    let api = test_api_toml();
    let out = temp.path().join("bindings");
    let output = run_polyplugc(&[
        "generate",
        "--api",
        api.to_str().expect("api path utf8"),
        "--lang",
        "rust",
        "--out",
        out.to_str().expect("out path utf8"),
        "--guest-contracts-import",
        "invalid-path",
    ]);
    assert_failure_contains(&output, "invalid rust import specifier");
}

#[test]
fn generate_layout_matrix_covers_all_languages_and_generation_modes() {
    let temporary_root = tempdir().expect("create CLI layout matrix root");
    let api = test_api_toml();
    let bundle = test_bundle_toml();
    let omit_api = temporary_root.path().join("omit_api.toml");
    fs::write(
        &omit_api,
        "[[host_contract]]\nname = \"host.matrix\"\nversion = \"1.0\"\n\n[[host_contract.functions]]\nname = \"ping\"\n",
    )
    .expect("write omission API");

    for language in LANGUAGE_LAYOUT_CASES {
        let language_root = temporary_root.path().join(language.lang);
        for (mode, manifest_flag, manifest_path, internal) in [
            ("ordinary", "--bundle", bundle.as_path(), false),
            ("internal", "--bundle", bundle.as_path(), true),
        ] {
            let mode_root = language_root.join(mode);
            let bindings = mode_root.join("bindings");
            let domain = mode_root.join("domain");
            let guest_contracts = mode_root.join("guest_contracts");
            let request = SplitGenerationRequest {
                manifest_flag,
                manifest_path,
                internal,
                language,
                bindings_root: &bindings,
                domain_root: &domain,
                guest_contracts_root: &guest_contracts,
            };
            let output = run_polyplugc_owned(&split_generation_args(&request));
            assert_success(&output);
            assert!(
                directory_has_file(&bindings),
                "{} {mode} split bindings must be emitted",
                language.lang
            );
            assert!(
                directory_has_file(&domain),
                "{} {mode} split domain types must be emitted",
                language.lang
            );
            assert!(
                directory_has_file(&guest_contracts),
                "{} {mode} split guest contracts must be emitted",
                language.lang
            );
        }

        let import_only_root = language_root.join("import_only");
        let import_only_args = vec![
            "generate".to_owned(),
            "--bundle".to_owned(),
            bundle.display().to_string(),
            "--lang".to_owned(),
            language.lang.to_owned(),
            "--out".to_owned(),
            import_only_root.display().to_string(),
            "--domain-types-import".to_owned(),
            language.domain_import.to_owned(),
            "--guest-contracts-import".to_owned(),
            language.guest_contracts_import.to_owned(),
        ];
        let output = run_polyplugc_owned(&import_only_args);
        assert_success(&output);
        assert!(
            directory_has_file(&import_only_root),
            "{} import-only route must preserve binding output",
            language.lang
        );

        let omit_root = language_root.join("omit");
        let omit_args = vec![
            "generate".to_owned(),
            "--api".to_owned(),
            omit_api.display().to_string(),
            "--lang".to_owned(),
            language.lang.to_owned(),
            "--out".to_owned(),
            omit_root.display().to_string(),
            "--guest-contracts-omit".to_owned(),
        ];
        let output = run_polyplugc_owned(&omit_args);
        assert_success(&output);
        assert!(
            directory_has_file(&omit_root),
            "{} omission route must preserve the binding output",
            language.lang
        );

        let missing_pair_root = language_root.join("missing_pair");
        let missing_pair_args = vec![
            "generate".to_owned(),
            "--api".to_owned(),
            api.display().to_string(),
            "--lang".to_owned(),
            language.lang.to_owned(),
            "--out".to_owned(),
            missing_pair_root.display().to_string(),
            "--domain-types-out".to_owned(),
            language_root
                .join("missing_pair_domain")
                .display()
                .to_string(),
        ];
        let output = run_polyplugc_owned(&missing_pair_args);
        assert_failure_contains(&output, "--domain-types-import");

        let invalid_import_root = language_root.join("invalid_import");
        let invalid_import_args = vec![
            "generate".to_owned(),
            "--api".to_owned(),
            api.display().to_string(),
            "--lang".to_owned(),
            language.lang.to_owned(),
            "--out".to_owned(),
            invalid_import_root.display().to_string(),
            "--guest-contracts-import".to_owned(),
            "../outside".to_owned(),
        ];
        let output = run_polyplugc_owned(&invalid_import_args);
        assert_failure_contains(&output, "invalid");
    }
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
