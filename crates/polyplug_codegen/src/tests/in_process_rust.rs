//! Focused output contracts for in-process Rust guest generation.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::{
    GenerateConfig, GenerateOutput, Lang, RustGuestMode, Side, generate, generate_rust_guest,
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
        "[[plugin_contract]]\nname = \"in_process.alpha\"\nversion = \"1.0\"\n\n[[plugin_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n\n[[plugin_contract]]\nname = \"in_process.beta\"\nversion = \"1.0\"\n\n[[plugin_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write API TOML");
}

fn config(path: &Path, side: Side) -> GenerateConfig {
    GenerateConfig {
        api_toml: path.to_path_buf(),
        lang: Lang::Rust,
        side,
        out_dir: path.with_extension("out"),
    }
}

#[test]
fn disk_mode_is_byte_identical_to_existing_rust_guest_generation() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    write_api(&api);

    let existing = output_map(generate(config(&api, Side::Guest)).expect("generate disk guest"));
    let explicit = output_map(
        generate_rust_guest(config(&api, Side::Guest), RustGuestMode::Disk)
            .expect("generate explicit disk guest"),
    );

    assert_eq!(
        existing, explicit,
        "Rust disk guest bytes must stay unchanged"
    );
}

#[test]
fn in_process_mode_uses_runtime_local_factories_and_canonical_descriptors() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    write_api(&api);

    let output = output_map(
        generate_rust_guest(
            config(&api, Side::Guest),
            RustGuestMode::InProcess {
                bundle_name: "in-process-output-contract".to_owned(),
            },
        )
        .expect("generate in-process guest"),
    );
    let interfaces = output
        .get(&PathBuf::from("guest/interfaces.rs"))
        .expect("in-process interfaces");
    let init = output
        .get(&PathBuf::from("guest/init.rs"))
        .expect("in-process init");
    let host = output_map(generate(config(&api, Side::Host)).expect("generate host"));
    let callers = host
        .get(&PathBuf::from("host/host_callers.rs"))
        .expect("generated host callers");

    assert!(interfaces.contains("pub struct InProcessFactories"));
    assert!(interfaces.contains("in_process_factory_in_process_alpha"));
    assert!(interfaces.contains("in_process_factory_in_process_beta"));
    assert!(!interfaces.contains("with_in_process_resident"));
    assert!(interfaces.contains("adapter_context: *mut c_void"));
    assert!(!interfaces.contains("OnceLock"));
    assert!(!interfaces.contains("polyplug_create_"));
    assert!(!interfaces.contains("no_mangle"));
    assert!(!interfaces.contains(concat!("Embedded", "Bundle")));
    assert!(!interfaces.contains(concat!("Embedded", "Contract")));
    assert!(!interfaces.contains(concat!("EMBEDDED_", "FACTORIES")));
    assert!(!interfaces.contains(concat!("install_", "embedded_factories")));
    assert!(!init.contains(concat!("register_", "embedded_bundle")));
    assert!(init.contains("pub fn register_in_process_bundle"));
    assert!(init.contains("ManifestData::parse_from_str(IN_PROCESS_MANIFEST)"));
    assert!(init.contains(
        "runtime.register_in_process_bundle(manifest, SupportedLanguage::Rust, resident, |host|"
    ));
    assert!(init.contains("let adapter_context: *mut c_void"));
    assert!(init.contains("register_guest_contract"));
    assert!(!init.contains("InProcessBundle"));
    assert!(!init.contains("InProcessContractRegistration"));
    assert!(!init.contains("polyplug_init"));
    assert!(!init.contains("no_mangle"));
    assert!(callers.contains("runtime: Arc<Runtime>"));
    assert!(callers.contains("new(handle: GuestContractHandle, runtime: Arc<Runtime>)"));
    assert!(callers.contains("self.runtime.as_context_ptr()"));
    assert!(!callers.contains("new(handle: GuestContractHandle, host: *const HostApi)"));
}
