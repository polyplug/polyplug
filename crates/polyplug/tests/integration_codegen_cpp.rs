#![allow(clippy::expect_used)]

//! Integration test: run polyplugc to generate C++ bindings, assert all 7 expected
//! files are present, optionally compile with g++, and dispatch through the pre-built
//! C++ test plugin interface when TEST_PLUGIN_CPP_SO is non-empty.
//!
//! This test crate is the crate root for the `integration_codegen_cpp` test binary.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::BundleInitContext;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::types::StringView;
use polyplug_abi::types::abi_error_ok;

mod common;

use common::polyplugc_bin;

// ─── Env vars set by build.rs ─────────────────────────────────────────────────

/// Path to the pre-compiled C++ test plugin .so, or empty if g++ was unavailable.
const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Workspace root resolved from `CARGO_MANIFEST_DIR` (`crates/polyplug`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Run `polyplugc generate --api <api_toml> --lang cpp --out <out_dir>`.
/// Returns the `Output` for inspection.
fn run_polyplugc_cpp(api_toml: &Path, out_dir: &Path) -> Output {
    Command::new(polyplugc_bin())
        .arg("generate")
        .arg("--api")
        .arg(api_toml)
        .arg("--lang")
        .arg("cpp")
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

// ─── Host functions for integration tests ─────────────────────────────────────

/// A register_guest_contract callback that stores interface entries into the thread-local
/// Registry for dispatch testing.
///
/// # Safety
/// `this`, `descriptor`, and `interface` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if descriptor.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let vt: &GuestContractInterface = unsafe { &*interface };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // Register with thread-local Registry.
    let result: Result<GuestContractHandle, _> = CPP_DISPATCH_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, RuntimeStore> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
        unsafe {
            registry.register_guest_contract(
                *desc,
                interface,
                contract_name.to_owned(),
                polyplug_utils::BundleId::from_u64(vt.contract_id.id()),
            )
        }
    });

    match result {
        Ok(_) => abi_error_ok(),
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_guest_contract callback.
unsafe extern "C" fn noop_find_guest_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<GuestContractHandle> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_guest_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(
    _this: *const HostApi,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    polyplug_abi::Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(
    _this: *const HostApi,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

/// No-op load_bundle callback.
unsafe extern "C" fn noop_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op reload_bundle callback.
unsafe extern "C" fn noop_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op register_host_contract callback.
unsafe extern "C" fn noop_register_host_contract(
    _this: *const HostApi,
    _interface: *const polyplug_abi::HostContractInterface,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op register_loader callback.
unsafe extern "C" fn noop_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut core::ffi::c_void,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op get_last_error callback.
unsafe extern "C" fn noop_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

/// No-op get_error_len callback.
unsafe extern "C" fn noop_get_error_len(_this: *const HostApi) -> usize {
    0
}

/// No-op call_guest_method callback.
unsafe extern "C" fn noop_call_guest_method(
    _this: *const HostApi,
    _instance: polyplug_abi::GuestContractInstance,
    _fn_id: u32,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
    _arena: *mut polyplug_abi::CallArena,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn noop_unload_bundle(
    _this: *const HostApi,
    _bundle_id: polyplug_utils::BundleId,
) -> AbiError {
    AbiError::ok()
}

/// Build a HostApi with all callbacks.
fn make_host_interface() -> HostApi {
    HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_guest_contract: noop_find_guest_contract,
        find_all_guest_contracts: noop_find_all_guest_contracts,
        resolve_guest_contract: noop_resolve_guest_contract,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        call_guest_method: noop_call_guest_method,
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    }
}

std::thread_local! {
    static CPP_DISPATCH_REGISTRY: core::cell::RefCell<RuntimeStore> =
        core::cell::RefCell::new(RuntimeStore::new());
}

/// `AddArgs` — mirrors the C++ struct in the test plugin (`#[repr(C)]`).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Part A: Codegen file existence check (always runs) ──────────────────────

#[test]
fn test_cpp_codegen_files_exist() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let out_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gen_cpp_codegen");
    let bundle_toml: PathBuf = workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_bundle.toml");

    std::fs::create_dir_all(&out_dir).expect("failed to create out_dir");

    // ── 2. Run polyplugc to generate C++ bindings ─────────────────────────────
    let gen_output: Output = Command::new(polyplugc_bin())
        .arg("generate")
        .arg("--bundle")
        .arg(&bundle_toml)
        .arg("--lang")
        .arg("cpp")
        .arg("--out")
        .arg(&out_dir)
        .output()
        .expect("failed to spawn polyplugc");
    assert!(
        gen_output.status.success(),
        "polyplugc generate --lang cpp failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gen_output.stdout),
        String::from_utf8_lossy(&gen_output.stderr),
    );

    // ── 3. Assert all 5 expected guest-side files exist ─────────────────────
    let expected_files: [&str; 5] = [
        "guest/types.hpp",
        "guest/contracts.hpp",
        "guest/interfaces.hpp",
        "guest/init.hpp",
        "manifest.toml",
    ];
    for filename in expected_files {
        let file_path: PathBuf = out_dir.join(filename);
        assert!(
            file_path.exists(),
            "expected generated file not found: {}",
            file_path.display()
        );
    }

    println!(
        "test_cpp_codegen_files_exist: all 5 guest files present in {} ✓",
        out_dir.display()
    );

    // ── 4. Attempt g++ compile of interfaces.hpp (skip if g++ not found) ───────
    let gpp_version_result: std::io::Result<std::process::Output> =
        Command::new("g++").args(["--version"]).output();

    if let Ok(version_out) = gpp_version_result {
        if version_out.status.success() {
            let sdks_cpp_abi: PathBuf = workspace_root().join("sdks").join("cpp").join("abi");
            let interfaces_hpp: PathBuf = out_dir.join("guest").join("interfaces.hpp");
            let out_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_codegen_interfaces.o");

            let compile_result: std::process::Output = Command::new("g++")
                .arg("-std=c++20")
                .arg(format!("-I{}", out_dir.join("guest").display()))
                .arg(format!("-I{}", out_dir.join("host").display()))
                .arg(format!("-I{}", sdks_cpp_abi.display()))
                .arg(&interfaces_hpp)
                .arg("-c")
                .arg("-o")
                .arg(&out_obj)
                .output()
                .expect("g++ failed to run");

            assert!(
                compile_result.status.success(),
                "interfaces.hpp did not compile:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&compile_result.stdout),
                String::from_utf8_lossy(&compile_result.stderr),
            );

            println!("test_cpp_codegen_files_exist: interfaces.hpp compiled successfully ✓");
        } else {
            eprintln!("skipping g++ compile check: g++ --version returned non-zero");
        }
    } else {
        eprintln!("skipping g++ compile check: g++ not found");
    }
}

// ─── Part B: Runtime dispatch through C++ plugin (skips if SO unavailable) ───

/// Contract id for `test.add@1`, computed from the canonical scheme
/// (`fnv1a_64("guest_contract:test.add@1")`) so it tracks the plugin fixtures.
fn test_add_contract_id() -> polyplug_utils::GuestContractId {
    polyplug_utils::GuestContractId::new("test.add", 1)
}

#[test]
fn test_cpp_plugin_dispatch() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        eprintln!("skipping cpp dispatch test: TEST_PLUGIN_CPP_SO not set (g++ not available)");
        return;
    }

    // ── 1. Load the pre-compiled C++ test plugin ──────────────────────────────
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO)
            .expect("failed to load C++ test plugin shared library")
    };

    // ── 2. Resolve polyplug_init (2-arg signature) ───────────────────────────
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };

    // ── 3. Reset the thread-local registry ───────────────────────────────────
    CPP_DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    // ── 4. Build HostApi + call polyplug_init ──────────────────────────
    let host_interface: HostApi = make_host_interface();

    let ctx: BundleInitContext = BundleInitContext {
        bundle_id: 0,
        bundle_path: StringView::null(),
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return Ok"
    );

    // ── 5. Look up interface for test.add by contract_id ─────────────────────────
    let handle: GuestContractHandle = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_contract_id(), 0_u32)
            .expect("test.add must be registered after polyplug_init")
    });

    let interface_ptr: *const GuestContractInterface = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("interface must be resolvable from handle")
    });

    // SAFETY: interface_ptr is valid — plugin is loaded and library is not yet dropped.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // ── 6. Get function pointer from interface.dispatch.native.functions[0] ───────
    // SAFETY: functions[0] is the cpp_test_add ABI wrapper with signature
    //   extern "C" AbiError(const void* args, void* out).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };

    // SAFETY: fn_ptr is transmuted to the generic dispatch signature. Argument
    // types are enforced: AddArgs matches what cpp_test_add expects.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // ── 7. Call fn_ptr(args_ptr, out_ptr) — add(10, 20) → 30 ─────────────────
    let args: AddArgs = AddArgs {
        a: 10_u32,
        b: 20_u32,
    };
    let mut out: u32 = 0_u32;

    // SAFETY: args is a valid AddArgs; out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "cpp_test_add must return Ok"
    );
    assert_eq!(out, 30_u32, "add(10, 20) must equal 30");

    println!("test_cpp_plugin_dispatch: add(10, 20) = {} ✓", out);

    // Keep the library alive until after the last call.
    core::mem::forget(library);
}

// ─── Part C: Cross-language test — Rust plugin loaded via Rust test infrastructure ───

/// Path to the pre-compiled Rust test plugin, set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

#[test]
fn test_cpp_host_loads_rust_plugin() {
    if TEST_PLUGIN_SO.is_empty() {
        eprintln!("skipping: TEST_PLUGIN_SO not set");
        return;
    }

    // SAFETY: TEST_PLUGIN_SO is a compiled Rust cdylib built by build.rs
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin")
    };

    // SAFETY: symbol matches expected ABI signature (2-arg)
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found in Rust plugin")
    };

    CPP_DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    let host_interface: HostApi = make_host_interface();

    let ctx: BundleInitContext = BundleInitContext {
        bundle_id: 0,
        bundle_path: StringView::null(),
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "Rust plugin polyplug_init must return Ok"
    );

    let handle: GuestContractHandle = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_contract_id(), 0_u32)
            .expect("test.add must be registered from Rust plugin")
    });

    let interface_ptr: *const GuestContractInterface = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("interface must be resolvable")
    });

    // SAFETY: interface_ptr is valid — plugin is loaded
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;

    // SAFETY: functions[0] is the first ABI wrapper with the frozen native signature
    //   extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr layout matches the target function signature per ABI contract.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args and out are valid stack allocations; null stateless instance.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "Rust plugin add(3,5) must return Ok"
    );
    assert_eq!(out, 8_u32, "Rust plugin add(3,5) must equal 8");

    println!(
        "test_cpp_host_loads_rust_plugin: Rust plugin add(3,5) = {} ✓",
        out
    );
    core::mem::forget(library);
}

// ─── Part D: Exception isolation test — throwing C++ plugin ───

const TEST_PLUGIN_CPP_THROW_SO: &str = env!("TEST_PLUGIN_CPP_THROW_SO");

#[test]
fn test_exception_isolation_cpp() {
    if TEST_PLUGIN_CPP_THROW_SO.is_empty() {
        eprintln!("skipping exception isolation test: g++ not available");
        return;
    }

    // SAFETY: TEST_PLUGIN_CPP_THROW_SO is a compiled C++ cdylib built by build.rs
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_THROW_SO)
            .expect("failed to load throwing C++ test plugin")
    };

    // SAFETY: symbol matches expected ABI signature (2-arg).
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found")
    };

    CPP_DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    let host_interface: HostApi = make_host_interface();

    let ctx: BundleInitContext = BundleInitContext {
        bundle_id: 0,
        bundle_path: StringView::null(),
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "throwing plugin init must return Ok"
    );

    let handle: GuestContractHandle = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_contract_id(), 0_u32)
            .expect("test.add registered from throwing plugin")
    });

    let interface_ptr: *const GuestContractInterface = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("interface resolvable")
    });

    // SAFETY: interface_ptr is valid — plugin is loaded
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    let args: AddArgs = AddArgs { a: 0_u32, b: 0_u32 };
    let mut out: u32 = 0_u32;

    // SAFETY: functions[0] is the cpp_throw_abi with noexcept wrapper
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr layout matches the target function signature per ABI contract.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args and out are valid
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    // Must return Generic (code=1) — std::exception was caught by noexcept wrapper
    assert_eq!(
        call_result.code,
        AbiErrorCode::Generic as u32,
        "exception must be caught and returned as Generic"
    );
    // Process survived — if we reach this line, no crash occurred
    println!("test_exception_isolation_cpp: exception caught, host survived ✓");
    core::mem::forget(library);
}

// ─── Enum types codegen test ─────────────────────────────────────────────────

#[test]
fn test_cpp_codegen_generates_enum_types() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_enum");
    let api_toml: PathBuf = workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_api.toml");

    std::fs::create_dir_all(&out_dir).expect("failed to create out_dir");

    // ── 2. Run polyplugc to generate C++ bindings ──────────────────────────────
    let gen_output: Output = run_polyplugc_cpp(&api_toml, &out_dir);
    assert!(
        gen_output.status.success(),
        "polyplugc generate --lang cpp failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gen_output.stdout),
        String::from_utf8_lossy(&gen_output.stderr),
    );

    // ── 3. Read host/types.hpp and assert enum content ─────────────────────────
    let types_file: PathBuf = out_dir.join("host").join("types.hpp");
    let content: String = std::fs::read_to_string(&types_file).expect("read types file");

    assert!(
        content.contains("enum class PixelFormat"),
        "types.hpp must contain enum class PixelFormat"
    );
    assert!(
        content.contains("operator|"),
        "types.hpp must contain operator|"
    );

    println!("test_cpp_codegen_generates_enum_types: all enum assertions passed ✓");
}

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const polyplug_abi::HostApi,
    _level: u32,
    _scope: polyplug_abi::StringView,
    _message: polyplug_abi::StringView,
) {
}
