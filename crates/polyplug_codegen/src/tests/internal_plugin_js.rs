//! Focused output contracts for internal JavaScript binding generation.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_utils::bundle_id;

use crate::GenerateConfig;
use crate::GenerateOutput;
use crate::InternalJavaScriptGenerateConfig;
use crate::Lang;
use crate::OutputDestination;
use crate::OutputLayout;
use crate::OutputPartition;
use crate::Side;
use crate::ValidatedImport;
use crate::generate;
use crate::generate_internal_javascript;
use crate::write_output;

fn output_map(output: GenerateOutput) -> BTreeMap<PathBuf, String> {
    output
        .files
        .into_iter()
        .map(|file| (file.path, file.content))
        .collect()
}

fn write_api(path: &Path) {
    fs::write(
        path,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Fast\"\nvalue = \"1\"\n\n[[types]]\nname = \"Inner\"\nfields = [{ name = \"name\", type = \"StringView\" }]\n\n[[types]]\nname = \"Outer\"\nfields = [{ name = \"inner\", type = \"Inner\" }, { name = \"payload\", type = \"Buffer\" }]\n\n[[guest_contract]]\nname = \"javascript.profile\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"scalar\"\nparams = [{ name = \"value\", type = \"u32\" }]\nreturn = \"u32\"\n\n[[guest_contract.functions]]\nname = \"text\"\nparams = [{ name = \"value\", type = \"StringView\" }]\nreturn = \"StringView\"\n\n[[guest_contract.functions]]\nname = \"many\"\nparams = [{ name = \"mode\", type = \"Mode\" }, { name = \"item\", type = \"Outer\" }, { name = \"values\", type = \"Array<u32>\" }]\nreturn = \"Outer\"\n\n[[guest_contract.functions]]\nname = \"bytes\"\nparams = [{ name = \"value\", type = \"Buffer\" }]\nreturn = \"Buffer\"\n",
    )
    .expect("write API TOML");
}

fn write_internal_bundle(path: &Path, name: &str, api: &str, plugin: &str) {
    fs::write(
        path,
        format!(
            "[bundle]\nname = \"{name}\"\nversion = \"1.0\"\napi = \"{api}\"\n\n[[plugin]]\nname = \"{plugin}\"\nimplements = [\"javascript.profile@1.0\"]\n"
        ),
    )
    .expect("write internal bundle TOML");
}

fn split_layout() -> OutputLayout {
    OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::JsQuickJs, "@test/javascript-domain")
                .expect("valid domain module specifier"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::JsQuickJs, "@test/javascript-contracts")
                .expect("valid contract module specifier"),
        },
    }
}

fn file_url(path: &Path) -> String {
    let normalized: String = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

#[test]
fn internal_javascript_profile_is_namespaced_typed_and_consumed_on_attempt() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    write_internal_bundle(
        &bundle,
        "javascript_internal",
        "api.toml",
        "javascript_provider",
    );

    let output = output_map(
        generate_internal_javascript(InternalJavaScriptGenerateConfig {
            bundle_toml: bundle,
            out_dir: temp.path().join("out"),
            layout: Default::default(),
        })
        .expect("generate internal JavaScript profile"),
    );
    let namespace = Path::new("internal").join(format!(
        "javascript_internal-{:016x}",
        bundle_id("javascript_internal")
    ));
    assert!(
        output.keys().all(|path| path.starts_with(&namespace)),
        "every internal JavaScript file must be namespaced: {output:#?}"
    );

    let facade = output
        .get(&namespace.join("internal.ts"))
        .expect("internal façade");
    let contracts = output
        .get(&namespace.join("guest").join("contracts.ts"))
        .expect("guest provider bindings");
    let callers = output
        .get(&namespace.join("host").join("callers.ts"))
        .expect("host caller bindings");

    assert!(facade.contains("export class InternalProviders"));
    assert!(facade.contains("internal provider input has already been consumed"));
    assert!(facade.contains("buildInternalPluginGuestContract"));
    assert!(facade.contains("createInternalPluginGuestBridge"));
    assert!(facade.contains("bundle.dispose(); throw error"));
    assert!(facade.contains("registerInternalPluginWithHandles"));
    assert!(facade.contains("published.handles[0]"));
    assert!(facade.contains("javascript_provider_javascript_profile"));
    assert!(facade.contains("scalar(value: number): number"));
    assert!(facade.contains("text(value: string): string"));
    assert!(!facade.contains("GuestContractInterface"));
    assert!(!facade.contains("PluginDescriptor"));

    assert!(contracts.contains("javascript_provider_javascript_profile_fn2_abi_wrapper"));
    assert!(contracts.contains("arg_item = { inner:"));
    assert!(contracts.contains("arg_values"));
    assert!(contracts.contains("arenaAlloc"));
    assert!(callers.contains("export class JavascriptProfileContract"));
    assert!(callers.contains("createFromHandle"));
    assert!(callers.contains("interfacePtr(): Deno.PointerValue;"));
    assert!(callers.contains("if (view.interfacePtr() === this.#view.interfacePtr()) {"));
}

#[test]
fn default_and_external_javascript_generation_do_not_emit_internal_artifacts() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("external.toml");
    write_api(&api);
    fs::write(
        &bundle,
        "[bundle]\nname = \"javascript_external\"\nversion = \"1.0\"\nloader = \"js-quickjs\"\nfile = \"plugin.js\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"javascript_provider\"\nimplements = [\"javascript.profile@1.0\"]\n",
    )
    .expect("write external bundle TOML");

    let default = output_map(
        generate(GenerateConfig {
            api_toml: api,
            lang: Lang::JsQuickJs,
            side: Side::Host,
            layout: OutputLayout::unified(),
        })
        .expect("generate default JavaScript host bindings"),
    );
    let external = output_map(
        generate(GenerateConfig {
            api_toml: bundle,
            lang: Lang::JsQuickJs,
            side: Side::Guest,
            layout: OutputLayout::unified(),
        })
        .expect("generate external JavaScript guest bindings"),
    );
    for output in [&default, &external] {
        assert!(
            output
                .keys()
                .all(|path| !path.starts_with(Path::new("internal"))),
            "default/external generation must not emit internal paths: {output:#?}"
        );
        assert!(
            output
                .values()
                .all(|content| !content.contains("createInternalPluginGuestBridge")),
            "default/external generation must not depend on the internal profile"
        );
        assert!(
            output
                .values()
                .all(|content| !content.contains("POLYPLUG_MANIFEST")),
            "default/external generation must not emit the legacy manifest helper"
        );
    }
}

#[test]
fn distinct_javascript_internal_bundles_emit_collision_free_packages() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let first_api = temp.path().join("first.toml");
    let second_api = temp.path().join("second.toml");
    let first_bundle = temp.path().join("first-bundle.toml");
    let second_bundle = temp.path().join("second-bundle.toml");
    write_api(&first_api);
    write_api(&second_api);
    write_internal_bundle(
        &first_bundle,
        "javascript_first",
        "first.toml",
        "first_provider",
    );
    write_internal_bundle(
        &second_bundle,
        "javascript_second",
        "second.toml",
        "second_provider",
    );

    let first = generate_internal_javascript(InternalJavaScriptGenerateConfig {
        bundle_toml: first_bundle,
        out_dir: temp.path().join("out"),
        layout: Default::default(),
    })
    .expect("generate first internal bundle");
    let second = generate_internal_javascript(InternalJavaScriptGenerateConfig {
        bundle_toml: second_bundle,
        out_dir: temp.path().join("out"),
        layout: Default::default(),
    })
    .expect("generate second internal bundle");
    let mut paths: HashSet<PathBuf> = HashSet::new();
    for file in first.files.iter().chain(second.files.iter()) {
        assert!(
            paths.insert(file.path.clone()),
            "two JavaScript internal bundles emitted colliding path `{}`",
            file.path.display()
        );
    }
    let first_namespace = Path::new("internal").join(format!(
        "javascript_first-{:016x}",
        bundle_id("javascript_first")
    ));
    let second_namespace = Path::new("internal").join(format!(
        "javascript_second-{:016x}",
        bundle_id("javascript_second")
    ));
    assert!(paths.iter().any(|path| path.starts_with(&first_namespace)));
    assert!(paths.iter().any(|path| path.starts_with(&second_namespace)));
}

#[test]
fn split_javascript_guest_uses_external_canonical_domain_and_contract_modules() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    fs::write(
        &bundle,
        "[bundle]\nname = \"javascript_split\"\nversion = \"1.0\"\nloader = \"js-quickjs\"\nfile = \"plugin.js\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"javascript_provider\"\nimplements = [\"javascript.profile@1.0\"]\n",
    )
    .expect("write guest bundle TOML");

    let output = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::JsQuickJs,
        side: Side::Guest,
        layout: split_layout(),
    })
    .expect("generate split JavaScript guest");

    let domain = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/types.ts"))
        .expect("domain types file");
    assert_eq!(domain.partition, OutputPartition::DomainTypes);
    assert!(domain.content.contains("export const Mode"));
    assert!(domain.content.contains("export interface Outer"));
    assert!(!domain.content.contains("javascript_profile_many"));

    let contracts = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/contracts.ts"))
        .expect("guest contracts file");
    assert_eq!(contracts.partition, OutputPartition::GuestContracts);
    assert_eq!(contracts.references, vec![OutputPartition::DomainTypes]);
    assert!(contracts.content.contains("from '@test/javascript-domain'"));
    assert!(
        contracts
            .content
            .contains("export type javascript_profile_many")
    );

    let bindings = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/bindings.ts"))
        .expect("guest bindings file");
    assert_eq!(bindings.partition, OutputPartition::Bindings);
    assert_eq!(
        bindings.references,
        vec![
            OutputPartition::DomainTypes,
            OutputPartition::GuestContracts
        ]
    );
    assert!(bindings.content.contains("from '@test/javascript-domain'"));
    assert!(
        bindings
            .content
            .contains("from '@test/javascript-contracts'")
    );
    assert!(
        bindings
            .content
            .contains("javascript_provider_fn2_abi_wrapper")
    );
    assert!(bindings.content.contains("arg_item = { inner:"));
    assert!(bindings.content.contains("arg_values"));

    let interface = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/interface.ts"))
        .expect("guest interface module");
    let init = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/init.ts"))
        .expect("guest init module");
    let index = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/index.ts"))
        .expect("guest index module");
    for file in [interface, init, index] {
        assert!(file.content.contains("'./bindings'"));
        assert!(!file.content.contains("'./contracts'"));
    }
}

#[test]
fn split_javascript_internal_profile_keeps_one_domain_path_and_runtime_glue_in_bindings() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    write_internal_bundle(
        &bundle,
        "javascript_split_internal",
        "api.toml",
        "javascript_provider",
    );

    let output = generate_internal_javascript(InternalJavaScriptGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout: split_layout(),
    })
    .expect("generate split internal JavaScript profile");
    let namespace = Path::new("internal").join(format!(
        "javascript_split_internal-{:016x}",
        bundle_id("javascript_split_internal")
    ));
    let domain = output
        .files
        .iter()
        .find(|file| file.path == namespace.join("domain/types.ts"))
        .expect("single internal domain module");
    assert_eq!(domain.partition, OutputPartition::DomainTypes);
    assert!(domain.content.contains("export interface Outer"));

    let contracts = output
        .files
        .iter()
        .find(|file| file.path == namespace.join("guest/contracts.ts"))
        .expect("internal guest declarations");
    assert_eq!(contracts.partition, OutputPartition::GuestContracts);
    assert!(contracts.content.contains("from '@test/javascript-domain'"));
    assert!(contracts.content.contains("javascript_profile_many"));

    let bindings = output
        .files
        .iter()
        .find(|file| file.path == namespace.join("guest/bindings.ts"))
        .expect("internal guest ABI bindings");
    assert_eq!(
        bindings.references,
        vec![
            OutputPartition::DomainTypes,
            OutputPartition::GuestContracts
        ]
    );
    assert!(bindings.content.contains("from '@test/javascript-domain'"));
    assert!(
        bindings
            .content
            .contains("from '@test/javascript-contracts'")
    );
    assert!(
        bindings
            .content
            .contains("javascript_provider_javascript_profile_fn2_abi_wrapper")
    );

    let callers = output
        .files
        .iter()
        .find(|file| file.path == namespace.join("host/callers.ts"))
        .expect("internal host callers");
    assert!(callers.content.contains("from '@test/javascript-domain'"));
    assert!(callers.content.contains("createFromHandle"));

    let facade = output
        .files
        .iter()
        .find(|file| file.path == namespace.join("internal.ts"))
        .expect("internal facade");
    assert_eq!(facade.partition, OutputPartition::Bindings);
    assert!(facade.content.contains("from '@test/javascript-domain'"));
    assert!(facade.content.contains("from \"./guest/bindings.ts\""));
    assert!(!facade.content.contains("./guest/contracts.ts"));
}

#[test]
fn split_javascript_guest_bindings_typecheck_against_emitted_external_packages() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    fs::write(
        &bundle,
        "[bundle]\nname = \"javascript_typecheck\"\nversion = \"1.0\"\nloader = \"js-quickjs\"\nfile = \"plugin.js\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"javascript_provider\"\nimplements = [\"javascript.profile@1.0\"]\n",
    )
    .expect("write guest bundle TOML");

    let domain_root = temp.path().join("domain");
    let contracts_root = temp.path().join("contracts");
    let domain_import = file_url(&domain_root.join("guest/types.ts"));
    let contracts_import = file_url(&contracts_root.join("guest/contracts.ts"));
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root,
            import: ValidatedImport::parse(Lang::JsQuickJs, domain_import)
                .expect("valid external domain file URL"),
        },
        guest_contracts: OutputDestination::Emit {
            root: contracts_root,
            import: ValidatedImport::parse(Lang::JsQuickJs, contracts_import)
                .expect("valid external contract file URL"),
        },
    };
    let output = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::JsQuickJs,
        side: Side::Guest,
        layout,
    })
    .expect("generate externally split JavaScript guest");
    let bindings_root = temp.path().join("bindings");
    write_output(&output, &bindings_root).expect("write split JavaScript output");
    let bindings = bindings_root.join("guest/bindings.ts");
    let result = Command::new("deno")
        .args(["check", bindings.to_str().expect("UTF-8 bindings path")])
        .output()
        .expect("run deno check");
    assert!(
        result.status.success(),
        "deno check failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
