#![allow(clippy::expect_used)]

//! Integration test: run polyplugc to generate C++ bindings, assert all 7 expected
//! files are present, optionally compile with g++, and dispatch through the pre-built
//! C++ test plugin interface when TEST_PLUGIN_CPP_SO is non-empty.
//!
//! This test crate is the crate root for the `integration_codegen_cpp` test binary.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use libloading::{Library, Symbol};

use core::cell::Ref;
use core::cell::RefCell;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::slice;
use core::str;

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::BundleInitContext;
use polyplug_abi::DependencyInfo;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::in_process::reject_in_process_bundle;
use polyplug_abi::types::StringView;
use polyplug_abi::types::abi_error_ok;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

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
    out_err: *mut AbiError,
) {
    if descriptor.is_null() || interface.is_null() {
        if !out_err.is_null() {
            // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
            unsafe {
                out_err.write(AbiError {
                    code: AbiErrorCode::Generic as u32,
                    message: StringView::null(),
                })
            };
        }
        return;
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let vt: &GuestContractInterface = unsafe { &*interface };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] = slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // Register with thread-local Registry.
    let result: Result<GuestContractHandle, _> = CPP_DISPATCH_REGISTRY.with(|reg_cell| {
        let registry: Ref<'_, RuntimeStore> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
        unsafe {
            registry.register_guest_contract(
                *desc,
                interface,
                contract_name.to_owned(),
                BundleId::from_u64(vt.contract_id.id()),
            )
        }
    });

    let err: AbiError = match result {
        Ok(_) => abi_error_ok(),
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(err) };
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
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// No-op resolve_guest_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    ptr::null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    ptr::null()
}

/// No-op load_bundle callback.
unsafe extern "C" fn noop_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op reload_bundle callback.
unsafe extern "C" fn noop_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_host_contract callback.
unsafe extern "C" fn noop_register_host_contract(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_loader callback.
unsafe extern "C" fn noop_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
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

unsafe extern "C" fn noop_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// Build a HostApi with all callbacks.
fn make_host_interface() -> HostApi {
    HostApi {
        runtime: ptr::null_mut(),
        register_guest_contract: registry_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
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
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    }
}

thread_local! {
    static CPP_DISPATCH_REGISTRY: RefCell<RuntimeStore> = RefCell::new(RuntimeStore::new());
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

    fs::create_dir_all(&out_dir).expect("failed to create out_dir");

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

    // ── 3. Assert all 6 expected guest-side files exist ─────────────────────
    let expected_files: [&str; 6] = [
        "guest/types.hpp",
        "guest/contracts.hpp",
        "guest/interfaces.hpp",
        "guest/init.hpp",
        "guest/in_process.hpp",
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
        "test_cpp_codegen_files_exist: all 6 guest files present in {} ✓",
        out_dir.display()
    );

    // ── 4. Attempt g++ compile of interfaces.hpp (skip if g++ not found) ───────
    let gpp_version_result: io::Result<Output> = Command::new("g++").args(["--version"]).output();

    if let Ok(version_out) = gpp_version_result {
        if version_out.status.success() {
            let sdks_cpp_abi: PathBuf = workspace_root().join("sdks").join("cpp").join("abi");
            let sdks_cpp_host: PathBuf = workspace_root().join("sdks").join("cpp").join("host");
            let sdks_cpp_guest: PathBuf = workspace_root().join("sdks").join("cpp").join("guest");
            let interfaces_hpp: PathBuf = out_dir.join("guest").join("interfaces.hpp");
            let out_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_codegen_interfaces.o");

            let compile_result: Output = Command::new("g++")
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

            let init_hpp: PathBuf = out_dir.join("guest").join("init.hpp");
            let init_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_codegen_init.o");
            let init_compile_result: Output = Command::new("g++")
                .arg("-std=c++20")
                .arg(format!("-I{}", out_dir.join("guest").display()))
                .arg(format!("-I{}", sdks_cpp_abi.display()))
                .arg(format!("-I{}", sdks_cpp_guest.display()))
                .arg(&init_hpp)
                .arg("-c")
                .arg("-o")
                .arg(&init_obj)
                .output()
                .expect("g++ failed to run");

            assert!(
                init_compile_result.status.success(),
                "init.hpp did not compile:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&init_compile_result.stdout),
                String::from_utf8_lossy(&init_compile_result.stderr),
            );
            println!("test_cpp_codegen_files_exist: init.hpp compiled successfully ✓");

            let in_process_hpp: PathBuf = out_dir.join("guest").join("in_process.hpp");
            let in_process_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_codegen_in_process.o");
            let in_process_compile_result: Output = Command::new("g++")
                .arg("-std=c++20")
                .arg(format!("-I{}", out_dir.join("guest").display()))
                .arg(format!("-I{}", sdks_cpp_abi.display()))
                .arg(format!("-I{}", sdks_cpp_host.display()))
                .arg(&in_process_hpp)
                .arg("-c")
                .arg("-o")
                .arg(&in_process_obj)
                .output()
                .expect("g++ failed to run");

            assert!(
                in_process_compile_result.status.success(),
                "in_process.hpp did not compile:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&in_process_compile_result.stdout),
                String::from_utf8_lossy(&in_process_compile_result.stderr),
            );
            println!("test_cpp_codegen_files_exist: in_process.hpp compiled successfully ✓");
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
fn test_add_contract_id() -> GuestContractId {
    GuestContractId::new("test.add", 1)
}

#[test]
fn test_cpp_plugin_dispatch() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        eprintln!("skipping cpp dispatch test: TEST_PLUGIN_CPP_SO not set (g++ not available)");
        return;
    }

    // ── 1. Load the pre-compiled C++ test plugin ──────────────────────────────
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib built by build.rs.
    let library: Library = unsafe {
        Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin shared library")
    };

    // ── 2. Resolve polyplug_init (2-arg signature) ───────────────────────────
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: Symbol<
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
    // SAFETY: functions[0] has the canonical native callback signature with
    // adapter context first; AddArgs matches the fixture's argument ABI.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { mem::transmute(fn_ptr) }
    };

    // ── 7. Call fn_ptr(args_ptr, out_ptr) — add(10, 20) → 30 ─────────────────
    let args: AddArgs = AddArgs {
        a: 10_u32,
        b: 20_u32,
    };
    let mut out: u32 = 0_u32;

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid AddArgs; out is a valid u32 location.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result,
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
    mem::forget(library);
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
    let library: Library =
        unsafe { Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin") };

    // SAFETY: symbol matches expected ABI signature (2-arg)
    let init_fn: Symbol<
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

    // SAFETY: functions[0] has the canonical native callback signature with
    // adapter context first.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { mem::transmute(fn_ptr) }
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args and out are valid stack allocations; null stateless instance.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result,
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
    mem::forget(library);
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
    let library: Library = unsafe {
        Library::new(TEST_PLUGIN_CPP_THROW_SO).expect("failed to load throwing C++ test plugin")
    };

    // SAFETY: symbol matches expected ABI signature (2-arg).
    let init_fn: Symbol<
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

    // SAFETY: functions[0] has the canonical native callback signature with
    // adapter context first.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { mem::transmute(fn_ptr) }
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args and out are valid
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result,
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
    mem::forget(library);
}

#[test]
fn test_cpp_in_process_adapters_are_stateful_and_context_local() {
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_cpp_in_process_adapters");
    let input_dir: PathBuf = out_dir.join("input");
    let generated_dir: PathBuf = out_dir.join("generated");
    fs::create_dir_all(&input_dir).expect("failed to create C++ adapter test input directory");

    fs::write(
        input_dir.join("api.toml"),
        r#"
[[contract]]
name = "test.alpha"
version = "1.0.0"

[[contract.functions]]
name = "increment"
return = "u32"

[[contract]]
name = "test.beta"
version = "1.0.0"

[[contract.functions]]
name = "increment"
return = "u32"
"#,
    )
    .expect("failed to write C++ adapter test API");
    fs::write(
        input_dir.join("bundle.toml"),
        r#"
[bundle]
name = "cpp_in_process_adapters"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libcpp_in_process_adapters.so"

[[plugin]]
name = "alpha"
implements = ["test.alpha@1.0"]

[[plugin]]
name = "beta"
implements = ["test.beta@1.0"]
"#,
    )
    .expect("failed to write C++ adapter test bundle");

    let generation: Output = Command::new(polyplugc_bin())
        .arg("generate")
        .arg("--bundle")
        .arg(input_dir.join("bundle.toml"))
        .arg("--lang")
        .arg("cpp")
        .arg("--out")
        .arg(&generated_dir)
        .output()
        .expect("failed to generate C++ in-process adapters");
    assert!(
        generation.status.success(),
        "C++ in-process adapter generation failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&generation.stdout),
        String::from_utf8_lossy(&generation.stderr),
    );

    let driver: PathBuf = out_dir.join("in_process_adapter_driver.cpp");
    fs::write(
        &driver,
        r#"
#include <cstdint>
#include <memory>
#include <stdexcept>

#include "in_process.hpp"

namespace {
int alpha_destroyed = 0;
int beta_destroyed = 0;

class Alpha final : public polyplug_plugin::TestAlphaGuestContract {
public:
    ~Alpha() override { ++alpha_destroyed; }
    uint32_t increment() override { return ++value_; }
private:
    uint32_t value_ = 0;
};

class Beta final : public polyplug_plugin::TestBetaGuestContract {
public:
    ~Beta() override { ++beta_destroyed; }
    uint32_t increment() override { return ++value_; }
private:
    uint32_t value_ = 40;
};

bool invoke_twice(const InProcessContractRegistration& registration, const HostApi& host,
                  uint32_t first, uint32_t second) {
    GuestContractInstance instance{nullptr, 0U};
    registration.interface->create_instance(
        registration.adapter_context, VmLoaderData{nullptr}, &host, nullptr, &instance);
    if (instance.data == nullptr) return false;

    auto dispatch = reinterpret_cast<void (*)(void*, GuestContractInstance, const void*, void*, AbiError*)>(
        registration.interface->dispatch.native.functions[0]);
    uint32_t result = 0;
    AbiError error{};
    dispatch(registration.adapter_context, instance, nullptr, &result, &error);
    if (error.code != static_cast<uint32_t>(AbiErrorCode::Ok) || result != first) return false;
    dispatch(registration.adapter_context, instance, nullptr, &result, &error);
    if (error.code != static_cast<uint32_t>(AbiErrorCode::Ok) || result != second) return false;
    registration.interface->destroy_instance(
        registration.adapter_context, VmLoaderData{nullptr}, &host, instance);
    return true;
}
}  // namespace

namespace polyplug_plugin {
TestAlphaGuestContract* polyplug_create_alpha(const HostApi*) { return nullptr; }
TestBetaGuestContract* polyplug_create_beta(const HostApi*) { return nullptr; }
}  // namespace polyplug_plugin

int main() {
    HostApi host{};
    auto bundle = polyplug_plugin::create_in_process_bundle(
        [](const HostApi*) { return std::make_unique<Alpha>(); },
        [](const HostApi*) { return std::make_unique<Beta>(); });

    const InProcessBundleRegistration& registration = bundle.in_process_registration();
    if (registration.contract_count != 2U || registration.contracts == nullptr) return 1;
    if (registration.contracts[0].adapter_context == registration.contracts[1].adapter_context) return 2;
    if (!invoke_twice(registration.contracts[0], host, 1U, 2U)) return 3;
    if (!invoke_twice(registration.contracts[1], host, 41U, 42U)) return 4;
    if (alpha_destroyed != 1 || beta_destroyed != 1) return 5;

    auto throwing_bundle = polyplug_plugin::create_in_process_bundle(
        [](const HostApi*) -> std::unique_ptr<Alpha> { throw std::runtime_error("factory failure"); },
        [](const HostApi*) { return std::make_unique<Beta>(); });
    const InProcessContractRegistration& throwing = throwing_bundle.in_process_registration().contracts[0];
    GuestContractInstance failed{reinterpret_cast<void*>(1), 99U};
    throwing.interface->create_instance(
        throwing.adapter_context, VmLoaderData{nullptr}, &host, nullptr, &failed);
    if (failed.data != nullptr || failed.contract_id != 0U) return 6;
    return 0;
}
"#,
    )
    .expect("failed to write C++ in-process adapter driver");

    let executable: PathBuf = out_dir.join("in_process_adapter_driver");
    let compile: Output = Command::new("g++")
        .arg("-std=c++20")
        .arg(&driver)
        .arg(format!("-I{}", generated_dir.join("guest").display()))
        .arg(format!(
            "-I{}",
            workspace_root()
                .join("sdks")
                .join("cpp")
                .join("abi")
                .display()
        ))
        .arg(format!(
            "-I{}",
            workspace_root()
                .join("sdks")
                .join("cpp")
                .join("host")
                .display()
        ))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("g++ failed to compile the C++ in-process adapter driver");
    assert!(
        compile.status.success(),
        "C++ in-process adapter driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution: Output = Command::new(&executable)
        .output()
        .expect("failed to run C++ in-process adapter driver");
    assert!(
        execution.status.success(),
        "C++ in-process adapter driver failed with {:?}:\nstdout: {}\nstderr: {}",
        execution.status.code(),
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
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

    fs::create_dir_all(&out_dir).expect("failed to create out_dir");

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
    let content: String = fs::read_to_string(&types_file).expect("read types file");

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
    _this: *const HostApi,
    _level: u32,
    _scope: StringView,
    _message: StringView,
) {
}

unsafe extern "C" fn stub_create_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _args: *const c_void,
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _instance: GuestContractInstance,
) {
}

unsafe extern "C" fn stub_registry_revision(_this: *const HostApi) -> u64 {
    0
}
