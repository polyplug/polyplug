//! Focused output contracts for generated Rust internal-plugin bindings.

#![allow(clippy::expect_used)]

use polyplug_utils::bundle_id;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::{
    GenerateConfig, GenerateOutput, GeneratedFile, InternalRustGenerateConfig, Lang,
    OutputDestination, OutputLayout, OutputPartition, PolyplugcError, Side, ValidatedImport,
    generate, generate_internal_rust, write_output,
};

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
        "[[types]]\nname = \"State\"\nfields = [{ name = \"value\", type = \"u32\" }]\n\n[[guest_contract]]\nname = \"platform.alpha\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n\n[[guest_contract]]\nname = \"platform.beta\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write API TOML");
}

fn write_internal_bundle(path: &Path) {
    fs::write(
        path,
        "[bundle]\nname = \"internal_profile_bundle\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"internal_alpha\"\nimplements = [\"platform.alpha@1.0\"]\n\n[[plugin]]\nname = \"internal_beta\"\nimplements = [\"platform.beta@1.0\"]\n",
    )
    .expect("write internal bundle TOML");
}

#[test]
fn unchanged_generate_config_literal_rejects_missing_external_acquisition_fields() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    write_internal_bundle(&bundle);

    let result = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::Rust,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    });
    assert!(matches!(
        result,
        Err(PolyplugcError::ValidationFailed { message })
            if message == "bundle.loader field is required"
    ));
}

#[test]
fn internal_rust_profile_accepts_artifactless_bundle_and_namespaces_both_binding_roles() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    write_internal_bundle(&bundle);

    let output = output_map(
        generate_internal_rust(InternalRustGenerateConfig {
            bundle_toml: bundle,
            layout: OutputLayout::unified(),
        })
        .expect("generate internal Rust profile"),
    );

    let namespace = Path::new("internal").join(format!(
        "internal_profile_bundle-{:016x}",
        bundle_id("internal_profile_bundle")
    ));
    assert!(
        output.keys().all(|path| path.starts_with(&namespace)),
        "every profile file must be bundle-identity namespaced: {output:#?}"
    );
    let interfaces = output
        .get(&namespace.join("guest").join("interfaces.rs"))
        .expect("generated guest provider bindings");
    let init = output
        .get(&namespace.join("guest").join("init.rs"))
        .expect("generated internal registration");
    let callers = output
        .get(&namespace.join("host").join("host_callers.rs"))
        .expect("generated host caller bindings");

    assert!(interfaces.contains("pub struct InternalProviders"));
    assert!(interfaces.contains("internal_provider_factory_internal_alpha_platform_alpha"));
    assert!(init.contains("impl RustGeneratedInternalPlugin for InternalRegistration"));
    assert!(init.contains("registrar.register_contract"));
    assert!(init.contains("runtime.register_generated_internal_plugin"));
    assert!(!init.contains("HostApi"));
    assert!(!init.contains("AbiError"));
    assert!(init.contains("pub internal_alpha_platform_alpha:"));
    assert!(init.contains("let callers = (|| -> Result<Registration, RuntimeError> {"));
    assert!(init.contains("runtime.unload_bundle(published.bundle_id)?;"));
    assert!(!init.contains("find_guest_contract_by_bundle"));
    assert!(!init.contains("manifest.file"));
    assert!(!init.contains("loader = \\\"\\\""));
    assert!(callers.contains("runtime: Arc<Runtime>"));
    assert!(callers.contains("if interface == self.interface {"));
    assert!(callers.contains("self.cached_revision = self.live_revision();"));
}

#[test]
fn two_internal_bundles_with_distinct_apis_have_collision_free_namespaces() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let first_api = temp.path().join("first-api.toml");
    let second_api = temp.path().join("second-api.toml");
    let first_bundle = temp.path().join("first-bundle.toml");
    let second_bundle = temp.path().join("second-bundle.toml");
    fs::write(
        &first_api,
        "[[guest_contract]]\nname = \"first.contract\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write first API");
    fs::write(
        &second_api,
        "[[guest_contract]]\nname = \"second.contract\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write second API");
    fs::write(
        &first_bundle,
        "[bundle]\nname = \"first_internal_bundle\"\nversion = \"1.0\"\napi = \"first-api.toml\"\n\n[[plugin]]\nname = \"first_provider\"\nimplements = [\"first.contract@1.0\"]\n",
    )
    .expect("write first bundle");
    fs::write(
        &second_bundle,
        "[bundle]\nname = \"second_internal_bundle\"\nversion = \"1.0\"\napi = \"second-api.toml\"\n\n[[plugin]]\nname = \"second_provider\"\nimplements = [\"second.contract@1.0\"]\n",
    )
    .expect("write second bundle");

    let first = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: first_bundle,
        layout: OutputLayout::unified(),
    })
    .expect("generate first internal bundle");
    let second = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: second_bundle,
        layout: OutputLayout::unified(),
    })
    .expect("generate second internal bundle");
    let mut paths = HashSet::new();
    for file in first.files.iter().chain(second.files.iter()) {
        assert!(
            paths.insert(file.path.clone()),
            "two internal bundles emitted colliding path `{}`",
            file.path.display()
        );
    }
    let first_namespace = Path::new("internal").join(format!(
        "first_internal_bundle-{:016x}",
        bundle_id("first_internal_bundle")
    ));
    let second_namespace = Path::new("internal").join(format!(
        "second_internal_bundle-{:016x}",
        bundle_id("second_internal_bundle")
    ));
    assert!(paths.iter().any(|path| path.starts_with(&first_namespace)));
    assert!(paths.iter().any(|path| path.starts_with(&second_namespace)));
}

#[test]
fn internal_profile_preserves_provider_multiplicity_and_disambiguates_guest_contract_symbols() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[guest_contract]]\nname = \"multi.alpha\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n\n[[guest_contract]]\nname = \"multi.beta\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write API");
    fs::write(
        &bundle,
        "[bundle]\nname = \"multi_profile\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"combined\"\nimplements = [\"multi.alpha@1.0\", \"multi.beta@1.0\"]\n\n[[plugin]]\nname = \"second\"\nimplements = [\"multi.alpha@1.0\"]\n",
    )
    .expect("write bundle");
    let output = output_map(
        generate_internal_rust(InternalRustGenerateConfig {
            bundle_toml: bundle,
            layout: OutputLayout::unified(),
        })
        .expect("generate profile"),
    );
    let init = output
        .values()
        .find(|content| content.contains("struct InternalRegistration"))
        .expect("profile init");
    let interfaces = output
        .values()
        .find(|content| content.contains("pub struct InternalProviders"))
        .expect("profile interfaces");
    assert!(
        init.contains("provides = [\\\"multi.alpha@1\\\", \\\"multi.beta@1\\\"]"),
        "manifest provides must be sorted and deduplicated by contract identity"
    );
    for symbol in [
        "combined_multi_alpha",
        "combined_multi_beta",
        "second_multi_alpha",
    ] {
        assert!(
            interfaces.contains(symbol),
            "missing disambiguated symbol {symbol}"
        );
        assert!(
            init.contains(symbol),
            "missing generated result field {symbol}"
        );
    }
}

#[test]
fn normalized_internal_namespace_includes_bundle_id_to_prevent_collisions() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    fs::write(
        &api,
        "[[guest_contract]]\nname = \"path.contract\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write API");
    let first_bundle = temp.path().join("first.toml");
    let second_bundle = temp.path().join("second.toml");
    fs::write(
        &first_bundle,
        "[bundle]\nname = \"path/a\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"first\"\nimplements = [\"path.contract@1.0\"]\n",
    )
    .expect("write first bundle");
    fs::write(
        &second_bundle,
        "[bundle]\nname = \"path?a\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"second\"\nimplements = [\"path.contract@1.0\"]\n",
    )
    .expect("write second bundle");
    let first = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: first_bundle,
        layout: OutputLayout::unified(),
    })
    .expect("generate first profile");
    let second = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: second_bundle,
        layout: OutputLayout::unified(),
    })
    .expect("generate second profile");
    let first_root = first
        .files
        .first()
        .expect("first generated file")
        .path
        .clone();
    let second_root = second
        .files
        .first()
        .expect("second generated file")
        .path
        .clone();
    assert_ne!(first_root, second_root);
    let mut combined = first.files;
    combined.extend(second.files);
    write_output(
        &GenerateOutput::from_files(Lang::Rust, OutputLayout::unified(), combined),
        temp.path(),
    )
    .expect("write distinct namespaces");
}

#[test]
fn output_writer_rejects_duplicate_paths_before_writing() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let output = GenerateOutput::from_files(
        Lang::Rust,
        OutputLayout::unified(),
        vec![
            GeneratedFile {
                path: Path::new("internal").join("same.rs"),
                content: "first".to_owned(),
                force_regenerate: false,
                partition: crate::OutputPartition::Bindings,
                references: Vec::new(),
            },
            GeneratedFile {
                path: Path::new("internal").join("same.rs"),
                content: "second".to_owned(),
                force_regenerate: false,
                partition: crate::OutputPartition::Bindings,
                references: Vec::new(),
            },
        ],
    );

    assert!(matches!(
        write_output(&output, temp.path()),
        Err(PolyplugcError::DuplicateOutputPath { .. })
    ));
    assert!(
        !temp.path().join("internal").join("same.rs").exists(),
        "duplicate rejection must happen before any write"
    );
}

#[test]
fn internal_rust_layout_emits_declarations_and_rebinds_private_bindings() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    write_internal_bundle(&bundle);
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: temp.path().join("domain"),
            import: ValidatedImport::parse(Lang::Rust, "common::domain")
                .expect("valid domain import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: temp.path().join("contracts"),
            import: ValidatedImport::parse(Lang::Rust, "common::contracts")
                .expect("valid contract import"),
        },
    };
    let output = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout,
    })
    .expect("generate split internal Rust profile");

    let domain = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("domain declaration file");
    assert!(domain.content.contains("pub struct"));
    let contracts = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::GuestContracts)
        .expect("guest contract declaration file");
    assert!(contracts.content.contains("pub trait"));
    assert!(contracts.content.contains("common::domain"));
    let bindings = output
        .files
        .iter()
        .find(|file| {
            file.partition == OutputPartition::Bindings
                && file.path.ends_with(Path::new("guest/interfaces.rs"))
        })
        .expect("private adapter bindings");
    assert!(bindings.content.contains("common::contracts"));
    let fingerprint = |content: &str| {
        content
            .lines()
            .find(|line| line.contains("INTERNAL_GENERATION_FINGERPRINT"))
            .map(str::to_owned)
            .expect("generation fingerprint")
    };
    assert_eq!(
        fingerprint(&domain.content),
        fingerprint(&contracts.content)
    );
    assert_eq!(
        fingerprint(&contracts.content),
        fingerprint(&bindings.content)
    );
}
