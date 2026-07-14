#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, Lang, OutputDestination, OutputLayout, OutputPartition, Side, ValidatedImport,
    generate,
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

fn generated_partition(
    output: &polyplug_codegen::GenerateOutput,
    partition: OutputPartition,
) -> &str {
    output
        .files
        .iter()
        .find(|file| file.partition == partition)
        .unwrap_or_else(|| panic!("missing generated {partition:?} partition"))
        .content
        .as_str()
}

fn sentinel_api() -> &'static str {
    r#"
[langs.lua]
attributes = ["root-first", "root-second"]

[[types]]
name = "Packet"
langs = { lua = { attributes = ["type-sentinel"] } }
[[types.fields]]
name = "code"
type = "u32"
langs = { lua = { attributes = ["field-sentinel"] } }

[[enum]]
name = "Mode"
repr = "u32"
langs = { lua = { attributes = ["enum-sentinel"] } }
[[enum.variants]]
name = "Fast"
value = "1"
langs = { lua = { attributes = ["variant-sentinel"] } }

[[enum]]
name = "Flags"
repr = "u32"
bitflag = true
langs = { lua = { attributes = ["bitflag-sentinel"] } }
[[enum.variants]]
name = "Read"
value = "1"
langs = { lua = { attributes = ["bitflag-variant-sentinel"] } }

[[guest_contract]]
name = "sample.Plugin"
version = "1.0.0"
langs = { lua = { attributes = ["guest-contract-sentinel"] } }
[[guest_contract.functions]]
name = "invoke"
langs = { lua = { attributes = ["guest-function-sentinel"] } }
[guest_contract.functions.return]
type = "u32"
langs = { lua = { attributes = ["guest-return-sentinel"] } }
[[guest_contract.functions.params]]
name = "value"
type = "u32"
langs = { lua = { attributes = ["guest-param-sentinel"] } }

[[host_contract]]
name = "host.Logger"
version = "1.0.0"
langs = { lua = { attributes = ["host-contract-sentinel"] } }
[[host_contract.functions]]
name = "log"
langs = { lua = { attributes = ["host-function-sentinel"] } }
[host_contract.functions.return]
type = "u32"
langs = { lua = { attributes = ["host-return-sentinel"] } }
[[host_contract.functions.params]]
name = "level"
type = "u32"
langs = { lua = { attributes = ["host-param-sentinel"] } }
"#
}

fn write_bundle(directory: &Path, api: &str) -> PathBuf {
    let api_path = directory.join("api.toml");
    let bundle_path = directory.join("bundle.toml");
    fs::write(&api_path, api).expect("write Lua sentinel API");
    fs::write(
        &bundle_path,
        "[bundle]\nname = \"lua_attributes\"\nversion = \"1.0.0\"\napi = \"api.toml\"\nloader = \"lua\"\nfile = \"plugin.lua\"\n\n[[plugin]]\nname = \"lua_attributes.provider\"\nimplements = [\"sample.Plugin@1.0\"]\n",
    )
    .expect("write Lua sentinel bundle");
    bundle_path
}

fn generate_lua(
    api_toml: PathBuf,
    side: Side,
    layout: OutputLayout,
) -> polyplug_codegen::GenerateOutput {
    generate(GenerateConfig {
        api_toml,
        lang: Lang::Lua,
        side,
        layout,
    })
    .expect("generate Lua bindings")
}

fn assert_in_order(text: &str, ordered: &[&str]) {
    let mut offset = 0;
    for expected in ordered {
        let index = text[offset..]
            .find(expected)
            .unwrap_or_else(|| panic!("missing `{expected}` in:\n{text}"));
        offset += index + expected.len();
    }
}

#[test]
fn lua_attributes_cover_public_semantic_surfaces_in_unified_and_split_outputs() {
    let temp = tempdir().expect("temporary Lua attribute fixture");
    let bundle = write_bundle(temp.path(), sentinel_api());
    let host = generate_lua(bundle.clone(), Side::Host, OutputLayout::unified());
    let guest = generate_lua(bundle.clone(), Side::Guest, OutputLayout::unified());

    let guest_types = generated(&guest, "guest/types.lua");
    assert_in_order(
        guest_types,
        &[
            "---@langprint Root: root-first\n---@langprint Root: root-second",
            "---@langprint Enum: enum-sentinel\n---@enum Mode",
            "---@langprint Variant: variant-sentinel\n---@field Fast integer",
            "---@langprint Enum: bitflag-sentinel\n---@enum Flags",
            "---@langprint Variant: bitflag-variant-sentinel\n---@field Read integer",
            "---@langprint Type: type-sentinel\n---@class Packet",
            "---@langprint Field: field-sentinel\n---@field code number",
        ],
    );
    assert_eq!(
        guest_types
            .matches("---@langprint Root: root-first")
            .count(),
        1
    );
    assert!(generated(&host, "host/types.lua").contains("---@langprint Root: root-second"));

    let guest_contracts = generated(&guest, "guest/contracts.lua");
    assert_in_order(
        guest_contracts,
        &[
            "---@langprint Type: guest-contract-sentinel\n---@class SamplePluginContractProvider",
            "---@langprint Function: guest-function-sentinel",
            "---@langprint Parameter: guest-param-sentinel",
            "---@langprint Return: guest-return-sentinel\n---@field invoke fun(self: SamplePluginContractProvider, value: number): number",
        ],
    );
    assert!(
        !guest_contracts.contains("---@langprint Root:"),
        "root annotations belong to the domain types unit, not a generated registration wrapper"
    );
    assert!(
        !guest_contracts
            .contains("---@langprint Function: guest-function-sentinel\nfunction M._register_"),
        "function annotations must target the public provider declaration, never private registration handlers"
    );

    let host_callers = generated(&host, "host/callers.lua");
    assert_in_order(
        host_callers,
        &[
            "---@langprint Type: guest-contract-sentinel\n---@class SamplePluginContract",
            "---@langprint Function: guest-function-sentinel",
            "---@langprint Parameter: guest-param-sentinel",
            "---@langprint Return: guest-return-sentinel",
            "invoke = function(self, value)",
        ],
    );

    let host_contracts = generated(&host, "host/contracts.lua");
    assert_in_order(
        host_contracts,
        &[
            "---@langprint Type: host-contract-sentinel\nHostLogger = {}",
            "---@langprint Function: host-function-sentinel",
            "---@langprint Parameter: host-param-sentinel",
            "---@langprint Return: host-return-sentinel",
            "function HostLogger:log(level)",
        ],
    );

    let guest_host_contracts = generated(&guest, "guest/host_contracts.lua");
    assert_in_order(
        guest_host_contracts,
        &[
            "---@langprint Type: host-contract-sentinel\nHostLoggerContract = {}",
            "---@langprint Function: host-function-sentinel",
            "---@langprint Parameter: host-param-sentinel",
            "---@langprint Return: host-return-sentinel",
            "function HostLoggerContract:log(level)",
        ],
    );

    let split = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: temp.path().join("domain"),
            import: ValidatedImport::parse(Lang::Lua, "sample.domain")
                .expect("valid Lua domain import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: temp.path().join("contracts"),
            import: ValidatedImport::parse(Lang::Lua, "sample.contracts")
                .expect("valid Lua contract import"),
        },
    };
    let split_guest = generate_lua(bundle, Side::Guest, split);
    let split_domain = generated_partition(&split_guest, OutputPartition::DomainTypes);
    assert!(split_domain.contains("---@langprint Type: type-sentinel\n---@class Packet"));
    assert!(
        split_domain
            .contains("---@langprint Variant: bitflag-variant-sentinel\n---@field Read integer")
    );
    let split_contracts = generated_partition(&split_guest, OutputPartition::GuestContracts);
    assert!(split_contracts.contains(
        "---@langprint Type: guest-contract-sentinel\n---@class SamplePluginContractProvider"
    ));
    assert!(
        split_contracts.contains("---@langprint Return: guest-return-sentinel\n---@field invoke")
    );
}

#[test]
fn lua_empty_attribute_rules_preserve_generated_bytes() {
    let temp = tempdir().expect("temporary Lua byte identity fixture");
    let base = r#"
[[types]]
name = "Packet"
[[types.fields]]
name = "code"
type = "u32"

[[guest_contract]]
name = "sample.Plugin"
version = "1.0.0"
[[guest_contract.functions]]
name = "invoke"
[guest_contract.functions.return]
type = "u32"

[[host_contract]]
name = "host.Logger"
version = "1.0.0"
[[host_contract.functions]]
name = "log"
[host_contract.functions.return]
type = "u32"
"#;
    let without_rules = temp.path().join("without");
    let empty_rules = temp.path().join("empty");
    fs::create_dir_all(&without_rules).expect("create no-rule fixture");
    fs::create_dir_all(&empty_rules).expect("create empty-rule fixture");
    let without_bundle = write_bundle(&without_rules, base);
    let empty_bundle = write_bundle(
        &empty_rules,
        &format!("[langs.lua]\nattributes = []\n\n{base}"),
    );

    for side in [Side::Host, Side::Guest] {
        let without = generate_lua(without_bundle.clone(), side, OutputLayout::unified());
        let empty = generate_lua(empty_bundle.clone(), side, OutputLayout::unified());
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
            "empty Lua rules must not alter {side:?} output"
        );
    }
}

#[test]
fn lua_attributes_parse_with_luajit() {
    let temp = tempdir().expect("temporary Lua parser fixture");
    let bundle = write_bundle(temp.path(), sentinel_api());
    let host = generate_lua(bundle.clone(), Side::Host, OutputLayout::unified());
    let guest = generate_lua(bundle, Side::Guest, OutputLayout::unified());
    let parser = temp.path().join("parse.lua");
    fs::write(&parser, "assert(loadfile(arg[1]))\n").expect("write LuaJIT parser");

    for (projection, output) in [("host", host), ("guest", guest)] {
        for file in &output.files {
            if file
                .path
                .extension()
                .is_some_and(|extension| extension == "lua")
            {
                let source = temp.path().join(projection).join(&file.path);
                fs::create_dir_all(source.parent().expect("Lua source parent"))
                    .expect("create Lua source parent");
                fs::write(&source, &file.content).expect("write generated Lua source");
                let result = Command::new("luajit")
                    .arg(&parser)
                    .arg(&source)
                    .output()
                    .expect("run LuaJIT parser");
                assert!(
                    result.status.success(),
                    "LuaJIT could not parse {} {}:\n{}\n{}",
                    projection,
                    file.path.display(),
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr),
                );
            }
        }
    }
}
