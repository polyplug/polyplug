// Integration tests for the polyplug_python loader's in-memory `BundleSource`
// support (`Code` and `Bytes`).
//
// `Code(String)` carries the plugin's Python module source directly, with no
// bundle directory. These tests load such a plugin through
// `Runtime::load_bundle_from_source`, then resolve its registered contract and
// assert the resolved interface's ABI metadata. They also verify that non-UTF-8
// `Bytes` are rejected with a structured error. See `CODE_PLUGIN_SRC` for why a
// live native call cannot be exercised from a pure-`ctypes` inline plugin.
//
// The inline plugin is intentionally `ctypes`-only (standard library): in-memory
// sources are single-file with no bundle directory, so a bundle-vendored
// generated SDK is not importable — only modules already on the interpreter's
// `sys.path` (the standard library) are reachable. `ctypes` is sufficient to
// reach the ABI, which is exactly how the existing path-based fixture works too.
#![allow(clippy::expect_used)]

use std::collections::HashMap;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime_builder::RuntimeBuilder;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractId;
use polyplug_python::PythonLoader;
use polyplug_utils::bundle_id;

/// Contract id the inline plugin registers under.
const CODE_CONTRACT_ID: u64 = 0x0BAD_F00D_1234_5678;

/// A self-contained Python plugin (ctypes only) that registers a native-dispatch
/// contract advertising one function.
///
/// # Dispatch limitation (why function 0 is a stub, not a live callback)
///
/// A native-dispatch function must have the frozen C signature
/// `extern "C" fn(GuestContractInstance, *const, *mut) -> AbiError`, returning a
/// 24-byte `AbiError` struct **by value**. `ctypes` cannot create a callback
/// (Python-implemented C function) whose return type is a struct of that size —
/// CPython raises `TypeError: invalid result type for callback function`. A real
/// Python guest therefore reaches a working native function through a *compiled* C
/// trampoline shipped in the bundle's generated SDK, which a single-file in-memory
/// source has no way to provide. Consequently an inline `Code`/`Bytes` plugin can
/// fully *register and resolve* a native contract (the source-only achievable
/// surface, and exactly what every path-based loader test asserts) but cannot host
/// a live native callback. The registered function-pointer slot here is a null
/// placeholder; the test asserts on the resolved interface metadata rather than
/// invoking the function.
///
/// All ctypes objects whose lifetime must outlive `polyplug_init` (the
/// function-pointer array, the descriptor and interface structs) are stashed as
/// attributes on the permanently-resident `ctypes` stdlib module, so they stay
/// alive for the interpreter's lifetime regardless of how the inline module object
/// itself is retained.
const CODE_PLUGIN_SRC: &str = r#"
import ctypes

class _StringView(ctypes.Structure):
    _fields_ = [("ptr", ctypes.c_void_p), ("len", ctypes.c_size_t)]

class _AbiError(ctypes.Structure):
    _fields_ = [("code", ctypes.c_uint32), ("message", _StringView)]

class _Version(ctypes.Structure):
    _fields_ = [
        ("major", ctypes.c_uint32),
        ("minor", ctypes.c_uint32),
        ("patch", ctypes.c_uint32),
    ]

class _PluginDescriptor(ctypes.Structure):
    _fields_ = [
        ("name",          _StringView),
        ("contract_name", _StringView),
        ("version_major", ctypes.c_uint32),
        ("version_minor", ctypes.c_uint32),
        ("version_patch", ctypes.c_uint32),
    ]

class _NativeDispatch(ctypes.Structure):
    _fields_ = [
        ("function_count", ctypes.c_uint32),
        ("functions",      ctypes.c_void_p),
    ]

class _VmDispatch(ctypes.Structure):
    _fields_ = [
        ("call",        ctypes.c_void_p),
        ("loader_data", ctypes.c_void_p),
    ]

class _DispatchMechanisms(ctypes.Union):
    _fields_ = [("native", _NativeDispatch), ("vm", _VmDispatch)]

class _GuestContractInterface(ctypes.Structure):
    _fields_ = [
        ("contract_id",      ctypes.c_uint64),
        ("contract_version", _Version),
        ("dispatch_type",    ctypes.c_uint32),  # 0 = Native, 1 = VM
        ("create_instance",  ctypes.c_void_p),
        ("destroy_instance", ctypes.c_void_p),
        ("dispatch",         _DispatchMechanisms),
    ]

# Native dispatch advertises one function. The slot is a null placeholder: a
# live native callback would need a 24-byte AbiError struct return, which ctypes
# callbacks cannot express (see the Rust-side doc comment). The test asserts on
# the resolved interface metadata, not on invoking the function.
_FUNCS = (ctypes.c_void_p * 1)()
_FUNCS[0] = None

_NAME_BYTES     = b"code_plugin\x00"
_CONTRACT_BYTES = b"code.contract\x00"

_DESC = _PluginDescriptor()
_DESC.name.ptr          = ctypes.cast(ctypes.c_char_p(_NAME_BYTES), ctypes.c_void_p).value
_DESC.name.len          = len(_NAME_BYTES) - 1
_DESC.contract_name.ptr = ctypes.cast(ctypes.c_char_p(_CONTRACT_BYTES), ctypes.c_void_p).value
_DESC.contract_name.len = len(_CONTRACT_BYTES) - 1
_DESC.version_major = 1
_DESC.version_minor = 0
_DESC.version_patch = 0

_INTERFACE = _GuestContractInterface()
_INTERFACE.contract_id            = 0x0BADF00D12345678
_INTERFACE.contract_version.major = 1
_INTERFACE.contract_version.minor = 0
_INTERFACE.contract_version.patch = 0
_INTERFACE.dispatch_type          = 0  # Native
_INTERFACE.create_instance        = None
_INTERFACE.destroy_instance       = None
_INTERFACE.dispatch.native.function_count = 1
_INTERFACE.dispatch.native.functions      = ctypes.cast(_FUNCS, ctypes.c_void_p).value

# Keep every object whose pointer the host will dereference alive for the
# interpreter's lifetime by anchoring it on the resident `ctypes` module.
ctypes._polyplug_code_plugin_anchor = (_FUNCS, _DESC, _INTERFACE)

_RegisterFn = ctypes.CFUNCTYPE(
    _AbiError,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_void_p,
)

class _HostApi(ctypes.Structure):
    _fields_ = [
        ("runtime", ctypes.c_void_p),
        ("register_guest_contract", _RegisterFn),
    ]

def polyplug_init(host_interface: int, _ctx: int) -> None:
    host = _HostApi.from_address(host_interface)
    host.register_guest_contract(
        ctypes.c_void_p(host_interface),
        ctypes.addressof(_DESC),
        ctypes.addressof(_INTERFACE),
    )
"#;

/// Build a `ManifestData` for an in-memory (sourceless) Python bundle.
fn inline_manifest(name: &str) -> ManifestData {
    ManifestData {
        id: bundle_id(name),
        name: name.to_owned(),
        runtime: "python".to_owned(),
        // `file` is ignored by the loader for in-memory sources, but the shared
        // `ManifestData::validate()` (run by `load_bundle_from_source` before
        // dispatch) requires a non-empty `file`, so a placeholder is supplied.
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

/// A `Code`-sourced plugin loads, registers its contract, and that contract
/// resolves to an interface carrying the correct ABI metadata.
///
/// "Resolve + assert correct results" is asserted at the interface level: the
/// resolved `GuestContractInterface` must carry the contract id, version, native
/// dispatch type, and advertised function count the inline plugin registered. A
/// live native *call* is not asserted because a pure-`ctypes` inline plugin cannot
/// host a native callback returning `AbiError` by value (see `CODE_PLUGIN_SRC`).
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

    // Resolve: the contract must be findable in the registry.
    let contract_id: GuestContractId = GuestContractId::from_u64(CODE_CONTRACT_ID);
    let handle: GuestContractHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("contract must be registered after inline Code load");

    // The interface must resolve to a non-null pointer.
    let interface_ptr: *const polyplug_abi::GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("registered contract must resolve to an interface");
    assert!(
        !interface_ptr.is_null(),
        "resolved interface must be non-null"
    );

    // SAFETY: `interface_ptr` is a non-null pointer to a registered, retire-not-drop
    // GuestContractInterface owned by the runtime; reading its fields is sound.
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.contract_id.id(),
        CODE_CONTRACT_ID,
        "resolved contract id must match the registered one"
    );
    assert_eq!(
        interface.contract_version.major, 1,
        "resolved contract major version must match"
    );
    assert_eq!(
        interface.dispatch_type,
        polyplug_abi::dispatch::DispatchType::Native,
        "inline plugin registered a native dispatch contract"
    );
    // SAFETY: dispatch_type == Native guarantees the `native` union variant is the
    // active one, so reading it is sound.
    let native: polyplug_abi::dispatch::NativeDispatch = unsafe { interface.dispatch.native };
    assert_eq!(
        native.function_count, 1,
        "inline plugin advertised exactly one native function"
    );
}

/// A `Bytes`-sourced plugin carrying valid UTF-8 source behaves identically to
/// `Code` — it loads and registers its contract.
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

    let contract_id: GuestContractId = GuestContractId::from_u64(CODE_CONTRACT_ID);
    assert!(
        runtime.registry().find(contract_id, 0).is_ok(),
        "contract must be registered after inline Bytes load"
    );
}

/// `Bytes` carrying invalid UTF-8 must fail with the unified
/// `LoaderError::InvalidSourceEncoding` — never a stringly-typed or panicking
/// error.
#[test]
fn bytes_source_invalid_utf8_returns_structured_error() {
    let runtime: std::sync::Arc<Runtime> = RuntimeBuilder::new()
        .loader(PythonLoader::default())
        .build()
        .expect("runtime build");

    let manifest: ManifestData = inline_manifest("bad_utf8_plugin");
    // 0xFF is never a valid UTF-8 byte.
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
            assert_eq!(loader, "python", "loader must be the Python runtime name");
            assert_eq!(source_kind, "bytes", "source_kind must be bytes");
            assert_eq!(
                bundle, "bad_utf8_plugin",
                "bundle must be the manifest bundle name"
            );
        }
        other => panic!("expected LoaderError::InvalidSourceEncoding, got: {other:?}"),
    }
}
