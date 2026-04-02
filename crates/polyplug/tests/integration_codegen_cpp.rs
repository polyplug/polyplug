#![allow(clippy::expect_used)]

//! Integration test: run polyplugc to generate C++ bindings, assert all 7 expected
//! files are present, optionally compile with g++, and dispatch through the pre-built
//! C++ test plugin vtable when TEST_PLUGIN_CPP_SO is non-empty.
//!
//! This test crate is the crate root for the `integration_codegen_cpp` test binary.

use polyplug::plugin_registry::PluginRegistry;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::PluginContext;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::StringView;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

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
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
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

/// A register_plugin callback that stores vtable entries into the thread-local
/// Registry for dispatch testing.
///
/// # Safety
/// `rt_ctx`, `descriptor`, and `vtable` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _rt_ctx: *mut core::ffi::c_void,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginInterface,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1_u32,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: vtable is valid for this call (ABI contract).
    let vt: &PluginInterface = unsafe { &*vtable };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // Register with thread-local Registry.
    let result: Result<PluginHandle, _> = CPP_DISPATCH_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, PluginRegistry> = reg_cell.borrow();
        // SAFETY: vtable pointer is 'static — extracted from a loaded library that outlives registry.
        unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
    });

    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1_u32,
            message: StringView::null(),
        },
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(
    _rt_ctx: *mut core::ffi::c_void,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(
    _rt_ctx: *mut core::ffi::c_void,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_by_bundle callback.
unsafe extern "C" fn noop_find_by_bundle(
    _rt_ctx: *mut core::ffi::c_void,
    _bundle_id: u64,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

/// No-op resolve_plugin callback.
unsafe extern "C" fn noop_resolve_plugin(
    _rt_ctx: *mut core::ffi::c_void,
    _handle: PluginHandle,
) -> *const PluginInterface {
    core::ptr::null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractVTable {
    core::ptr::null()
}

std::thread_local! {
    static CPP_DISPATCH_REGISTRY: core::cell::RefCell<PluginRegistry> =
        core::cell::RefCell::new(PluginRegistry::new());
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
    let gen_output: Output = Command::new(env!("CARGO_BIN_EXE_polyplugc"))
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
        "guest/vtables.hpp",
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

    // ── 4. Attempt g++ compile of vtables.hpp (skip if g++ not found) ────────
    let gpp_version_result: std::io::Result<std::process::Output> =
        Command::new("g++").args(["--version"]).output();

    if let Ok(version_out) = gpp_version_result {
        if version_out.status.success() {
            let sdks_cpp_abi: PathBuf = workspace_root().join("sdks").join("cpp").join("abi");
            let vtables_hpp: PathBuf = out_dir.join("guest").join("vtables.hpp");
            let out_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_codegen_vtables.o");

            let compile_result: std::process::Output = Command::new("g++")
                .arg("-std=c++20")
                .arg(format!("-I{}", out_dir.join("guest").display()))
                .arg(format!("-I{}", out_dir.join("host").display()))
                .arg(format!("-I{}", sdks_cpp_abi.display()))
                .arg(&vtables_hpp)
                .arg("-c")
                .arg("-o")
                .arg(&out_obj)
                .output()
                .expect("g++ failed to run");

            assert!(
                compile_result.status.success(),
                "vtables.hpp did not compile:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&compile_result.stdout),
                String::from_utf8_lossy(&compile_result.stderr),
            );

            println!("test_cpp_codegen_files_exist: vtables.hpp compiled successfully ✓");
        } else {
            eprintln!("skipping g++ compile check: g++ --version returned non-zero");
        }
    } else {
        eprintln!("skipping g++ compile check: g++ not found");
    }
}

// ─── Part B: Runtime dispatch through C++ plugin (skips if SO unavailable) ───

/// Contract id for `test.add@1` (FNV-1a hash, matches C++ plugin).
const TEST_ADD_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B_u64;

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

    // ── 2. Resolve polyplug_init ──────────────────────────────────────────────
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *mut core::ffi::c_void,
            *const HostVTable,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };

    // ── 3. Reset the thread-local registry ───────────────────────────────────
    CPP_DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    // ── 4. Build HostVTable + call polyplug_init ─────────────────────────────
    let host_vtable: HostVTable = HostVTable {
        register_plugin: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_by_bundle: noop_find_by_bundle,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_plugin: noop_resolve_plugin,
        get_host_contract: noop_get_host_contract,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: POLYPLUG_ABI_VERSION,
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_vtable and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            core::ptr::null_mut(),
            &host_vtable as *const HostVTable,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");

    // ── 5. Look up vtable for test.add by contract_id ─────────────────────────
    let handle: PluginHandle = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(TEST_ADD_CONTRACT_ID, 0_u32)
            .expect("test.add must be registered after polyplug_init")
    });

    let vtable_ptr: *const PluginInterface = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("vtable must be resolvable from handle")
    });

    // SAFETY: vtable_ptr is valid — plugin is loaded and library is not yet dropped.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };

    assert_eq!(
        vtable.function_count, 1_u32,
        "C++ test.add vtable must have 1 function"
    );

    // ── 6. Get function pointer from vtable.functions[0] ─────────────────────
    // SAFETY: functions[0] is the cpp_test_add ABI wrapper with signature
    //   extern "C" AbiError(const void* args, void* out).
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(0) };

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

    assert_eq!(call_result.code, ABI_OK, "cpp_test_add must return ABI_OK");
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

    // SAFETY: symbol matches expected ABI signature
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *mut core::ffi::c_void,
            *const HostVTable,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found in Rust plugin")
    };

    CPP_DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    let host_vtable: HostVTable = HostVTable {
        register_plugin: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_by_bundle: noop_find_by_bundle,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_plugin: noop_resolve_plugin,
        get_host_contract: noop_get_host_contract,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: POLYPLUG_ABI_VERSION,
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_vtable and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            core::ptr::null_mut(),
            &host_vtable as *const HostVTable,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(
        init_result.code, ABI_OK,
        "Rust plugin polyplug_init must return ABI_OK"
    );

    let handle: PluginHandle = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(TEST_ADD_CONTRACT_ID, 0_u32)
            .expect("test.add must be registered from Rust plugin")
    });

    let vtable_ptr: *const PluginInterface = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("vtable must be resolvable")
    });

    // SAFETY: vtable_ptr is valid — plugin is loaded
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.function_count, 1_u32,
        "test.add vtable must have 1 function"
    );

    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;

    // SAFETY: functions[0] is the first ABI wrapper with signature
    //   extern "C" fn(*const (), *mut ()) -> AbiError
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr layout matches the target function signature per ABI contract.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args and out are valid stack allocations
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(
        call_result.code, ABI_OK,
        "Rust plugin add(3,5) must return ABI_OK"
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

    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *mut core::ffi::c_void,
            *const HostVTable,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init not found")
    };

    CPP_DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    let host_vtable: HostVTable = HostVTable {
        register_plugin: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_by_bundle: noop_find_by_bundle,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_plugin: noop_resolve_plugin,
        get_host_contract: noop_get_host_contract,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: POLYPLUG_ABI_VERSION,
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_vtable and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            core::ptr::null_mut(),
            &host_vtable as *const HostVTable,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(
        init_result.code, ABI_OK,
        "throwing plugin init must return ABI_OK"
    );

    let handle: PluginHandle = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(TEST_ADD_CONTRACT_ID, 0_u32)
            .expect("test.add registered from throwing plugin")
    });

    let vtable_ptr: *const PluginInterface = CPP_DISPATCH_REGISTRY
        .with(|cell| cell.borrow().resolve(handle).expect("vtable resolvable"));

    // SAFETY: vtable_ptr is valid — plugin is loaded
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };

    let args: AddArgs = AddArgs { a: 0_u32, b: 0_u32 };
    let mut out: u32 = 0_u32;

    // SAFETY: functions[0] is the cpp_throw_abi with noexcept wrapper
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(0) };
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

    // Must return ABI_ERROR_GENERIC (code=1) — std::exception was caught by noexcept wrapper
    assert_eq!(
        call_result.code, 1_u32,
        "exception must be caught and returned as ABI_ERROR_GENERIC"
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
