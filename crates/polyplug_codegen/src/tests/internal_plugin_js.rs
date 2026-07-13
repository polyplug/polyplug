//! Focused output contracts for internal JavaScript binding generation.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use polyplug_utils::bundle_id;

use crate::GenerateConfig;
use crate::GenerateOutput;
use crate::InternalJavaScriptGenerateConfig;
use crate::Lang;
use crate::Side;
use crate::generate;
use crate::generate_internal_javascript;

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
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Fast\"\nvalue = \"1\"\n\n[[types]]\nname = \"Inner\"\nfields = [{ name = \"name\", type = \"StringView\" }]\n\n[[types]]\nname = \"Outer\"\nfields = [{ name = \"inner\", type = \"Inner\" }, { name = \"payload\", type = \"Buffer\" }]\n\n[[plugin_contract]]\nname = \"javascript.profile\"\nversion = \"1.0\"\n\n[[plugin_contract.functions]]\nname = \"scalar\"\nparams = [{ name = \"value\", type = \"u32\" }]\nreturn = \"u32\"\n\n[[plugin_contract.functions]]\nname = \"text\"\nparams = [{ name = \"value\", type = \"StringView\" }]\nreturn = \"StringView\"\n\n[[plugin_contract.functions]]\nname = \"many\"\nparams = [{ name = \"mode\", type = \"Mode\" }, { name = \"item\", type = \"Outer\" }, { name = \"values\", type = \"Array<u32>\" }]\nreturn = \"Outer\"\n\n[[plugin_contract.functions]]\nname = \"bytes\"\nparams = [{ name = \"value\", type = \"Buffer\" }]\nreturn = \"Buffer\"\n",
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
            out_dir: temp.path().join("host-out"),
        })
        .expect("generate default JavaScript host bindings"),
    );
    let external = output_map(
        generate(GenerateConfig {
            api_toml: bundle,
            lang: Lang::JsQuickJs,
            side: Side::Guest,
            out_dir: temp.path().join("guest-out"),
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
    })
    .expect("generate first internal bundle");
    let second = generate_internal_javascript(InternalJavaScriptGenerateConfig {
        bundle_toml: second_bundle,
        out_dir: temp.path().join("out"),
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
