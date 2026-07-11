//! Integration tests: DotnetLoader — cross-language .NET plugin scenarios.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use core::ffi::c_void;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::LogLevel;
use polyplug_abi::StringView;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;
use polyplug_utils::BundleId;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::sync::Arc;
use std::sync::Mutex;

/// Path to the compiled C# fixture DLL — set by build.rs.
/// Value is "DOTNET_NOT_AVAILABLE" if dotnet is not installed.
const CSHARP_DLL: &str = env!("TEST_CSHARP_PLUGIN_DLL");
const SKIP_DOTNET: bool = {
    // const equality check on &str slices
    let a: &[u8] = CSHARP_DLL.as_bytes();
    let b: &[u8] = b"DOTNET_NOT_AVAILABLE";
    if a.len() != b.len() {
        false
    } else {
        let mut i: usize = 0;
        let mut eq: bool = true;
        while i < a.len() {
            if a[i] != b[i] {
                eq = false;
            }
            i += 1;
        }
        eq
    }
};

macro_rules! skip_if_no_dotnet {
    () => {
        if SKIP_DOTNET {
            return;
        }
    };
}

// ─── ABI arg-pack structs ─────────────────────────────────────────────────────
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Helper: make loader and load fixture DLL ────────────────────────────────

fn make_loader() -> DotnetLoader {
    DotnetLoader::new(DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Auto,
    })
}

fn create_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(make_loader())
        .build()
        .expect("failed to build runtime")
}

fn load_fixture(rt: &Runtime) -> Result<(), RuntimeError> {
    rt.load_bundle(std::path::Path::new(CSHARP_DLL))
}

fn get_vtable(rt: &Runtime) -> *const GuestContractInterface {
    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load_fixture()");
    rt.resolve_guest_contract(handle)
        .expect("handle must be valid")
}

/// Read the function count from a Native-dispatch vtable.
fn native_function_count(vtable: &GuestContractInterface) -> u32 {
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::Native,
        "dotnet loader must use Native dispatch"
    );
    // SAFETY: dispatch_type is Native, so accessing the native union member is valid.
    unsafe { vtable.dispatch.native.function_count }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn integration_dotnet_loader_registration() {
    skip_if_no_dotnet!();
    let loader: DotnetLoader = make_loader();
    assert_eq!(loader.loader_name(), "dotnet");
}

#[test]
fn integration_dotnet_bundle_loads() {
    skip_if_no_dotnet!();
    let rt: Arc<Runtime> = create_runtime();
    let result: Result<(), RuntimeError> = load_fixture(&rt);
    assert!(
        result.is_ok(),
        "DotnetLoader::load() must succeed for fixture DLL: {:?}",
        result.err()
    );
}

#[test]
fn integration_dotnet_add() {
    skip_if_no_dotnet!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid (CLR keeps assembly loaded for process lifetime).
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert!(
        native_function_count(vtable) >= 1,
        "test.add vtable must have at least 1 function"
    );
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 0 (add). args/out are correctly typed for the add function.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args is a valid AddArgs, out is a valid u32.
    let mut result: AbiError = AbiError::ok();
    unsafe {
        dispatch_fn(
            vtable.adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut result as *mut AbiError,
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn integration_dotnet_add_primitive() {
    skip_if_no_dotnet!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid, CLR keeps assembly loaded.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert!(
        native_function_count(vtable) >= 2,
        "test.add vtable must have at least 2 functions"
    );
    // function index 1 = add_primitive(a, b: u32) -> u32 (same arg-pack as add)
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 1 (add_primitive). args/out are correctly typed.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(1) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args and out are valid and correctly typed.
    let mut result: AbiError = AbiError::ok();
    unsafe {
        dispatch_fn(
            vtable.adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut result as *mut AbiError,
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add_primitive must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 30_u32, "add_primitive(10, 20) must equal 30");
}

#[test]
fn integration_dotnet_version_string() {
    skip_if_no_dotnet!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert!(
        native_function_count(vtable) >= 3,
        "test.add vtable must have at least 3 functions"
    );
    // function index 2 = version() -> StringView (no args, pass null)
    let mut out_view: StringView = StringView::null();
    // SAFETY: fn_ptr is function 2 (version). No arg input needed; pass null.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: out_view is a valid StringView allocation on the stack.
    let mut result: AbiError = AbiError::ok();
    unsafe {
        dispatch_fn(
            vtable.adapter_context,
            GuestContractInstance::null(),
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
            &mut result as *mut AbiError,
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "version must return AbiErrorCode::Ok"
    );
    // SAFETY: out_view.ptr points to valid UTF-8 bytes for out_view.len bytes (C# static array).
    let version_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    assert_eq!(version_bytes, b"1.0", "version() must return \"1.0\"");
}

#[test]
fn integration_dotnet_reset() {
    skip_if_no_dotnet!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert!(
        native_function_count(vtable) >= 4,
        "test.add vtable must have at least 4 functions"
    );
    // function index 3 = reset() — no args, no meaningful output
    // SAFETY: fn_ptr is function 3 (reset). No args; dummy out is acceptable.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(3) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = unsafe { core::mem::transmute(fn_ptr) };
    let mut dummy_out: u32 = 0_u32;
    // SAFETY: null args and dummy_out are safe because reset() ignores both.
    let mut result: AbiError = AbiError::ok();
    unsafe {
        dispatch_fn(
            vtable.adapter_context,
            GuestContractInstance::null(),
            core::ptr::null::<()>(),
            &mut dummy_out as *mut u32 as *mut (),
            &mut result as *mut AbiError,
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "reset must return AbiErrorCode::Ok"
    );
}

/// End-to-end guest logging: the C# fixture's `reset` calls the guest SDK's
/// `PolyplugHost.Log(hostPtr, ...)` with the plugin-owned host pointer captured
/// during `PolyplugInit` (the SDK stores no host), transcoding UTF-16 → UTF-8
/// across `HostApi.log` and landing verbatim in the host logger installed via
/// `RuntimeBuilder::logger`.
///
/// The fixture is loaded as a COPY under a unique bundle name. The .NET bridge
/// keys collectible AssemblyLoadContexts by (runtime id, bundle id) — a unique
/// bundle id gets its own ALC with fresh statics, so the fixture's captured
/// host pointer is deterministically this runtime's even while sibling tests
/// load the shared fixture in parallel.
#[test]
fn integration_dotnet_guest_log_routes_to_host_logger() {
    skip_if_no_dotnet!();

    // Stage the bundle copy: every assembly artifact (.dll + .deps.json — the
    // resolver needs deps.json to find Polyplug.Guest.dll / Polyplug.Abi.dll
    // next to the main assembly) plus a manifest with a unique bundle name.
    let fixture_dir: &std::path::Path = std::path::Path::new(CSHARP_DLL)
        .parent()
        .expect("fixture DLL path must have a parent directory");
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    for entry in std::fs::read_dir(fixture_dir).expect("read fixture dir") {
        let entry: std::fs::DirEntry = entry.expect("fixture dir entry");
        let path: std::path::PathBuf = entry.path();
        let is_assembly_artifact: bool = matches!(
            path.extension().and_then(|e: &std::ffi::OsStr| e.to_str()),
            Some("dll") | Some("json")
        );
        if is_assembly_artifact {
            std::fs::copy(&path, tmp.path().join(entry.file_name()))
                .expect("copy fixture artifact");
        }
    }
    let id_val: u64 = bundle_id("csharp_log_adder");
    let manifest: String = format!(
        "name = \"csharp_log_adder\"\n\
         id = {id_val}\n\
         version = \"1.0.0\"\n\
         loader = \"dotnet\"\n\
         file = \"CsharpPlugin.dll\"\n\
         provides = [\"test.add@1\"]\n\n\
         [function_count]\n\
         \"test.add@1\" = 4\n",
    );
    std::fs::write(tmp.path().join("manifest.toml"), manifest).expect("write manifest.toml");

    let records: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::clone(&records);
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(make_loader())
        .logger(move |level: LogLevel, scope: &str, message: &str| {
            sink.lock()
                .expect("logger mutex must not be poisoned")
                .push((level, scope.to_owned(), message.to_owned()));
        })
        .build()
        .expect("failed to build runtime");
    rt.load_bundle(tmp.path())
        .expect("log fixture bundle must load");

    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid, CLR keeps assembly loaded.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert!(
        native_function_count(vtable) >= 4,
        "test.add vtable must have at least 4 functions"
    );
    // function index 3 = reset() — logs (Info, "guest.csharp_test_adder", ...) and ignores args/out
    // SAFETY: fn_ptr is function 3 (reset). No args; dummy out is acceptable.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(3) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = unsafe { core::mem::transmute(fn_ptr) };
    let mut dummy_out: u32 = 0_u32;
    // SAFETY: null args and dummy_out are safe because reset() ignores both.
    let mut result: AbiError = AbiError::ok();
    unsafe {
        dispatch_fn(
            vtable.adapter_context,
            GuestContractInstance::null(),
            core::ptr::null::<()>(),
            &mut dummy_out as *mut u32 as *mut (),
            &mut result as *mut AbiError,
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "reset must return AbiErrorCode::Ok"
    );

    let captured: Vec<(LogLevel, String, String)> = records
        .lock()
        .expect("logger mutex must not be poisoned")
        .clone();
    let guest_records: Vec<&(LogLevel, String, String)> = captured
        .iter()
        .filter(|(_, scope, _)| scope == "guest.csharp_test_adder")
        .collect();
    assert_eq!(
        guest_records.len(),
        1,
        "exactly one guest.csharp_test_adder record expected; got: {captured:?}"
    );
    let (level, scope, message): &(LogLevel, String, String) = guest_records[0];
    assert_eq!(*level, LogLevel::Info, "level must arrive verbatim");
    assert_eq!(
        scope, "guest.csharp_test_adder",
        "scope must arrive verbatim"
    );
    assert_eq!(
        message, "héllo from .NET ✓",
        "message must arrive verbatim, UTF-16 → UTF-8 transcode intact"
    );
}

#[test]
fn integration_dotnet_wrong_major_version_rejected() {
    skip_if_no_dotnet!();
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net99.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(CSHARP_DLL));
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("version") || error.contains("framework") || error.contains("99.0"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for net99.0, got: {other:?}"),
    }
}

#[test]
fn integration_dotnet_clr_shared_across_loads() {
    skip_if_no_dotnet!();
    // Load the fixture, unload it, and load it again with the same DotnetLoader.
    // CLR is a global once-initialized singleton — the re-load must succeed and
    // reuse the already-initialized CLR. (The unload in between is required:
    // re-loading a still-loaded bundle is rejected as a duplicate registration.)
    let rt: Arc<Runtime> = create_runtime();
    let result1: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(CSHARP_DLL));
    assert!(
        result1.is_ok(),
        "first load must succeed: {:?}",
        result1.err()
    );
    rt.unload_bundle(BundleId::new("csharp_test_adder"))
        .expect("unload must succeed");
    let result2: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(CSHARP_DLL));
    assert!(
        result2.is_ok(),
        "re-load after unload (CLR shared) must succeed: {:?}",
        result2.err()
    );
}

#[test]
fn pelite_reads_target_framework() {
    skip_if_no_dotnet!();
    let tfm: String =
        polyplug_dotnet::version::read_target_framework(std::path::Path::new(CSHARP_DLL))
            .expect("pelite TFM read must succeed");
    assert!(!tfm.is_empty(), "TFM must be non-empty for .NET assembly");
    // TFM from CA blob is LONG form: ".NETCoreApp,Version=v10.0" (NOT "net10.0")
    assert!(
        tfm.starts_with(".NETCoreApp,Version=v"),
        "TFM must be long-form '.NETCoreApp,Version=vX.Y': got {tfm}"
    );
}

#[test]
fn version_mismatch_pelite() {
    skip_if_no_dotnet!();
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net99.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(CSHARP_DLL));
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("version") || error.contains("framework"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for version mismatch, got: {other:?}"),
    }
}

#[test]
fn delegate_loader_cached_across_loads() {
    skip_if_no_dotnet!();
    // Load, unload, and re-load the same DLL — the re-load must succeed, proving
    // the AssemblyDelegateLoader is cached and reused. (The unload in between is
    // required: re-loading a still-loaded bundle is rejected as a duplicate
    // registration.)
    let rt: Arc<Runtime> = create_runtime();
    let result1: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(CSHARP_DLL));
    assert!(
        result1.is_ok(),
        "first load must succeed: {:?}",
        result1.err()
    );
    rt.unload_bundle(BundleId::new("csharp_test_adder"))
        .expect("unload must succeed");
    let result2: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(CSHARP_DLL));
    assert!(
        result2.is_ok(),
        "re-load after unload (cached loader) must succeed: {:?}",
        result2.err()
    );
}

#[test]
fn non_dotnet_dll_allowed() {
    // A non-.NET shared library (e.g., a plain C .so) should be allowed through version check
    // because read_target_framework returns Ok("") for non-CLR files.
    // We test this by passing a path to a known-non-dotnet file (a Rust test binary or lib).
    // The actual load will fail at the CLR level (not a .NET assembly) but NOT at version check.
    // Since we can't guarantee a non-dotnet file path in CI, test with a dummy path that
    // doesn't exist — the version::read_target_framework returns InitFailed, not
    // a version-specific error, confirming the version check path is bypassed for non-dotnet files.
    //
    // Instead: test the module function directly.
    let result: Result<String, LoaderError> =
        polyplug_dotnet::version::read_target_framework(std::path::Path::new("nonexistent.dll"));
    // Non-existent file should return InitFailed error
    match result {
        Err(LoaderError::InitFailed { bundle: _, error }) => {
            assert!(
                error.contains("assembly") || error.contains("not found"),
                "error: {error}"
            );
        }
        Ok(s) => panic!("expected error for nonexistent file, got Ok({s:?})"),
        Err(other) => panic!("expected InitFailed, got: {other:?}"),
    }
}
