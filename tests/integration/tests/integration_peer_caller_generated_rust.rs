//! Runtime test: the GENERATED rust peer caller executes inside a real dylib.
//!
//! `integration_peer_caller_rust` replicates the generated `peer_callers.rs`
//! logic inline — it proves the convention, not the generated code. This test
//! closes that gap: it loads the REAL example dylibs (`rust_validator` +
//! `rust_transformer`, built by `examples/build_all.sh`) through the runtime,
//! then drives the transformer's `polyplug_test_peer_validate` probe, which
//! calls the generated `PipelineValidatorContractPeer` (resolve + arena +
//! direct dispatch through the cached interface) compiled into `libtransformer.so`.
//!
//! Skip policy: when the example bundles have not been built the test logs an
//! explicit skip with the exact command to run (same convention as
//! `integration_csharp_generated.rs`) — never a silent pass.

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;

use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::RuntimeConfig;
use polyplug_abi::StringView;
use polyplug_native::{NativeConfig, NativeLoader};

/// dlopen the transformer image and drive its `polyplug_test_peer_validate`
/// probe — which runs the generated `PipelineValidatorContractPeer` (resolve +
/// direct dispatch through the cached interface). Returns the decoded result.
fn drive_validate_probe(
    host_abi: *const polyplug_abi::HostApi,
    transformer_dir: &std::path::Path,
) -> String {
    let plugin_path: PathBuf = transformer_dir.join("libtransformer.so");
    // SAFETY: the library is the freshly built trusted example plugin.
    let library: libloading::Library =
        unsafe { libloading::Library::new(&plugin_path) }.expect("transformer .so must dlopen");
    // SAFETY: the transformer exports this exact symbol and signature.
    let probe: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const polyplug_abi::HostApi, StringView, *mut StringView) -> u32,
    > = unsafe {
        library
            .get(b"polyplug_test_peer_validate")
            .expect("probe symbol must resolve")
    };
    let input: &str = "DECODED:name|value|42";
    let input_view = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view = StringView {
        ptr: core::ptr::null(),
        len: 0,
    };
    // SAFETY: probe is a valid extern fn; host_abi is the live runtime's; input
    // borrows live test data and out_view is a valid local slot.
    let code: u32 = unsafe { probe(host_abi, input_view, &mut out_view) };
    assert_eq!(
        code,
        AbiErrorCode::Ok as u32,
        "generated peer caller must dispatch Ok, got code={code}"
    );
    assert!(!out_view.ptr.is_null(), "out view must be populated");
    // SAFETY: out_view points at host-allocated UTF-8 written by the probe.
    let result: &str = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(out_view.ptr, out_view.len))
            .expect("result must be valid UTF-8")
    };
    result.to_owned()
}

/// Workspace-relative path to a built example bundle directory.
fn example_bundle_dir(bundle: &str) -> PathBuf {
    let manifest_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root: &std::path::Path = manifest_dir
        .parent()
        .and_then(|p: &std::path::Path| p.parent())
        .expect("workspace root from tests/integration");
    workspace_root.join("examples").join("plugins").join(bundle)
}

/// Returns the two bundle dirs, or logs an explicit skip and returns None.
fn bundles_or_skip() -> Option<(PathBuf, PathBuf)> {
    let validator: PathBuf = example_bundle_dir("rust_validator");
    let transformer: PathBuf = example_bundle_dir("rust_transformer");
    for (name, dir) in [
        ("rust_validator", &validator),
        ("rust_transformer", &transformer),
    ] {
        if !dir.join("manifest.toml").exists() {
            println!(
                "SKIPPED: example bundle `{name}` not built at {} — run `bash examples/build_all.sh` first",
                dir.display()
            );
            return None;
        }
    }
    if !transformer.join("libtransformer.so").exists() {
        println!(
            "SKIPPED: libtransformer.so not built at {} — run `bash examples/build_all.sh` first",
            transformer.display()
        );
        return None;
    }
    Some((validator, transformer))
}

#[test]
fn generated_rust_peer_caller_validates_through_real_dylibs() {
    let (validator_dir, transformer_dir): (PathBuf, PathBuf) = match bundles_or_skip() {
        Some(dirs) => dirs,
        None => return,
    };

    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(NativeLoader::new(NativeConfig::default()))
        .build()
        .expect("runtime build must succeed");

    // The transformer declares a contract dependency on pipeline.Validator —
    // load the provider first.
    runtime
        .load_bundle(&validator_dir)
        .expect("rust_validator bundle must load");
    runtime
        .load_bundle(&transformer_dir)
        .expect("rust_transformer bundle must load");

    // The probe receives the runtime's live HostApi pointer — the guest holds no
    // host statics — and runs the generated peer (resolve + direct dispatch).
    assert_eq!(
        drive_validate_probe(runtime.host_abi(), &transformer_dir),
        "VALID:name|value|42",
        "generated peer caller must return the validator's response"
    );
}

/// A PROVIDER hot-reload must not break the generated peer caller. The
/// transformer dispatches to the validator directly through the cached interface
/// (no host-mediated `call_guest_method`). Reloading the validator republishes
/// the registry and epoch-reclaims the old interface; the generated peer must
/// resolve the swapped-in interface and still dispatch correctly — never
/// touching the reclaimed one. This exercises the same revision-counter
/// revalidation that `integration_host_caller_revalidate` proves for the
/// host→guest caller, but through the real generated PEER path end to end.
#[test]
fn generated_rust_peer_caller_survives_validator_reload() {
    let (validator_dir, transformer_dir): (PathBuf, PathBuf) = match bundles_or_skip() {
        Some(dirs) => dirs,
        None => return,
    };

    let config: RuntimeConfig = RuntimeConfig {
        hot_reload_enabled: true,
        ..RuntimeConfig::default()
    };
    let runtime: Arc<Runtime> = Runtime::builder()
        .config(config)
        .loader(NativeLoader::new(NativeConfig::default()))
        .build()
        .expect("runtime build must succeed");

    runtime
        .load_bundle(&validator_dir)
        .expect("rust_validator bundle must load");
    runtime
        .load_bundle(&transformer_dir)
        .expect("rust_transformer bundle must load");

    // Before reload: the generated peer direct-dispatches to the validator.
    assert_eq!(
        drive_validate_probe(runtime.host_abi(), &transformer_dir),
        "VALID:name|value|42",
        "peer dispatch must work before reload"
    );

    // Hot-reload the PROVIDER. `reload_bundle` takes the bundle directory (it
    // re-reads manifest.toml); this swaps a new validator interface into the same
    // registry slot, bumps the revision, and epoch-reclaims the old interface.
    runtime
        .reload_bundle(&validator_dir)
        .expect("rust_validator must hot-reload while the transformer depends on it");

    // After reload: a fresh resolve inside the generated peer picks up the
    // swapped-in interface and dispatches correctly through it.
    assert_eq!(
        drive_validate_probe(runtime.host_abi(), &transformer_dir),
        "VALID:name|value|42",
        "peer dispatch must still work after the provider hot-reloads"
    );
}
