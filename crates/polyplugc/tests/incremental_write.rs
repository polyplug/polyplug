//! Incremental-write behaviour of `polyplugc::write_output`.
//!
//! Every generator stamps `GeneratedFile::force_regenerate` — `true` for files like
//! `manifest.toml` whose ids must always reflect the current contract, `false` for
//! language bindings. This test proves the consumer honours that flag: a re-run that
//! produces byte-identical output rewrites ONLY the force-regenerate files and leaves
//! unchanged bindings untouched, while a binding whose on-disk content drifts is
//! rewritten. Without the flag being wired (its prior state), every file was rewritten
//! unconditionally and this contract had no enforcement.

#![allow(clippy::expect_used)]

use polyplugc::WriteSummary;
use polyplugc::generate;
use polyplugc::write_output;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use polyplug_codegen::{GenerateConfig, GenerateOutput, Lang, Side};

const API_TOML: &str = "\
[[plugin_contract]]
name = \"pipeline.Decoder\"
version = \"1.0.0\"

[[plugin_contract.functions]]
name = \"decode\"
return = \"StringView\"
";

const BUNDLE_TOML: &str = "\
[bundle]
name = \"inc_write\"
version = \"1.0.0\"
api = \"api.toml\"
loader = \"lua\"
file = \"plugin.lua\"

[[plugin]]
name = \"decoder\"
implements = [\"pipeline.Decoder@1.0\"]
";

fn generate_lua_bundle(tmp_dir: &PathBuf) -> (GenerateOutput, PathBuf) {
    fs::create_dir_all(tmp_dir).expect("create tmp dir");
    let api_path: PathBuf = tmp_dir.join("api.toml");
    let bundle_path: PathBuf = tmp_dir.join("bundle.toml");
    fs::write(&api_path, API_TOML).expect("write api.toml");
    fs::write(&bundle_path, BUNDLE_TOML).expect("write bundle.toml");

    let out_dir: PathBuf = tmp_dir.join("out");
    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_path,
        lang: Lang::Lua,
        side: Side::Guest,
        out_dir: out_dir.clone(),
    };
    let output: GenerateOutput = generate(config).expect("generate guest");
    (output, out_dir)
}

#[test]
fn manifest_is_force_regenerate_and_bindings_are_not() {
    let tmp_dir: PathBuf =
        env::temp_dir().join(format!("polyplugc_inc_write_flags_{}", process::id()));
    let _ = fs::remove_dir_all(&tmp_dir);
    let (output, _out_dir): (GenerateOutput, PathBuf) = generate_lua_bundle(&tmp_dir);

    let manifest = output
        .files
        .iter()
        .find(|f| f.path.file_name().is_some_and(|n| n == "manifest.toml"))
        .expect("manifest.toml must be generated");
    assert!(
        manifest.force_regenerate,
        "manifest.toml must be force_regenerate (its ids must always be current)"
    );

    let lua_binding = output
        .files
        .iter()
        .find(|f| f.path.extension().is_some_and(|e| e == "lua"))
        .expect("a .lua binding must be generated");
    assert!(
        !lua_binding.force_regenerate,
        "language bindings must NOT be force_regenerate (so unchanged ones are cached)"
    );

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn rewrites_only_force_and_changed_files() {
    let tmp_dir: PathBuf =
        env::temp_dir().join(format!("polyplugc_inc_write_cache_{}", process::id()));
    let _ = fs::remove_dir_all(&tmp_dir);
    let (output, out_dir): (GenerateOutput, PathBuf) = generate_lua_bundle(&tmp_dir);

    let total: usize = output.files.len();
    let force_count: usize = output.files.iter().filter(|f| f.force_regenerate).count();
    assert!(
        force_count >= 1,
        "expected at least the manifest to be force_regenerate"
    );
    assert!(
        total > force_count,
        "expected at least one cacheable binding"
    );

    // First write: nothing on disk yet, so every file is written.
    let first: WriteSummary = write_output(&output, &out_dir).expect("first write");
    assert_eq!(first.written, total, "first write must emit every file");
    assert_eq!(first.unchanged, 0, "first write has nothing to skip");

    // Identical re-write: only the force-regenerate files are rewritten; every other
    // file is byte-identical on disk and skipped.
    let second: WriteSummary = write_output(&output, &out_dir).expect("second write");
    assert_eq!(
        second.written, force_count,
        "an identical re-run must rewrite only the force_regenerate files"
    );
    assert_eq!(
        second.unchanged,
        total - force_count,
        "every unchanged binding must be skipped"
    );

    // Drift one cached binding on disk: it must be rewritten back to canonical form.
    let victim = output
        .files
        .iter()
        .find(|f| !f.force_regenerate)
        .expect("a non-force binding exists");
    let victim_path: PathBuf = out_dir.join(&victim.path);
    fs::write(&victim_path, "-- stale, drifted content\n").expect("drift victim");

    let third: WriteSummary = write_output(&output, &out_dir).expect("third write");
    assert_eq!(
        third.written,
        force_count + 1,
        "the drifted binding plus the force files must be rewritten"
    );
    assert_eq!(third.unchanged, total - force_count - 1);

    let restored: String = fs::read_to_string(&victim_path).expect("read restored victim");
    assert_eq!(
        restored, victim.content,
        "the drifted binding must be restored to its generated content"
    );

    let _ = fs::remove_dir_all(&tmp_dir);
}
