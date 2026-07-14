//! Round-trip test for `[[dependency]]` tables emitted into a generated
//! `manifest.toml`.
//!
//! Regression guard for the shared `emit_manifest_dependencies` helper: every
//! generator must emit the full union of fields the RUNTIME manifest parser
//! requires (`kind`, `contract`, `contract_id`, plus `bundle`/`bundle_id` for
//! ByBundle deps, and `min_version` as a quoted string). Previously the
//! generators diverged — some omitted `contract_id`/`bundle_id` (so ByBundle
//! deps were dropped and ByContract deps got contract_id=0), others omitted the
//! REQUIRED `kind` or emitted `min_version` as a bare integer (parse failure).
//!
//! This test generates a bundle with both dependency shapes and parses the
//! emitted manifest through the actual runtime parser to prove it round-trips.

#![allow(clippy::expect_used)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use polyplug_codegen::{GenerateConfig, GenerateOutput, Lang, OutputLayout, Side};
use polyplug_common::{ManifestData, ManifestDependency};

mod cli_support;
use cli_support::cli_generate;

const API_TOML: &str = "\
[[guest_contract]]
name = \"pipeline.Decoder\"
version = \"1.0.0\"

[[guest_contract.functions]]
name = \"decode\"
return = \"StringView\"
";

const DEPENDENCIES_TOML: &str = "\
[[plugin]]
name = \"decoder\"
implements = [\"pipeline.Decoder@1.0\"]

[[dependency]]
kind = \"contract\"
contract = \"image.Transcoder\"
min_version = \"2.0\"

[[dependency]]
kind = \"bundle\"
bundle = \"codec-bundle\"
contract = \"audio.Encoder\"
min_version = \"1.0\"
";

/// Build a bundle.toml whose `runtime` and `[file]` shape match what the parser
/// requires for the given language: native runtimes (rust, cpp) need a
/// `[bundle.file]` platform table, VM runtimes need a flat `file` field.
fn bundle_toml_for_lang(lang: Lang) -> String {
    let (runtime, file_block): (&str, &str) = match lang {
        // Declare every platform CI/dev machines run on — the roundtrip feeds
        // the generated manifest back through the runtime parser, which errors
        // when the ACTIVE platform has no [file] entry.
        Lang::Rust => (
            "native",
            "[bundle.file]\nlinux.x86_64 = \"libdep.so\"\nmacos.x86_64 = \"libdep.dylib\"\nmacos.aarch64 = \"libdep.dylib\"\nwindows.x86_64 = \"dep.dll\"\n",
        ),
        Lang::Cpp => (
            "native",
            "[bundle.file]\nlinux.x86_64 = \"libdep.so\"\nmacos.x86_64 = \"libdep.dylib\"\nmacos.aarch64 = \"libdep.dylib\"\nwindows.x86_64 = \"dep.dll\"\n",
        ),
        Lang::CSharp => ("csharp", "file = \"libdep.dll\"\n"),
        Lang::Python => ("python", "file = \"plugin.py\"\n"),
        Lang::Lua => ("lua", "file = \"plugin.lua\"\n"),
        Lang::JsQuickJs => ("js-quickjs", "file = \"plugin.js\"\n"),
    };
    format!(
        "[bundle]\nname = \"dep_roundtrip\"\nversion = \"1.0.0\"\napi = \"api.toml\"\nloader = \"{runtime}\"\n{file_block}\n{DEPENDENCIES_TOML}"
    )
}

fn generate_manifest_for_lang(lang: Lang) -> String {
    let tmp_dir: PathBuf = env::temp_dir().join(format!(
        "polyplugc_dep_roundtrip_{:?}_{}",
        lang,
        process::id()
    ));
    let _ = fs::remove_dir_all(&tmp_dir);
    fs::create_dir_all(&tmp_dir).expect("create tmp dir");

    let api_path: PathBuf = tmp_dir.join("api.toml");
    let bundle_path: PathBuf = tmp_dir.join("bundle.toml");
    fs::write(&api_path, API_TOML).expect("write api.toml");
    fs::write(&bundle_path, bundle_toml_for_lang(lang)).expect("write bundle.toml");

    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_path,
        lang,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    };
    let output: GenerateOutput = cli_generate(&config).expect("generate guest");

    let manifest_file = output
        .files
        .into_iter()
        .find(|f| {
            f.path
                .file_name()
                .map(|n| n == "manifest.toml")
                .unwrap_or(false)
        })
        .expect("manifest.toml must be generated");

    manifest_file.content
}

/// Parse the emitted manifest through the real runtime parser and assert both
/// dependency shapes round-trip with the ids the runtime needs.
fn assert_manifest_roundtrips(manifest_toml: &str) {
    let parsed: ManifestData =
        ManifestData::parse_from_str(manifest_toml).expect("runtime parser must accept manifest");

    let deps: Vec<ManifestDependency> = parsed.resolved_dependencies();
    assert_eq!(
        deps.len(),
        2,
        "both dependencies must survive resolution (ByBundle without bundle_id would be dropped):\n{manifest_toml}"
    );

    let mut saw_by_contract: bool = false;
    let mut saw_by_bundle: bool = false;
    for dep in &deps {
        match dep {
            ManifestDependency::ByContract {
                contract,
                contract_id,
                ..
            } => {
                saw_by_contract = true;
                assert_eq!(contract, "image.Transcoder");
                assert_ne!(
                    contract_id.id(),
                    0,
                    "ByContract dep must carry a non-zero contract_id"
                );
            }
            ManifestDependency::ByBundle {
                bundle,
                bundle_id,
                contract,
                contract_id,
                ..
            } => {
                saw_by_bundle = true;
                assert_eq!(bundle, "codec-bundle");
                assert_eq!(contract, "audio.Encoder");
                assert_ne!(
                    bundle_id.id(),
                    0,
                    "ByBundle dep must carry a non-zero bundle_id"
                );
                assert_ne!(
                    contract_id.id(),
                    0,
                    "ByBundle dep must carry a non-zero contract_id"
                );
            }
        }
    }
    assert!(saw_by_contract, "expected a ByContract dependency");
    assert!(saw_by_bundle, "expected a ByBundle dependency");
}

#[test]
fn rust_manifest_dependencies_roundtrip() {
    assert_manifest_roundtrips(&generate_manifest_for_lang(Lang::Rust));
}

#[test]
fn cpp_manifest_dependencies_roundtrip() {
    assert_manifest_roundtrips(&generate_manifest_for_lang(Lang::Cpp));
}

#[test]
fn csharp_manifest_dependencies_roundtrip() {
    assert_manifest_roundtrips(&generate_manifest_for_lang(Lang::CSharp));
}

#[test]
fn python_manifest_dependencies_roundtrip() {
    assert_manifest_roundtrips(&generate_manifest_for_lang(Lang::Python));
}

#[test]
fn lua_manifest_dependencies_roundtrip() {
    assert_manifest_roundtrips(&generate_manifest_for_lang(Lang::Lua));
}

#[test]
fn js_manifest_dependencies_roundtrip() {
    assert_manifest_roundtrips(&generate_manifest_for_lang(Lang::JsQuickJs));
}
