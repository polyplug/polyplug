#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, InternalRustGenerateConfig, Lang, OutputDestination, OutputLayout, Side,
    ValidatedImport, generate, generate_internal_rust, write_output,
};
use polyplug_utils::bundle_id;
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
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Cold\"\nvalue = \"1\"\n\n[[enum.variants]]\nname = \"Hot\"\nvalue = \"2\"\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"mode\", type = \"Mode\" }, { name = \"modes\", type = \"Array<Mode>\" }]\n\n[[guest_contract]]\nname = \"platform.plugin\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"cycle\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"generated_internal_plugin\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"first\"\nimplements = [\"platform.plugin@1.0\"]\n\n[[plugin]]\nname = \"second\"\nimplements = [\"platform.plugin@1.0\"]\n",
    )
    .expect("write internal bundle TOML");

    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: OutputLayout::unified(),
    })
    .expect("generate Rust internal-plugin bindings");
    let fingerprints: Vec<&str> = generated
        .files
        .iter()
        .filter_map(|file| {
            file.content
                .lines()
                .find(|line| line.starts_with("pub const INTERNAL_GENERATION_FINGERPRINT:"))
        })
        .collect();
    assert_eq!(
        fingerprints.len(),
        3,
        "domain declarations, guest contracts, and core bindings must expose one fingerprint"
    );
    assert!(
        fingerprints.windows(2).all(|pair| pair[0] == pair[1]),
        "split declarations and bindings must use the same generation fingerprint: {fingerprints:?}"
    );
    let interfaces = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/interfaces.rs"))
        .expect("generated internal interfaces")
        .content
        .as_str();
    assert!(
        interfaces.contains("super::types::Mode::Cold => super::domain::Mode::Cold")
            && interfaces.contains("super::domain::Mode::Hot => super::types::Mode::Hot"),
        "nominal enum values must cross the ABI boundary through explicit mappings: {interfaces}"
    );
    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write generated bindings");
    let generated_root = Path::new("internal").join(format!(
        "generated_internal_plugin-{:016x}",
        bundle_id("generated_internal_plugin")
    ));
    let generated_module_path = generated_root.join("mod.rs");
    assert!(
        generated
            .files
            .iter()
            .any(|file| file.path == generated_module_path),
        "generated internal root module"
    );

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

impl generated::guest::guest_contracts::PlatformPluginContract for Provider {
    fn cycle(
        &self,
        value: generated::guest::domain::Envelope,
    ) -> Result<generated::guest::domain::Envelope, GuestError> {
        let mode = if self.next.fetch_add(1, Ordering::Relaxed) % 2 == 0 {
            generated::guest::domain::Mode::Hot
        } else {
            generated::guest::domain::Mode::Cold
        };
        Ok(generated::guest::domain::Envelope {
            mode,
            modes: value.modes,
        })
    }
}

fn main() {
    let runtime = Arc::new(Runtime::builder().build().expect("build runtime"));
    let mut registration = generated::guest::init::register(
        Arc::clone(&runtime),
        generated::guest::interfaces::InternalProviders {
            first_platform_plugin: generated::guest::interfaces::InternalProviderFactory::new(|| -> Box<dyn generated::guest::guest_contracts::PlatformPluginContract> { Box::new(Provider::new(10)) }),
            second_platform_plugin: generated::guest::interfaces::InternalProviderFactory::new(|| -> Box<dyn generated::guest::guest_contracts::PlatformPluginContract> { Box::new(Provider::new(20)) }),
        },
    )
    .expect("register internal plugin");
    let input = generated::host::types::Envelope {
        mode: generated::host::types::Mode::Cold,
        modes: generated::host::types::ArrayOf_Mode {
            items: 0,
            len: 0,
        },
    };
    assert_eq!(
        registration
            .first_platform_plugin
            .cycle(&input)
            .expect("call first provider")
            .mode,
        generated::host::types::Mode::Hot
    );
    assert_eq!(
        registration
            .first_platform_plugin
            .cycle(&input)
            .expect("preserve first provider state")
            .mode,
        generated::host::types::Mode::Cold
    );
    assert_eq!(
        registration
            .second_platform_plugin
            .cycle(&input)
            .expect("call second provider")
            .mode,
        generated::host::types::Mode::Hot
    );
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

#[test]
fn generated_empty_internal_rust_bindings_compile_without_declaration_modules() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(&api, "").expect("write empty API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"empty_internal_plugin\"\nversion = \"1.0\"\napi = \"api.toml\"\n",
    )
    .expect("write empty internal bundle TOML");
    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Omit,
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("generate empty Rust internal bindings");
    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write binding-only output");
    let generated_root = Path::new("internal").join(format!(
        "empty_internal_plugin-{:016x}",
        bundle_id("empty_internal_plugin")
    ));
    let generated_module_path = generated_root.join("mod.rs");
    let guest_mod = fs::read_to_string(source_dir.join(&generated_root).join("guest/mod.rs"))
        .expect("read generated guest module");
    assert!(
        !guest_mod.contains("pub mod domain;") && !guest_mod.contains("pub mod guest_contracts;"),
        "omitted declaration partitions must not leave unusable module declarations: {guest_mod}"
    );
    fs::write(
        crate_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"empty_generated_internal_plugin_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write consumer Cargo.toml");
    fs::write(
        source_dir.join("main.rs"),
        format!("#[path = {generated_module_path:?}]\nmod generated;\nfn main() {{}}\n"),
    )
    .expect("write consumer source");
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&crate_root)
        .output()
        .expect("check binding-only consumer");
    assert!(
        output.status.success(),
        "generated empty Rust bindings did not compile without declarations:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_internal_rust_nested_peer_buffer_caller_compiles_and_runs() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[types]]\nname = \"Inner\"\nfields = [{ name = \"payload\", type = \"Buffer\" }]\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"inner\", type = \"Inner\" }]\n\n[[guest_contract]]\nname = \"peer.buffer\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"echo\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write nested peer API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"nested_peer_buffer\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[dependency]]\nkind = \"contract\"\ncontract = \"peer.buffer\"\nmin_version = \"1.0.0\"\n",
    )
    .expect("write nested peer bundle TOML");
    let declarations_root = temp.path().join("declarations");

    let generated = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: declarations_root.clone(),
                import: ValidatedImport::parse(Lang::Rust, "crate::domain").expect("domain import"),
            },
            guest_contracts: OutputDestination::Emit {
                root: declarations_root.clone(),
                import: ValidatedImport::parse(Lang::Rust, "crate::guest_contracts")
                    .expect("guest-contract import"),
            },
        },
    })
    .expect("generate nested peer Rust bindings");
    let peer_callers = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/peer_callers.rs"))
        .expect("peer caller output")
        .content
        .as_str();
    assert!(
        peer_callers.contains("Buffer { ptr:"),
        "nested Buffer peer conversion must construct an ABI Buffer: {peer_callers}"
    );
    assert!(
        peer_callers.contains("let host = self.host;"),
        "peer Buffer return cleanup must use the stored host field: {peer_callers}"
    );

    let crate_root = temp.path().join("consumer");
    let source_dir = crate_root.join("src");
    fs::create_dir_all(&source_dir).expect("create consumer source directory");
    write_output(&generated, &source_dir).expect("write nested peer bindings");
    let generated_root = Path::new("internal").join(format!(
        "nested_peer_buffer-{:016x}",
        bundle_id("nested_peer_buffer")
    ));
    let generated_module_path = generated_root.join("mod.rs");
    let domain_module_path = declarations_root
        .join(&generated_root)
        .join("guest/domain.rs");
    let guest_contracts_module_path = declarations_root
        .join(&generated_root)
        .join("guest/guest_contracts.rs");
    fs::write(
        crate_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"nested_peer_buffer_consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write consumer Cargo.toml");
    fs::write(
        source_dir.join("main.rs"),
        format!(
            "#[path = {domain_module_path:?}]\npub mod domain;\n#[path = {guest_contracts_module_path:?}]\npub mod guest_contracts;\n#[path = {generated_module_path:?}]\nmod generated;\nfn main() {{}}\n"
        ),
    )
    .expect("write consumer source");
    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&crate_root)
        .output()
        .expect("run nested peer consumer");
    assert!(
        output.status.success(),
        "generated nested peer Buffer caller did not compile and run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_ordinary_rust_guest_uses_external_domain_and_contract_paths() {
    let temp: TempDir = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    fs::write(
        &api,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Cold\"\nvalue = \"1\"\n\n[[enum.variants]]\nname = \"Hot\"\nvalue = \"2\"\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"mode\", type = \"Mode\" }]\n\n[[guest_contract]]\nname = \"demo.control\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"cycle\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write API TOML");
    let common = temp.path().join("common");
    let guest = temp.path().join("guest");
    fs::create_dir_all(common.join("src")).expect("create common source directory");
    fs::create_dir_all(guest.join("src")).expect("create guest source directory");
    fs::write(
        common.join("Cargo.toml"),
        format!(
            "[package]\nname = \"common\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug_guest = {{ path = \"{}\" }}\n",
            cargo_path("sdks/rust/guest"),
        ),
    )
    .expect("write common Cargo.toml");
    fs::write(
        common.join("src/lib.rs"),
        "pub mod domain {\n    #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n    pub enum Mode { Cold, Hot }\n    #[derive(Debug, Clone, PartialEq)]\n    pub struct Envelope { pub mode: Mode }\n}\npub mod guest_contracts {\n    use polyplug_guest::GuestError;\n    use super::domain::Envelope;\n    pub trait DemoControlContract: Send + Sync { fn cycle(&self, value: Envelope) -> Result<Envelope, GuestError>; }\n}\n",
    )
    .expect("write common source");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::domain").expect("domain import"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::guest_contracts")
                .expect("contract import"),
        },
    };
    let generated = generate(GenerateConfig {
        api_toml: api.clone(),
        lang: Lang::Rust,
        side: Side::Guest,
        layout,
    })
    .expect("generate split ordinary Rust guest");
    write_output(&generated, &guest.join("src/generated")).expect("write split guest bindings");
    let guest_mod =
        fs::read_to_string(guest.join("src/generated/guest/mod.rs")).expect("read guest module");
    assert!(
        guest_mod.contains("pub use common::domain;")
            && guest_mod.contains("pub use common::guest_contracts;"),
        "split guest must expose imported canonical declarations: {guest_mod}"
    );
    let interfaces = fs::read_to_string(guest.join("src/generated/guest/interfaces.rs"))
        .expect("read guest interfaces");
    assert!(
        interfaces.contains("Box<dyn common::guest_contracts::DemoControlContract>")
            && interfaces.contains("DemoControlDomainAdapter"),
        "guest factory and adapter must use the imported domain contract: {interfaces}"
    );
    fs::write(
        guest.join("Cargo.toml"),
        format!(
            "[package]\nname = \"ordinary_split_guest\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug_abi"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write guest Cargo.toml");
    fs::write(
        guest.join("src/main.rs"),
        "#[path = \"generated/guest/mod.rs\"]\nmod generated;\n\nuse common::domain::{Envelope, Mode};\nuse common::guest_contracts::DemoControlContract;\nuse polyplug_guest::{GuestError, HostContext};\n\nstruct Provider;\nimpl DemoControlContract for Provider {\n    fn cycle(&self, value: Envelope) -> Result<Envelope, GuestError> { Ok(Envelope { mode: match value.mode { Mode::Cold => Mode::Hot, Mode::Hot => Mode::Cold } }) }\n}\n#[unsafe(no_mangle)]\npub fn polyplug_create_demo_control(_host: HostContext) -> Box<dyn DemoControlContract> { Box::new(Provider) }\nfn main() { assert_eq!(Provider.cycle(Envelope { mode: Mode::Cold }).expect(\"cycle\").mode, Mode::Hot); }\n",
    )
    .expect("write guest source");
    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&guest)
        .output()
        .expect("run split guest");
    assert!(
        output.status.success(),
        "ordinary split Rust guest did not compile and run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let host = temp.path().join("host");
    fs::create_dir_all(host.join("src")).expect("create host source directory");
    let host_generated = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Rust,
        side: Side::Host,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::Rust, "common::domain")
                    .expect("domain import"),
            },
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("generate split ordinary Rust host");
    write_output(&host_generated, &host.join("src/generated")).expect("write split host bindings");
    let callers = fs::read_to_string(host.join("src/generated/host/host_callers.rs"))
        .expect("read host callers");
    assert!(
        callers.contains("value: &common::domain::Envelope")
            && callers.contains("Result<common::domain::Envelope, ContractError>"),
        "split host caller must expose canonical domain types: {callers}"
    );
    fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname = \"ordinary_split_host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\n\n[workspace]\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
        ),
    )
    .expect("write host Cargo.toml");
    fs::write(
        host.join("src/main.rs"),
        "#[path = \"generated/mod.rs\"]\nmod generated;\nuse common::domain::{Envelope, Mode};\nfn main() { let value = Envelope { mode: Mode::Cold }; assert_eq!(value.mode, Mode::Cold); }\n",
    )
    .expect("write host source");
    let output = Command::new("cargo")
        .arg("run")
        .current_dir(&host)
        .output()
        .expect("run split host");
    assert!(
        output.status.success(),
        "ordinary split Rust host did not compile and run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_internal_rust_three_crate_split_preserves_nominal_types_and_stateful_arrays() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    fs::write(
        &api,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Cold\"\nvalue = \"3\"\n\n[[enum.variants]]\nname = \"Warm\"\nvalue = \"7\"\n\n[[enum.variants]]\nname = \"Hot\"\nvalue = \"11\"\n\n[[enum]]\nname = \"Flags\"\nrepr = \"u32\"\nbitflag = true\n\n[[enum.variants]]\nname = \"Read\"\nvalue = \"1\"\n\n[[enum.variants]]\nname = \"Write\"\nvalue = \"1 << 1\"\n\n[[enum.variants]]\nname = \"Read_Write\"\nvalue = \"Read | Write\"\n\n[[types]]\nname = \"Row\"\nfields = [{ name = \"modes\", type = \"Array<Mode>\" }]\n\n[[types]]\nname = \"Envelope\"\nfields = [{ name = \"mode\", type = \"Mode\" }, { name = \"flags\", type = \"Flags\" }, { name = \"text\", type = \"StringView\" }, { name = \"payload\", type = \"Buffer\" }, { name = \"rows\", type = \"Array<Row>\" }]\n\n[[guest_contract]]\nname = \"platform.plugin\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"cycle\"\nparams = [{ name = \"value\", type = \"Envelope\" }]\nreturn = \"Envelope\"\n",
    )
    .expect("write split API TOML");
    fs::write(
        &bundle,
        "[bundle]\nname = \"split_internal_plugin\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"platform\"\nimplements = [\"platform.plugin@1.0\"]\n",
    )
    .expect("write split bundle TOML");
    let generated_root = Path::new("internal").join(format!(
        "split_internal_plugin-{:016x}",
        bundle_id("split_internal_plugin")
    ));

    let common = temp.path().join("common");
    let platform = temp.path().join("platform");
    let core = temp.path().join("core");
    for crate_root in [&common, &platform, &core] {
        fs::create_dir_all(crate_root.join("src")).expect("create crate source directory");
    }
    fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"common\", \"platform\", \"core\"]\nresolver = \"3\"\n",
    )
    .expect("write temporary workspace manifest");

    let common_generated_root = common.join("src/generated");
    let common_layout = OutputLayout {
        bindings: OutputDestination::Omit,
        domain_types: OutputDestination::Emit {
            root: common_generated_root.clone(),
            import: ValidatedImport::parse(Lang::Rust, "crate::domain")
                .expect("common domain import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: common_generated_root.clone(),
            import: ValidatedImport::parse(Lang::Rust, "crate::guest_contracts")
                .expect("common guest contract import"),
        },
    };
    let common_output = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle.clone(),
        layout: common_layout,
    })
    .expect("generate common declarations");
    write_output(&common_output, &common.join("src/ignored"))
        .expect("write common declaration partitions");
    let common_domain = common_generated_root
        .join(&generated_root)
        .join("guest/domain.rs");
    let common_contracts = common_generated_root
        .join(&generated_root)
        .join("guest/guest_contracts.rs");
    assert!(
        common_domain.is_file() && common_contracts.is_file(),
        "common must emit canonical domain and guest-contract declarations"
    );
    assert!(
        !common.join("src/ignored/guest/types.rs").exists(),
        "common must omit ABI binding partitions"
    );
    let common_domain_source =
        fs::read_to_string(&common_domain).expect("read generated common domain");
    assert_eq!(
        common_domain_source.matches("pub struct Envelope").count(),
        1,
        "common must own exactly one domain declaration"
    );
    fs::write(
        common.join("Cargo.toml"),
        format!(
            "[package]\nname = \"common\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npolyplug_guest = {{ path = \"{}\" }}\n",
            cargo_path("sdks/rust/guest"),
        ),
    )
    .expect("write common manifest");
    let common_domain_module_path = Path::new("generated")
        .join(&generated_root)
        .join("guest/domain.rs");
    let common_contracts_module_path = Path::new("generated")
        .join(&generated_root)
        .join("guest/guest_contracts.rs");
    fs::write(
        common.join("src/lib.rs"),
        format!(
            "#[path = {common_domain_module_path:?}]\npub mod domain;\n#[path = {common_contracts_module_path:?}]\npub mod guest_contracts;\n"
        ),
    )
    .expect("write common declarations module");

    fs::write(
        platform.join("Cargo.toml"),
        format!(
            "[package]\nname = \"platform\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\npolyplug_guest = {{ path = \"{}\" }}\n",
            cargo_path("sdks/rust/guest"),
        ),
    )
    .expect("write platform manifest");
    fs::write(
        platform.join("src/lib.rs"),
        "use std::sync::atomic::{AtomicUsize, Ordering};\n\nuse common::domain::{Envelope, Mode};\nuse common::guest_contracts::PlatformPluginContract;\nuse polyplug_guest::GuestError;\n\npub struct Platform {\n    calls: AtomicUsize,\n}\n\nimpl Platform {\n    pub fn new() -> Self {\n        Self { calls: AtomicUsize::new(0) }\n    }\n}\n\nimpl PlatformPluginContract for Platform {\n    fn cycle(&self, value: Envelope) -> Result<Envelope, GuestError> {\n        let mode = if self.calls.fetch_add(1, Ordering::Relaxed) % 2 == 0 {\n            Mode::Hot\n        } else {\n            Mode::Warm\n        };\n        Ok(Envelope { mode, ..value })\n    }\n}\n"
    )
    .expect("write platform implementation");

    let core_generated_root = core.join("src/generated");
    let core_layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::domain")
                .expect("core domain import"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Rust, "common::guest_contracts")
                .expect("core guest-contract import"),
        },
    };
    let core_output = generate_internal_rust(InternalRustGenerateConfig {
        bundle_toml: bundle,
        layout: core_layout,
    })
    .expect("generate core bindings");
    write_output(&core_output, &core_generated_root).expect("write core binding partition");
    assert!(
        !core_generated_root
            .join(&generated_root)
            .join("guest/domain.rs")
            .exists()
            && !core_generated_root
                .join(&generated_root)
                .join("guest/guest_contracts.rs")
                .exists(),
        "core must not emit duplicate declaration partitions"
    );
    let core_guest_mod = fs::read_to_string(
        core_generated_root
            .join(&generated_root)
            .join("guest/mod.rs"),
    )
    .expect("read generated core guest module");
    assert!(
        core_guest_mod.contains("pub use common::domain;")
            && core_guest_mod.contains("pub use common::guest_contracts;"),
        "core bindings must import canonical declarations: {core_guest_mod}"
    );
    fs::write(
        core.join("Cargo.toml"),
        format!(
            "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncommon = {{ path = \"../common\" }}\nplatform = {{ path = \"../platform\" }}\npolyplug = {{ path = \"{}\" }}\npolyplug_abi = {{ path = \"{}\" }}\npolyplug_common = {{ path = \"{}\" }}\npolyplug_guest = {{ path = \"{}\" }}\npolyplug_utils = {{ path = \"{}\" }}\n",
            cargo_path("crates/polyplug"),
            cargo_path("crates/polyplug_abi"),
            cargo_path("crates/polyplug_common"),
            cargo_path("sdks/rust/guest"),
            cargo_path("crates/polyplug_utils"),
        ),
    )
    .expect("write core manifest");
    let generated_module_path = Path::new("generated").join(&generated_root).join("mod.rs");
    fs::write(
        core.join("src/main.rs"),
        format!(
            "#[path = {generated_module_path:?}]\nmod generated;\n\nuse std::alloc::{{GlobalAlloc, Layout, System}};\nuse std::sync::Arc;\nuse std::sync::atomic::{{AtomicBool, AtomicUsize, Ordering}};\n\nuse common::domain::{{flags, Envelope, Mode, Row, INTERNAL_GENERATION_FINGERPRINT as DOMAIN_FINGERPRINT}};\nuse common::guest_contracts::INTERNAL_GENERATION_FINGERPRINT as CONTRACT_FINGERPRINT;\nuse platform::Platform;\nuse polyplug::Runtime;\n\nstatic BUFFER_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);\nstatic TRACK_BUFFER_ALLOCATIONS: AtomicBool = AtomicBool::new(false);\n\nstruct TrackingAllocator;\n\nunsafe impl GlobalAlloc for TrackingAllocator {{\n    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {{\n        let ptr = unsafe {{ System.alloc(layout) }};\n        if TRACK_BUFFER_ALLOCATIONS.load(Ordering::SeqCst) && layout.size() == 37 && !ptr.is_null() {{\n            BUFFER_ALLOCATIONS.fetch_add(1, Ordering::SeqCst);\n        }}\n        ptr\n    }}\n\n    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {{\n        if TRACK_BUFFER_ALLOCATIONS.load(Ordering::SeqCst) && layout.size() == 37 && !ptr.is_null() {{\n            BUFFER_ALLOCATIONS.fetch_sub(1, Ordering::SeqCst);\n        }}\n        unsafe {{ System.dealloc(ptr, layout) }};\n    }}\n}}\n\n#[global_allocator]\nstatic ALLOCATOR: TrackingAllocator = TrackingAllocator;\n\nfn main() {{\n    assert_eq!(DOMAIN_FINGERPRINT, CONTRACT_FINGERPRINT, \"common declarations must share one fingerprint\");\n    assert_eq!(DOMAIN_FINGERPRINT, generated::guest::interfaces::INTERNAL_GENERATION_FINGERPRINT, \"core bindings must retain the common declaration fingerprint\");\n\n    let runtime = Arc::new(Runtime::builder().build().expect(\"build runtime\"));\n    let mut registration = generated::guest::init::register(\n        Arc::clone(&runtime),\n        generated::guest::interfaces::InternalProviders {{\n            platform_platform_plugin: generated::guest::interfaces::InternalProviderFactory::new(|| -> Box<dyn common::guest_contracts::PlatformPluginContract> {{ Box::new(Platform::new()) }}),\n        }},\n    )\n    .expect(\"register platform provider\");\n\n    let input = Envelope {{\n        mode: Mode::Cold,\n        flags: flags::READ_WRITE,\n        text: \"canonical common input\".to_owned(),\n        payload: vec![0xA5; 37],\n        rows: vec![\n            Row {{ modes: vec![Mode::Cold, Mode::Warm] }},\n            Row {{ modes: vec![Mode::Hot] }},\n        ],\n    }};\n    TRACK_BUFFER_ALLOCATIONS.store(true, Ordering::SeqCst);\n    let first = registration.platform_platform_plugin.cycle(&input).expect(\"first stateful roundtrip\");\n    assert_eq!(first.mode, Mode::Hot);\n    assert_eq!(first.flags, flags::READ_WRITE);\n    assert_eq!(first.text, input.text);\n    assert_eq!(first.payload, input.payload);\n    assert_eq!(first.rows, input.rows);\n    drop(first);\n    assert_eq!(BUFFER_ALLOCATIONS.load(Ordering::SeqCst), 0, \"returned Buffer must be copied and released through HostApi\");\n    let second = registration.platform_platform_plugin.cycle(&input).expect(\"preserve platform state\");\n    assert_eq!(second.mode, Mode::Warm);\n    assert_eq!(second.flags, flags::READ_WRITE);\n    assert_eq!(second.text, input.text);\n    assert_eq!(second.payload, input.payload);\n    assert_eq!(second.rows, input.rows);\n    drop(second);\n    assert_eq!(BUFFER_ALLOCATIONS.load(Ordering::SeqCst), 0, \"every returned Buffer allocation must be balanced\");\n    TRACK_BUFFER_ALLOCATIONS.store(false, Ordering::SeqCst);\n\n    let bundle_id = registration.bundle_id;\n    drop(registration);\n    runtime.unload_bundle(bundle_id).expect(\"unload after callers tear down\");\n}}\n"
        ),
    )
    .expect("write core executable");

    let output = Command::new("cargo")
        .arg("run")
        .arg("-p")
        .arg("core")
        .current_dir(temp.path())
        .output()
        .expect("run split internal Rust workspace");
    assert!(
        output.status.success(),
        "three-crate split internal Rust workspace did not compile, roundtrip, and unload:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
