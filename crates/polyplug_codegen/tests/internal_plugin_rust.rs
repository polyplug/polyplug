#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{InternalRustGenerateConfig, generate_internal_rust, write_output};
use tempfile::TempDir;

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

#[test]
fn generated_internal_rust_same_contract_providers_dispatch_statefully_and_unload() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[plugin_contract]]\nname = \"platform.plugin\"\nversion = \"1.0\"\n\n[[plugin_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"generated_internal_plugin\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"first\"\nimplements = [\"platform.plugin@1.0\"]\n\n[[plugin]]\nname = \"second\"\nimplements = [\"platform.plugin@1.0\"]\n",
    )
    .expect("write internal bundle TOML");

    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("unused"),
    })
    .expect("generate Rust internal-plugin bindings");
    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write generated bindings");
    let generated_module_path = generated
        .files
        .iter()
        .find_map(|file| {
            let path = file.path.to_string_lossy();
            (path.starts_with("internal/")
                && path.ends_with("/mod.rs")
                && path.matches('/').count() == 2)
                .then_some(path.into_owned())
        })
        .expect("generated internal root module");

    fs::write(
        crate_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated_internal_plugin_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write consumer Cargo.toml");
    let consumer_source = format!("#[path = {generated_module_path:?}]\nmod generated;\n")
        + r#"use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use polyplug::Runtime;
use polyplug_guest::GuestError;

struct Provider {
    next: AtomicU32,
}

impl Provider {
    fn new(next: u32) -> Self {
        Self { next: AtomicU32::new(next) }
    }
}

impl generated::guest::domain::PlatformPluginContract for Provider {
    fn value(&self) -> Result<u32, GuestError> {
        Ok(self.next.fetch_add(1, Ordering::Relaxed))
    }
}

fn main() {
    let runtime = Arc::new(Runtime::builder().build().expect("build runtime"));
    let mut registration = generated::guest::init::register(
        Arc::clone(&runtime),
        generated::guest::domain::InternalProviders {
            first_platform_plugin: generated::guest::domain::InternalProviderFactory::new(|| -> Box<dyn generated::guest::domain::PlatformPluginContract> { Box::new(Provider::new(10)) }),
            second_platform_plugin: generated::guest::domain::InternalProviderFactory::new(|| -> Box<dyn generated::guest::domain::PlatformPluginContract> { Box::new(Provider::new(20)) }),
        },
    )
    .expect("register internal plugin");
    assert_eq!(registration.first_platform_plugin.value().expect("call first provider"), 10);
    assert_eq!(registration.first_platform_plugin.value().expect("preserve first provider state"), 11);
    assert_eq!(registration.second_platform_plugin.value().expect("call second provider"), 20);

    let bundle_id = registration.bundle_id;
    drop(registration);
    runtime.unload_bundle(bundle_id).expect("unload after callers tear down");
}
"#;
    fs::write(source_dir.join("main.rs"), consumer_source).expect("write consumer source");

    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&crate_root)
        .output()
        .expect("run generated consumer check");
    assert!(
        output.status.success(),
        "generated internal Rust bindings did not register, dispatch, and unload:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
