//! Integration tests for the `pack` command scaffold generation.
//!
//! Covers: Cargo.toml correctness, scaffold file structure, manifest.toml
//! correctness, and the full set of supported languages.
//!
//! Run with:
//!   cargo test --test pack_command --package polyplug_codegen

#![allow(clippy::expect_used)]

use polyplug_codegen::{Lang, PackConfig, pack};
use std::fs;
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Minimal bundle TOML with a name and version.
const BUNDLE_TOML: &str = "[bundle]\nname = \"my-plugin\"\nversion = \"1.2.3\"\nfile = \"test.so\"";

/// Bundle TOML whose name contains hyphens (used for C#/Python underscore tests).
const HYPHEN_BUNDLE_TOML: &str =
    "[bundle]\nname = \"my-cool-plugin\"\nversion = \"0.1.0\"\nfile = \"test.so\"";

/// Write a temporary bundle manifest TOML to a temp file and return a
/// `PackConfig` pointing at it with the given language and output dir.
fn make_config(
    bundle_toml: &str,
    lang: Lang,
    out_dir: &std::path::Path,
    tmp_manifest: &std::path::Path,
) -> PackConfig {
    let manifest_path: PathBuf = tmp_manifest.join("bundle.toml");
    fs::create_dir_all(tmp_manifest).expect("create tmp manifest dir");
    fs::write(&manifest_path, bundle_toml).expect("write bundle.toml");
    PackConfig {
        manifest: manifest_path,
        lang,
        out_dir: out_dir.to_path_buf(),
    }
}

/// Read a file inside `out_dir` at the relative `path`, panicking with a
/// helpful message if the file does not exist.
fn read_file(out_dir: &std::path::Path, relative_path: &str) -> String {
    let full: PathBuf = out_dir.join(relative_path);
    fs::read_to_string(&full).unwrap_or_else(|e| panic!("expected file at {}: {e}", full.display()))
}

/// Assert `haystack` contains `needle`, printing a diagnostic on failure.
fn assert_contains(haystack: &str, needle: &str, label: &str) {
    assert!(
        haystack.contains(needle),
        "{label}: expected to contain {needle:?}\nActual:\n{haystack}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Rust scaffold
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rust_scaffold_cargo_toml_is_valid_toml() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Rust, &out_dir, &manifest_dir);
    pack(config).expect("pack rust succeeded");

    let cargo_toml: String = read_file(&out_dir, "Cargo.toml");
    // Must be parseable as TOML.
    let parsed: toml::Value =
        toml::from_str::<toml::Value>(&cargo_toml).expect("Cargo.toml must be valid TOML");
    // [package].name must equal the bundle name.
    let name: &str = parsed["package"]["name"]
        .as_str()
        .expect("[package].name must be a string");
    assert_eq!(name, "my-plugin", "[package].name must match bundle name");

    // [package].version must match.
    let version: &str = parsed["package"]["version"]
        .as_str()
        .expect("[package].version must be a string");
    assert_eq!(
        version, "1.2.3",
        "[package].version must match bundle version"
    );

    // [lib].crate-type must include "cdylib".
    let crate_types: &toml::value::Array = parsed["lib"]["crate-type"]
        .as_array()
        .expect("[lib].crate-type must be an array");
    assert!(
        crate_types
            .iter()
            .any(|v: &toml::Value| v.as_str() == Some("cdylib")),
        "[lib].crate-type must contain \"cdylib\""
    );
}

#[test]
fn rust_scaffold_contains_polyplug_guest_dep() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Rust, &out_dir, &manifest_dir);
    pack(config).expect("pack rust succeeded");

    let cargo_toml: String = read_file(&out_dir, "Cargo.toml");
    assert_contains(&cargo_toml, "polyplug_guest", "Cargo.toml [dependencies]");
}

#[test]
fn rust_scaffold_file_structure() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Rust, &out_dir, &manifest_dir);
    pack(config).expect("pack rust succeeded");

    // Both expected scaffold files must exist.
    assert!(out_dir.join("Cargo.toml").exists(), "Cargo.toml must exist");
    assert!(out_dir.join("src/lib.rs").exists(), "src/lib.rs must exist");
}

#[test]
fn rust_scaffold_lib_rs_has_generated_header() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Rust, &out_dir, &manifest_dir);
    pack(config).expect("pack rust succeeded");

    let lib_rs: String = read_file(&out_dir, "src/lib.rs");
    assert_contains(
        &lib_rs,
        "polyplugc",
        "src/lib.rs must have generated header",
    );
    assert_contains(&lib_rs, "my-plugin", "src/lib.rs must name the plugin");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. C++ scaffold
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cpp_scaffold_file_structure() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Cpp, &out_dir, &manifest_dir);
    pack(config).expect("pack cpp succeeded");

    assert!(
        out_dir.join("CMakeLists.txt").exists(),
        "CMakeLists.txt must exist"
    );
    assert!(
        out_dir.join("include/my-plugin.hpp").exists(),
        "include/my-plugin.hpp must exist"
    );
    assert!(
        out_dir.join("src/my-plugin.cpp").exists(),
        "src/my-plugin.cpp must exist"
    );
}

#[test]
fn cpp_cmake_contains_bundle_name_and_version() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Cpp, &out_dir, &manifest_dir);
    pack(config).expect("pack cpp succeeded");

    let cmake: String = read_file(&out_dir, "CMakeLists.txt");
    assert_contains(
        &cmake,
        "my-plugin",
        "CMakeLists.txt must reference bundle name",
    );
    assert_contains(
        &cmake,
        "1.2.3",
        "CMakeLists.txt must reference bundle version",
    );
}

#[test]
fn cpp_header_has_generated_header_comment() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Cpp, &out_dir, &manifest_dir);
    pack(config).expect("pack cpp succeeded");

    let header: String = read_file(&out_dir, "include/my-plugin.hpp");
    assert_contains(
        &header,
        "polyplugc",
        "include/my-plugin.hpp must have generated header comment",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. C# scaffold
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn csharp_scaffold_file_structure() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(HYPHEN_BUNDLE_TOML, Lang::CSharp, &out_dir, &manifest_dir);
    pack(config).expect("pack csharp succeeded");

    // PascalCase: my-cool-plugin → MyCoolPlugin
    assert!(
        out_dir.join("MyCoolPlugin.csproj").exists(),
        "MyCoolPlugin.csproj must exist"
    );
    assert!(
        out_dir.join("MyCoolPlugin.nuspec").exists(),
        "MyCoolPlugin.nuspec must exist"
    );
    assert!(out_dir.join("Plugin.cs").exists(), "Plugin.cs must exist");
}

#[test]
fn csharp_csproj_contains_version() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::CSharp, &out_dir, &manifest_dir);
    pack(config).expect("pack csharp succeeded");

    let csproj: String = read_file(&out_dir, "MyPlugin.csproj");
    assert_contains(&csproj, "1.2.3", ".csproj must contain bundle version");
}

#[test]
fn csharp_pascal_case_conversion() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(HYPHEN_BUNDLE_TOML, Lang::CSharp, &out_dir, &manifest_dir);
    pack(config).expect("pack csharp succeeded");

    let csproj: String = read_file(&out_dir, "MyCoolPlugin.csproj");
    assert_contains(&csproj, "MyCoolPlugin", ".csproj must use PascalCase name");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Python scaffold
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn python_scaffold_file_structure() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(HYPHEN_BUNDLE_TOML, Lang::Python, &out_dir, &manifest_dir);
    pack(config).expect("pack python succeeded");

    // Hyphens → underscores for Python package names.
    assert!(
        out_dir.join("pyproject.toml").exists(),
        "pyproject.toml must exist"
    );
    assert!(
        out_dir.join("my_cool_plugin/__init__.py").exists(),
        "my_cool_plugin/__init__.py must exist"
    );
    assert!(
        out_dir.join("my_cool_plugin/plugin.py").exists(),
        "my_cool_plugin/plugin.py must exist"
    );
}

#[test]
fn python_pyproject_toml_contains_name_and_version() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Python, &out_dir, &manifest_dir);
    pack(config).expect("pack python succeeded");

    let pyproject: String = read_file(&out_dir, "pyproject.toml");
    assert_contains(
        &pyproject,
        "my-plugin",
        "pyproject.toml must contain bundle name",
    );
    assert_contains(
        &pyproject,
        "1.2.3",
        "pyproject.toml must contain bundle version",
    );
    assert_contains(
        &pyproject,
        "polyplug_guest",
        "pyproject.toml must list polyplug_guest dependency",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Lua scaffold
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lua_scaffold_file_structure() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Lua, &out_dir, &manifest_dir);
    pack(config).expect("pack lua succeeded");

    assert!(out_dir.join("init.lua").exists(), "init.lua must exist");
    assert!(
        out_dir.join("my-plugin-1.2.3.rockspec").exists(),
        "rockspec file must exist"
    );
}

#[test]
fn lua_rockspec_contains_name_and_version() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Lua, &out_dir, &manifest_dir);
    pack(config).expect("pack lua succeeded");

    let rockspec: String = read_file(&out_dir, "my-plugin-1.2.3.rockspec");
    assert_contains(&rockspec, "my-plugin", "rockspec must contain package name");
    assert_contains(&rockspec, "1.2.3", "rockspec must contain version");
    assert_contains(
        &rockspec,
        "polyplug_guest",
        "rockspec must list polyplug_guest dependency",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. JS-QuickJS scaffold
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn js_quickjs_scaffold_file_structure() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::JsQuickJs, &out_dir, &manifest_dir);
    pack(config).expect("pack js-quickjs succeeded");

    assert!(
        out_dir.join("package.json").exists(),
        "package.json must exist"
    );
    assert!(out_dir.join("index.ts").exists(), "index.ts must exist");
    assert!(out_dir.join(".gitignore").exists(), ".gitignore must exist");
}

#[test]
fn js_quickjs_package_json_contains_name_version_and_dep() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::JsQuickJs, &out_dir, &manifest_dir);
    pack(config).expect("pack js-quickjs succeeded");

    let package_json: String = read_file(&out_dir, "package.json");
    assert_contains(
        &package_json,
        "\"my-plugin\"",
        "package.json must contain bundle name",
    );
    assert_contains(
        &package_json,
        "\"1.2.3\"",
        "package.json must contain bundle version",
    );
    assert_contains(
        &package_json,
        "polyplug_guest",
        "package.json must list polyplug_guest dependency",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Version fallback (no bundle section → defaults)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rust_pack_defaults_when_no_bundle_section() {
    // An api.toml without a [bundle] section triggers default name/version.
    let api_toml: &str = "[[contract]]\nname = \"test.math\"\nversion = \"1.0\"\n\n[[contract.functions]]\nname = \"add\"\n";
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let manifest_path: PathBuf = manifest_dir.join("api.toml");
    fs::create_dir_all(&manifest_dir).expect("create manifest dir");
    fs::write(&manifest_path, api_toml).expect("write api.toml");

    let config: PackConfig = PackConfig {
        manifest: manifest_path,
        lang: Lang::Rust,
        out_dir: out_dir.clone(),
    };
    pack(config).expect("pack rust with api.toml succeeded");

    let cargo_toml: String = read_file(&out_dir, "Cargo.toml");
    // Should fall back to name="plugin" and version="0.1.0"
    assert_contains(
        &cargo_toml,
        "plugin",
        "Cargo.toml must use default plugin name",
    );
    assert_contains(
        &cargo_toml,
        "0.1.0",
        "Cargo.toml must use default version 0.1.0",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ═══════════════════════════════════════════════════════════════════════════
// 9. Lua init.lua scaffold has generated header and returns module table
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lua_init_lua_has_generated_header_and_module_table() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Lua, &out_dir, &manifest_dir);
    pack(config).expect("pack lua succeeded");

    let init_lua: String = read_file(&out_dir, "init.lua");
    assert_contains(
        &init_lua,
        "polyplugc",
        "init.lua must have generated header",
    );
    assert_contains(
        &init_lua,
        "return M",
        "init.lua must return the module table",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. All scaffold files carry the generated-by header
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_rust_scaffold_files_carry_generated_header() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Rust, &out_dir, &manifest_dir);
    pack(config).expect("pack rust succeeded");

    for relative in &["Cargo.toml", "src/lib.rs"] {
        let content: String = read_file(&out_dir, relative);
        assert_contains(
            &content,
            "polyplugc",
            &format!("{relative} must contain 'polyplugc' in generated header"),
        );
    }
}

#[test]
fn all_python_scaffold_files_carry_generated_header() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let out_dir: PathBuf = tmp.path().join("out");
    let manifest_dir: PathBuf = tmp.path().join("manifest");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let config: PackConfig = make_config(BUNDLE_TOML, Lang::Python, &out_dir, &manifest_dir);
    pack(config).expect("pack python succeeded");

    for relative in &[
        "pyproject.toml",
        "my_plugin/__init__.py",
        "my_plugin/plugin.py",
    ] {
        let content: String = read_file(&out_dir, relative);
        assert_contains(
            &content,
            "polyplugc",
            &format!("{relative} must contain 'polyplugc' in generated header"),
        );
    }
}
