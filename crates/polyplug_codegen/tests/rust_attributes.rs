#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, Lang, OutputDestination, OutputLayout, Side, ValidatedImport, generate,
    write_output,
};
use tempfile::tempdir;

fn generated<'a>(output: &'a polyplug_codegen::GenerateOutput, path: &str) -> &'a str {
    output
        .files
        .iter()
        .find(|file| file.path == Path::new(path))
        .unwrap_or_else(|| panic!("missing generated {path}"))
        .content
        .as_str()
}

fn generated_partition<'a>(output: &'a polyplug_codegen::GenerateOutput, suffix: &str) -> &'a str {
    output
        .files
        .iter()
        .find(|file| file.path.to_string_lossy().ends_with(suffix))
        .unwrap_or_else(|| panic!("missing generated partition ending in {suffix}"))
        .content
        .as_str()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates directory")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cargo_path(crate_dir: &str) -> String {
    workspace_root()
        .join(crate_dir)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn sentinel_api() -> &'static str {
    r#"
[langs.rust]
attributes = ["allow(non_snake_case)"]

[[types]]
name = "Packet"
langs = { rust = { attributes = ["allow(non_camel_case_types)"] } }
[[types.fields]]
name = "code"
type = "u32"
langs = { rust = { attributes = ["allow(dead_code)"] } }

[[enum]]
name = "Mode"
repr = "u32"
langs = { rust = { attributes = ["allow(non_upper_case_globals)"] } }
[[enum.variants]]
name = "Fast"
value = "1"
langs = { rust = { attributes = ["allow(non_camel_case_types)"] } }

[[enum]]
name = "Flags"
repr = "u32"
bitflag = true
langs = { rust = { attributes = ["allow(non_upper_case_globals)"] } }
[[enum.variants]]
name = "Read"
value = "1"
langs = { rust = { attributes = ["allow(non_camel_case_types)"] } }

[[guest_contract]]
name = "sample.plugin"
version = "1.0.0"
langs = { rust = { attributes = ["allow(non_camel_case_types)"] } }
[[guest_contract.functions]]
name = "invoke"
langs = { rust = { attributes = ["allow(unused_variables)"] } }
[guest_contract.functions.return]
type = "u32"
langs = { rust = { attributes = ["allow(clippy::needless_return)"] } }
[[guest_contract.functions.params]]
name = "value"
type = "u32"
langs = { rust = { attributes = ["allow(unused_variables)", "allow(unused_mut)"] } }

[[host_contract]]
name = "host.logger"
version = "1.0.0"
langs = { rust = { attributes = ["allow(non_camel_case_types)"] } }
[[host_contract.functions]]
name = "log"
langs = { rust = { attributes = ["allow(unused_variables)"] } }
[host_contract.functions.return]
type = "u32"
langs = { rust = { attributes = ["allow(clippy::needless_return)"] } }
[[host_contract.functions.params]]
name = "level"
type = "u32"
langs = { rust = { attributes = ["allow(unused_variables)", "allow(unused_mut)"] } }
"#
}

fn generate_rust(
    api: PathBuf,
    side: Side,
    layout: OutputLayout,
) -> polyplug_codegen::GenerateOutput {
    generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Rust,
        side,
        layout,
    })
    .expect("generate Rust bindings")
}

#[test]
fn rust_attributes_cover_public_semantic_surfaces_in_unified_and_split_outputs() {
    let temp = tempdir().expect("temporary api directory");
    let api = temp.path().join("api.toml");
    fs::write(&api, sentinel_api()).expect("write sentinel api");

    let host = generate_rust(api.clone(), Side::Host, OutputLayout::unified());
    let guest = generate_rust(api.clone(), Side::Guest, OutputLayout::unified());

    assert!(generated(&host, "mod.rs").contains("#![allow(non_snake_case)]\npub mod host;"));
    assert!(
        generated(&guest, "guest/mod.rs").contains("#![allow(non_snake_case)]\npub mod contracts;")
    );

    let guest_types = generated(&guest, "guest/types.rs");
    assert_eq!(
        guest_types.matches("#![allow(non_snake_case)]").count(),
        2,
        "the generated public declaration root must retain the LangPrint inner API attribute"
    );
    assert!(guest_types.contains("#[repr(C)]\n#[allow(non_camel_case_types)]\n#[derive(Debug, Clone, Copy)]\npub struct Packet"));
    assert!(guest_types.contains("#[allow(dead_code)]\n    pub code: u32"));
    assert!(guest_types.contains("#[allow(non_upper_case_globals)]\n#[repr(u32)]\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum Mode"));
    assert!(guest_types.contains("    #[allow(non_camel_case_types)]\n    Fast = 1,"));
    assert!(guest_types.contains("#[allow(non_upper_case_globals)]\npub mod flags"));
    assert!(
        guest_types.contains("    #[allow(non_camel_case_types)]\n    pub const READ: Flags = 1;")
    );

    let guest_contracts = generated(&guest, "guest/contracts.rs");
    assert!(
        guest_contracts
            .contains("#[allow(non_camel_case_types)]\npub trait SamplePluginGuestContract")
    );
    assert!(guest_contracts.contains(
        "#[allow(clippy::needless_return)]\n    #[allow(unused_variables)]\n    fn invoke(&self,\n        #[allow(unused_variables)]\n        #[allow(unused_mut)]\n        value: u32)"
    ));

    let host_callers = generated(&host, "host/host_callers.rs");
    assert!(
        host_callers.contains("#[allow(non_camel_case_types)]\npub struct SamplePluginContract")
    );
    assert!(host_callers.contains(
        "#[allow(clippy::needless_return)]\n    #[allow(unused_variables)]\n    #[allow(clippy::absurd_extreme_comparisons)]\n    pub fn invoke(&mut self,\n        #[allow(unused_variables)]\n        #[allow(unused_mut)]\n        value: u32)"
    ));

    let host_contracts = generated(&host, "host/host_contracts.rs");
    assert!(host_contracts.contains("#[allow(non_camel_case_types)]\npub trait HostLogger"));
    assert!(host_contracts.contains(
        "#[allow(clippy::needless_return)]\n    #[allow(unused_variables)]\n    fn log(&self,\n        #[allow(unused_variables)]\n        #[allow(unused_mut)]\n        level: u32)"
    ));

    let guest_host_callers = generated(&guest, "guest/host_contract_callers.rs");
    assert!(
        guest_host_callers.contains("#[allow(non_camel_case_types)]\npub struct HostLoggerCaller")
    );
    assert!(guest_host_callers.contains(
        "#[allow(clippy::needless_return)]\n#[allow(unused_variables)]\npub fn log(\n    &self,\n    #[allow(unused_variables)]\n    #[allow(unused_mut)]\n    level: u32,"
    ));

    let split = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: PathBuf::from("domain"),
            import: ValidatedImport::parse(Lang::Rust, "app::domain")
                .expect("valid Rust domain import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: PathBuf::from("contracts"),
            import: ValidatedImport::parse(Lang::Rust, "app::contracts")
                .expect("valid Rust guest-contract import"),
        },
    };
    let split_guest = generate_rust(api, Side::Guest, split);
    let split_domain = generated_partition(&split_guest, "guest/domain.rs");
    assert!(
        split_domain.starts_with(
            "// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n#![allow(non_snake_case)]"
        ),
        "split domain root must use LangPrint's Rust inner attribute syntax: {split_domain}"
    );
    assert!(split_domain.contains("#[allow(non_camel_case_types)]\npub struct Packet"));
    assert!(split_domain.contains("#[allow(dead_code)]\n    pub code: u32"));
    let split_contracts = generated_partition(&split_guest, "guest/guest_contracts.rs");
    assert!(
        split_contracts.contains("#[allow(non_camel_case_types)]\npub trait SamplePluginContract")
    );
    assert!(split_contracts.contains(
        "#[allow(clippy::needless_return)]\n    #[allow(unused_variables)]\n    fn invoke(&self,\n        #[allow(unused_variables)]\n        #[allow(unused_mut)]\n        value: u32)"
    ));
}

#[test]
fn rust_empty_attribute_rules_preserve_generated_bytes() {
    let temp = tempdir().expect("temporary api directory");
    let without_rules = temp.path().join("without-rules.toml");
    let empty_rules = temp.path().join("empty-rules.toml");
    let base = r#"
[[types]]
name = "Packet"
[[types.fields]]
name = "code"
type = "u32"

[[guest_contract]]
name = "sample.plugin"
version = "1.0.0"
[[guest_contract.functions]]
name = "invoke"
[guest_contract.functions.return]
type = "u32"
"#;
    fs::write(&without_rules, base).expect("write base api");
    fs::write(
        &empty_rules,
        format!("[langs.rust]\nattributes = []\n\n{base}"),
    )
    .expect("write empty-rules api");

    for side in [Side::Host, Side::Guest] {
        let without = generate_rust(without_rules.clone(), side, OutputLayout::unified());
        let empty = generate_rust(empty_rules.clone(), side, OutputLayout::unified());
        assert_eq!(
            without
                .files
                .iter()
                .map(|file| (&file.path, &file.content))
                .collect::<Vec<_>>(),
            empty
                .files
                .iter()
                .map(|file| (&file.path, &file.content))
                .collect::<Vec<_>>(),
            "empty Rust rules must not alter {side:?} output"
        );
    }
}

#[test]
fn generated_rust_host_and_guest_with_attributes_compile() {
    let temp = tempdir().expect("temporary generated crate");
    let api = temp.path().join("api.toml");
    fs::write(&api, sentinel_api()).expect("write sentinel api");
    let host = generate_rust(api.clone(), Side::Host, OutputLayout::unified());
    let guest = generate_rust(api, Side::Guest, OutputLayout::unified());

    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    let generated_dir = source_dir.join("generated");
    fs::create_dir_all(&generated_dir).expect("create generated source directory");
    write_output(&host, &generated_dir).expect("write generated host");
    write_output(&guest, &generated_dir).expect("write generated guest");
    let generated_root = generated_dir.join("mod.rs");
    let mut root = fs::read_to_string(&generated_root).expect("read generated root");
    root.push_str("pub mod guest;\n");
    fs::write(&generated_root, root).expect("wire generated guest module");

    fs::write(
        crate_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated_rust_attribute_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write consumer manifest");
    fs::write(
        source_dir.join("main.rs"),
        "#[path = \"generated/mod.rs\"]\nmod generated;\nfn main() {}\n",
    )
    .expect("write consumer source");

    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&crate_root)
        .output()
        .expect("run generated consumer check");
    assert!(
        output.status.success(),
        "generated Rust host and guest bindings with attributes did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
