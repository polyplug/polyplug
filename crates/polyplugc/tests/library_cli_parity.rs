#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use polyplug_codegen::GenerateConfig;
use polyplug_codegen::Lang;
use polyplug_codegen::OutputLayout;
use polyplug_codegen::Side;
use polyplug_codegen::generate;
use polyplug_codegen::write_output;
use tempfile::tempdir;

mod cli_support;

use cli_support::cli_generate;

const API_TOML: &str = r#"
[[guest_contract]]
name = "pipeline.Decoder"
version = "1.0.0"

[[guest_contract.functions]]
name = "decode"
params = [{ name = "input", type = "StringView" }]
return = "StringView"
"#;

const BUNDLE_TOML: &str = r#"
[bundle]
name = "parity_decoder"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libparity_decoder.so"

[[plugin]]
name = "decoder"
implements = ["pipeline.Decoder@1.0"]
"#;

#[test]
fn public_library_writer_matches_cli_bytes() {
    let temp = tempdir().expect("create temporary directory");
    let api_path = temp.path().join("api.toml");
    let bundle_path = temp.path().join("bundle.toml");
    let library_dir = temp.path().join("library");
    let cli_dir = temp.path().join("cli");
    fs::write(&api_path, API_TOML).expect("write api manifest");
    fs::write(&bundle_path, BUNDLE_TOML).expect("write bundle manifest");

    let library_config = GenerateConfig {
        api_toml: bundle_path.clone(),
        lang: Lang::Rust,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    };
    let library_output = generate(library_config).expect("generate through public library");
    write_output(&library_output, &library_dir).expect("write through public library");

    let cli_config = GenerateConfig {
        api_toml: bundle_path,
        lang: Lang::Rust,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    };
    let cli_output = cli_generate(&cli_config).expect("generate through CLI");
    write_output(&cli_output, &cli_dir).expect("write CLI output through public library");

    let library_files = output_bytes(&library_dir);
    let cli_files = output_bytes(&cli_dir);
    assert!(!library_files.is_empty(), "fixture must generate files");
    assert_eq!(library_files, cli_files, "library and CLI bytes must match");
}

fn output_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect_output_bytes(root, root, &mut files);
    files
}

fn collect_output_bytes(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in fs::read_dir(directory)
        .expect("read generated directory")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            collect_output_bytes(root, &path, files);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("generated file must be below root")
                .to_path_buf();
            let bytes = fs::read(&path).expect("read generated file");
            files.insert(relative, bytes);
        }
    }
}
