//! End-to-end proof for generated in-process Rust guest modules.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, Lang, RustGuestMode, Side, generate, generate_rust_guest, write_output,
};
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates directory")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}
fn write_api(path: &Path, contracts: &[&str]) {
    let mut source = String::new();
    for name in contracts {
        source.push_str("[[plugin_contract]]\n");
        source.push_str(&format!("name = \"{name}\"\nversion = \"1.0\"\n\n"));
        source.push_str("[[plugin_contract.functions]]\n");
        source.push_str("name = \"value\"\nreturn = \"u32\"\n\n");
    }
    fs::write(path, source).expect("write API TOML");
}

fn generate_bindings(api: &Path, guest_out: &Path, host_out: &Path, bundle_name: &str) {
    let guest = generate_rust_guest(
        GenerateConfig {
            api_toml: api.to_path_buf(),
            lang: Lang::Rust,
            side: Side::Guest,
            out_dir: guest_out.to_path_buf(),
        },
        RustGuestMode::InProcess {
            bundle_name: bundle_name.to_owned(),
        },
    )
    .expect("generate in-process Rust guest");
    write_output(&guest, guest_out).expect("write in-process guest");

    let host = generate(GenerateConfig {
        api_toml: api.to_path_buf(),
        lang: Lang::Rust,
        side: Side::Host,
        out_dir: host_out.to_path_buf(),
    })
    .expect("generate Rust host");
    write_output(&host, host_out).expect("write host callers");
}

fn write_consumer_manifest(root: &Path) {
    let workspace = workspace_root();
    let polyplug = workspace
        .join("crates/polyplug")
        .display()
        .to_string()
        .replace('\\', "/");
    let abi = workspace
        .join("crates/polyplug_abi")
        .display()
        .to_string()
        .replace('\\', "/");
    let guest = workspace
        .join("sdks/rust/guest")
        .display()
        .to_string()
        .replace('\\', "/");
    let utils = workspace
        .join("crates/polyplug_utils")
        .display()
        .to_string()
        .replace('\\', "/");
    let common = workspace
        .join("crates/polyplug_common")
        .display()
        .to_string()
        .replace('\\', "/");
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated_in_process_rust_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{polyplug}\" }}\npolyplug_abi = {{ path = \"{abi}\" }}\npolyplug_common = {{ path = \"{common}\" }}\npolyplug_guest = {{ path = \"{guest}\" }}\npolyplug_utils = {{ path = \"{utils}\" }}\n\n[workspace]\n"
        ),
    )
    .expect("write consumer Cargo.toml");
}

fn write_consumer_main(path: &Path) {
    let source = r#"
use std::sync::Arc;

use polyplug::{Runtime, error::RegistryError};
use polyplug_guest::{GuestError, HostContext};

#[path = "first_guest/guest/mod.rs"]
mod first_guest;
#[path = "first_host/mod.rs"]
mod first_host;
#[path = "second_guest/guest/mod.rs"]
mod second_guest;
#[path = "second_host/mod.rs"]
mod second_host;

struct FirstAlpha;
impl first_guest::contracts::EmbeddedAlphaGuestContract for FirstAlpha {
    fn value(&self) -> Result<u32, GuestError> {
        Ok(11)
    }
}

struct FirstShared;
impl first_guest::contracts::EmbeddedSharedGuestContract for FirstShared {
    fn value(&self) -> Result<u32, GuestError> {
        Ok(17)
    }
}

struct SecondBeta;
impl second_guest::contracts::EmbeddedBetaGuestContract for SecondBeta {
    fn value(&self) -> Result<u32, GuestError> {
        Ok(29)
    }
}

fn first_alpha(_host: HostContext) -> Box<dyn first_guest::contracts::EmbeddedAlphaGuestContract> {
    Box::new(FirstAlpha)
}

fn first_shared(_host: HostContext) -> Box<dyn first_guest::contracts::EmbeddedSharedGuestContract> {
    Box::new(FirstShared)
}

fn second_beta(_host: HostContext) -> Box<dyn second_guest::contracts::EmbeddedBetaGuestContract> {
    Box::new(SecondBeta)
}

fn main() {
    let runtime: Arc<Runtime> = Runtime::builder().build().expect("build runtime");
    let first_id = first_guest::init::register_in_process_bundle(
        &runtime,
        first_guest::interfaces::InProcessFactories {
            embedded_alpha: first_alpha,
            embedded_shared: first_shared,
        },
    )
    .expect("register first generated in-process bundle atomically");
    second_guest::init::register_in_process_bundle(
        &runtime,
        second_guest::interfaces::InProcessFactories {
            embedded_beta: second_beta,
        },
    )
    .expect("register second generated in-process bundle");

    assert!(runtime
        .find_guest_contract(first_host::host::types::EMBEDDED_SHARED_CONTRACT_ID, 0)
        .is_ok());

    let isolated: Arc<Runtime> = Runtime::builder().build().expect("build isolated runtime");
    assert!(isolated
        .find_guest_contract(first_host::host::types::EMBEDDED_ALPHA_CONTRACT_ID, 0)
        .is_err());

    let alpha_handle = runtime
        .find_guest_contract(first_host::host::types::EMBEDDED_ALPHA_CONTRACT_ID, 0)
        .expect("find alpha");
    let mut alpha = first_host::host::host_callers::EmbeddedAlphaContract::new(
        alpha_handle,
        Arc::clone(&runtime),
    )
    .expect("create alpha caller");
    assert_eq!(alpha.value().expect("call alpha"), 11);

    let beta_handle = runtime
        .find_guest_contract(second_host::host::types::EMBEDDED_BETA_CONTRACT_ID, 0)
        .expect("find beta");
    let mut beta = second_host::host::host_callers::EmbeddedBetaContract::new(
        beta_handle,
        Arc::clone(&runtime),
    )
    .expect("create beta caller");
    assert_eq!(beta.value().expect("call beta"), 29);

    let weak = Arc::downgrade(&runtime);
    drop(runtime);
    assert!(weak.upgrade().is_some(), "generated callers retain the runtime Arc");
    assert_eq!(beta.value().expect("call beta after original Arc drop"), 29);

    let runtime = weak.upgrade().expect("caller-owned runtime remains available");
    drop(alpha);
    runtime.unload_bundle(first_id).expect("unload first bundle");

    let stale = alpha_handle;
    assert!(matches!(
        runtime.resolve_guest_contract(stale),
        Err(RegistryError::StaleHandle { .. })
    ));

    first_guest::init::register_in_process_bundle(
        &runtime,
        first_guest::interfaces::InProcessFactories {
            embedded_alpha: first_alpha,
            embedded_shared: first_shared,
        },
    )
    .expect("re-register first generated bundle");
    let replacement_handle = runtime
        .find_guest_contract(first_host::host::types::EMBEDDED_ALPHA_CONTRACT_ID, 0)
        .expect("find re-registered alpha");
    let mut replacement = first_host::host::host_callers::EmbeddedAlphaContract::new(
        replacement_handle,
        Arc::clone(&runtime),
    )
    .expect("create re-registered alpha caller");
    assert_eq!(replacement.value().expect("call re-registered alpha"), 11);
}
"#;
    fs::write(path, source).expect("write consumer main");
}

#[test]
fn generated_in_process_modules_link_register_and_revalidate() {
    let temp = TempDir::new().expect("create temporary consumer");
    let source = temp.path().join("src");
    fs::create_dir_all(&source).expect("create consumer src");

    let first_api = temp.path().join("first.toml");
    let second_api = temp.path().join("second.toml");
    write_api(&first_api, &["embedded.alpha", "embedded.shared"]);
    write_api(&second_api, &["embedded.beta"]);
    generate_bindings(
        &first_api,
        &source.join("first_guest"),
        &source.join("first_host"),
        "generated-embedded-first",
    );
    generate_bindings(
        &second_api,
        &source.join("second_guest"),
        &source.join("second_host"),
        "generated-embedded-second",
    );
    write_consumer_manifest(temp.path());
    write_consumer_main(&source.join("main.rs"));

    let status = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--quiet")
        .arg("--target-dir")
        .arg(temp.path().join("target"))
        .arg("--manifest-path")
        .arg(temp.path().join("Cargo.toml"))
        .current_dir(workspace_root())
        .status()
        .expect("run generated in-process Rust consumer");
    assert!(
        status.success(),
        "generated in-process Rust consumer must succeed"
    );
}
