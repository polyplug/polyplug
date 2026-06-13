// Integration tests for the polyplug_python loader's in-memory `BundleSource`
// support (`Code` and `Bytes`) under the VM-dispatch model.
//
// `Code(String)` / `Bytes(Vec<u8>)` carry the plugin's Python module source
// directly, with no bundle directory. These tests load such a plugin through
// `Runtime::load_bundle_from_source`, resolve its registered contract, assert
// the resolved interface is VM dispatch, and exercise a live VM call. They also
// verify that non-UTF-8 `Bytes` are rejected with a structured error.
//
// The inline plugin is `ctypes`-only (standard library): in-memory sources are
// single-file with no bundle directory, so a bundle-vendored generated SDK is
// not importable — only modules already on `sys.path` (the standard library) are
// reachable. `ctypes` is sufficient to write into the `out` pointer the VM
// dispatcher forwards as an int.
#![allow(clippy::expect_used)]

use std::collections::HashMap;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime_builder::RuntimeBuilder;
use polyplug_abi::AbiError;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::dispatch::DispatchType;
use polyplug_python::PythonLoader;
use polyplug_utils::GuestContractId;
use polyplug_utils::bundle_id;

/// A self-contained Python plugin (ctypes only) that registers one VM-dispatch
/// contract whose function 0 writes 0x7B into the i32 at `out`.
const CODE_PLUGIN_SRC: &str = r#"
import ctypes

def _fn0(args_ptr, out_ptr, arena_ptr):
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int32))[0] = 0x7B

def polyplug_init(host_interface: int, _ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {
            "contract": "code.contract@1",
            "plugin_name": "code_plugin",
            "functions": [_fn0],
        },
    ]
"#;

/// Contract id the inline plugin registers under (`code.contract@1`).
fn code_contract_id() -> u64 {
    GuestContractId::new("code.contract", 1).id()
}

/// Build a `ManifestData` for an in-memory (sourceless) Python bundle.
fn inline_manifest(name: &str) -> ManifestData {
    ManifestData {
        id: bundle_id(name),
        name: name.to_owned(),
        loader: "python".to_owned(),
        // `file` is ignored for in-memory sources, but `ManifestData::validate()`
        // (run by `load_bundle_from_source`) requires a non-empty placeholder.
        file: "<inline>".to_owned(),
        path: std::path::PathBuf::new(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

/// A `Code`-sourced plugin loads, registers a VM-dispatch contract, and that
/// contract dispatches a live VM call that writes into `out`.
#[test]
fn code_source_loads_resolves_and_dispatches() {
    let runtime: std::sync::Arc<Runtime> = RuntimeBuilder::new()
        .loader(PythonLoader::default())
        .build()
        .expect("runtime build");

    let manifest: ManifestData = inline_manifest("code_plugin");
    let result: Result<(), RuntimeError> =
        runtime.load_bundle_from_source(manifest, BundleSource::Code(CODE_PLUGIN_SRC.to_owned()));
    assert!(result.is_ok(), "inline Code load failed: {result:?}");

    let contract_id: GuestContractId = GuestContractId::from_u64(code_contract_id());
    let handle: GuestContractHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("contract must be registered after inline Code load");

    let interface_ptr: *const polyplug_abi::GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("registered contract must resolve to an interface");
    assert!(
        !interface_ptr.is_null(),
        "resolved interface must be non-null"
    );

    // SAFETY: runtime-owned interface leaked for the runtime lifetime; reading its fields is sound.
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.contract_id.id(),
        code_contract_id(),
        "resolved contract id must match the registered one"
    );
    assert_eq!(
        interface.dispatch_type,
        DispatchType::VirtualMachine,
        "inline plugin registered a VM dispatch contract"
    );

    // Live VM call: fn 0 writes 0x7B into out.
    // SAFETY: vm union arm is active (dispatch_type == VirtualMachine); loader_data
    // wraps a live PythonLoaderData; out points at a valid i32.
    let vm: polyplug_abi::dispatch::vm_dispatch::VmDispatch = unsafe { interface.dispatch.vm };
    let mut out_buf: i32 = 0;
    let mut err: AbiError = AbiError::ok();
    // SAFETY: see above — out is valid, args/arena are null and ignored.
    unsafe {
        (vm.call)(
            vm.loader_data,
            GuestContractInstance::null(),
            0,
            core::ptr::null(),
            &mut out_buf as *mut i32 as *mut (),
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        );
    }
    assert!(
        err.is_ok(),
        "vm dispatch should return Ok, got code {}",
        err.code
    );
    assert_eq!(out_buf, 0x7B, "callable must have written 0x7B into out");
}

/// A `Bytes`-sourced plugin carrying valid UTF-8 behaves identically to `Code`.
#[test]
fn bytes_source_valid_utf8_loads() {
    let runtime: std::sync::Arc<Runtime> = RuntimeBuilder::new()
        .loader(PythonLoader::default())
        .build()
        .expect("runtime build");

    let manifest: ManifestData = inline_manifest("bytes_plugin");
    let bytes: Vec<u8> = CODE_PLUGIN_SRC.as_bytes().to_vec();
    let result: Result<(), RuntimeError> =
        runtime.load_bundle_from_source(manifest, BundleSource::Bytes(bytes));
    assert!(result.is_ok(), "inline Bytes load failed: {result:?}");

    let contract_id: GuestContractId = GuestContractId::from_u64(code_contract_id());
    assert!(
        runtime.registry().find(contract_id, 0).is_ok(),
        "contract must be registered after inline Bytes load"
    );
}

/// `Bytes` carrying invalid UTF-8 must fail with `LoaderError::InvalidSourceEncoding`.
#[test]
fn bytes_source_invalid_utf8_returns_structured_error() {
    let runtime: std::sync::Arc<Runtime> = RuntimeBuilder::new()
        .loader(PythonLoader::default())
        .build()
        .expect("runtime build");

    let manifest: ManifestData = inline_manifest("bad_utf8_plugin");
    let bytes: Vec<u8> = vec![0x70, 0x79, 0xFF, 0xFE, 0x00];
    let err: RuntimeError = runtime
        .load_bundle_from_source(manifest, BundleSource::Bytes(bytes))
        .expect_err("invalid UTF-8 bytes must be rejected");
    match err {
        RuntimeError::Loader(LoaderError::InvalidSourceEncoding {
            loader,
            source_kind,
            bundle,
        }) => {
            assert_eq!(loader, "python");
            assert_eq!(source_kind, "bytes");
            assert_eq!(bundle, "bad_utf8_plugin");
        }
        other => panic!("expected LoaderError::InvalidSourceEncoding, got: {other:?}"),
    }
}
