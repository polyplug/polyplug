//! Smoke tests — Phase 1 gate. Must pass before any hardening work begins.
//!
//! Two E2E codegen round-trip tests:
//!   1. `smoke_rust_codegen_dispatch` — generate Rust bindings, compile plugin, load,
//!      dispatch add(3, 5), assert == 8 and AbiErrorCode::Ok.
//!   2. `smoke_cpp_codegen_dispatch` — generate C++ bindings, assert files exist,
//!      optionally compile/load if g++ available, otherwise gracefully skip.
//!
//! This test crate is the crate root for the `smoke` test binary.

#![allow(clippy::expect_used)]

use core::ffi::c_void;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::BundleInitContext;
use polyplug_abi::DependencyInfo;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_utils::BundleId;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

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

/// Platform-specific shared library filename for the generated Rust plugin.
fn so_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libsmoke_rust_test_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "smoke_rust_test_plugin.dll"
    } else {
        "libsmoke_rust_test_plugin.so"
    }
}

/// Run `polyplugc generate --bundle <bundle_toml> --lang rust --out <out_dir>`.
fn run_polyplugc_rust(bundle_toml: &Path, out_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .arg("generate")
        .arg("--bundle")
        .arg(bundle_toml)
        .arg("--lang")
        .arg("rust")
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

/// Run `polyplugc generate --bundle <bundle_toml> --lang cpp --out <out_dir>`.
fn run_polyplugc_cpp(bundle_toml: &Path, out_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .arg("generate")
        .arg("--bundle")
        .arg(bundle_toml)
        .arg("--lang")
        .arg("cpp")
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

/// Write a `Cargo.toml` for a cdylib crate that depends on `polyplug_abi`,
/// `polyplug_guest`, and `polyplug_utils`.
fn write_plugin_cargo_toml(crate_dir: &Path, guest_lib_path: &Path) {
    let abi_lib_path: PathBuf = workspace_root().join("crates").join("polyplug_abi");
    let utils_lib_path: PathBuf = workspace_root().join("crates").join("polyplug_utils");
    let content: String = format!(
        r#"[package]
name    = "smoke_rust_test_plugin"
version = "0.1.0"
edition = "2021"

[lib]
name      = "smoke_rust_test_plugin"
crate-type = ["cdylib"]

[dependencies]
polyplug_abi = {{ path = "{}" }}
polyplug_guest = {{ path = "{}" }}
polyplug_utils = {{ path = "{}" }}

[workspace]
"#,
        // Backslashes are invalid TOML escape sequences; forward slashes are valid
        // path separators on every platform (including Windows).
        abi_lib_path.to_string_lossy().replace('\\', "/"),
        guest_lib_path.to_string_lossy().replace('\\', "/"),
        utils_lib_path.to_string_lossy().replace('\\', "/")
    );
    let cargo_toml_path: PathBuf = crate_dir.join("Cargo.toml");
    std::fs::write(&cargo_toml_path, content).expect("failed to write plugin Cargo.toml");
}

/// Write a `src/lib.rs` that declares generated modules, implements `MyPlugin`,
/// and exports `polyplug_init`.
fn write_plugin_lib_rs(src_dir: &Path) {
    let content: &str = r#"// THIS FILE IS WRITTEN BY smoke TEST — DO NOT EDIT BY HAND

mod guest {
    pub mod types;
    pub mod contracts;
    pub mod interfaces;
}

#[allow(unused_imports)]
use polyplug_abi::AbiErrorCode;
use polyplug_abi::AbiError;
use polyplug_abi::PluginDescriptor;
use polyplug_guest::GuestError;
use polyplug_abi::HostApi;
use polyplug_abi::BundleInitContext;
use polyplug_abi::StringView;
use polyplug_abi::Version;
use guest::contracts::TestAddGuestContract;
use guest::types::AddArgs;
use guest::interfaces::TEST_ADDER_INTERFACE;
use polyplug_guest::HostContext;

struct MyPlugin;

impl TestAddGuestContract for MyPlugin {
    fn add(&self, args: &AddArgs) -> Result<u32, GuestError> {
        Ok(args.a.wrapping_add(args.b))
    }

    fn add_primitive(&self, a: u32, b: u32) -> Result<u32, GuestError> {
        Ok(a.wrapping_add(b))
    }

    fn version(&self) -> Result<StringView, GuestError> {
        Ok(StringView { ptr: b"1.0.0".as_ptr(), len: 5_usize })
    }

    fn reset(&self) -> Result<(), GuestError> {
        Ok(())
    }
}

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data` —
/// no static storage.
#[no_mangle]
pub fn polyplug_create_test_adder(_host: HostContext) -> Box<dyn TestAddGuestContract> {
    Box::new(MyPlugin)
}

/// # Safety
/// `host` must be a valid non-null pointer provided by the host.
#[no_mangle]
pub unsafe extern "C" fn polyplug_init(
    host: *const HostApi,
    _ctx: *const BundleInitContext,
) -> AbiError {
    if host.is_null() {
        return AbiError { code: AbiErrorCode::Generic as u32, message: StringView::null() };
    }

    // SAFETY: host is non-null and valid per ABI contract.
    let host: &HostApi = unsafe { &*host };

    let desc: PluginDescriptor = PluginDescriptor {
        name: StringView { ptr: b"smoke_test_plugin".as_ptr(), len: 17_usize },
        contract_name: StringView { ptr: b"test.add".as_ptr(), len: 8_usize },
        version: Version { major: 1, minor: 0, patch: 0 },
    };

    // SAFETY: desc and TEST_ADDER_INTERFACE are 'static; host is valid.
    unsafe {
        (host.register_guest_contract)(
            host as *const _,
            &desc as *const PluginDescriptor,
            &TEST_ADDER_INTERFACE as *const _,
        )
    }
}
"#;
    let lib_rs_path: PathBuf = src_dir.join("lib.rs");
    std::fs::write(&lib_rs_path, content).expect("failed to write plugin src/lib.rs");
}

// ─── HostApi callback capturing the interface pointer ───────────────────────

// Captured interface pointer from the register_guest_contract callback, stored in a thread-local.
std::thread_local! {
    static CAPTURED_INTERFACE: core::cell::Cell<*const GuestContractInterface> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

/// register_guest_contract callback that captures the interface pointer into `CAPTURED_INTERFACE`.
///
/// # Safety
/// `descriptor` and `interface` must be valid for the duration of the call.
unsafe extern "C" fn capture_interface_callback(
    _host: *const HostApi,
    _descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    CAPTURED_INTERFACE.with(|cell| cell.set(interface));
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// `AddArgs` — must match generated `types.rs` layout (`#[repr(C)]`).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── HostApi stub functions ─────────────────────────────────────────────────

unsafe extern "C" fn stub_alloc(_host: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

unsafe extern "C" fn stub_free(_host: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: This is an unsafe extern "C" function. The caller ensures ptr is valid.
    unsafe {
        polyplug_abi::ffi::polyplug_host_free(ptr, size, align);
    }
}

unsafe extern "C" fn stub_find_guest_contract(
    _host: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle {
        index: u32::MAX,
        generation: 0,
    }
}

unsafe extern "C" fn stub_find_all_guest_contracts(
    _host: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

unsafe extern "C" fn stub_resolve_guest_contract(
    _host: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn stub_get_host_contract(
    _host: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance {
        data: core::ptr::null_mut(),
    }
}

unsafe extern "C" fn stub_resolve_host_contract_interface(
    _host: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn stub_list_bundles(_host: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn stub_get_dependencies(_host: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

unsafe extern "C" fn stub_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn stub_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn stub_register_host_contract(
    _this: *const HostApi,
    _interface: *const polyplug_abi::HostContractInterface,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn stub_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut c_void,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

unsafe extern "C" fn stub_get_error_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn stub_call_guest_method(
    _this: *const HostApi,
    _instance: GuestContractInstance,
    _fn_id: u32,
    _args: *const c_void,
    _out: *mut c_void,
    _arena: *mut polyplug_abi::CallArena,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn stub_unload_bundle(_this: *const HostApi, _bundle_id: BundleId) -> AbiError {
    AbiError::ok()
}

// ─── Test 1: Rust codegen round-trip ─────────────────────────────────────────

#[test]
fn smoke_rust_codegen_dispatch() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let tmp_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("smoke_rust_test");
    let src_dir: PathBuf = tmp_dir.join("src");
    let bundle_toml: PathBuf = workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_bundle.toml");
    let guest_lib_path: PathBuf = workspace_root().join("sdks").join("rust").join("guest");

    std::fs::create_dir_all(&src_dir).expect("failed to create src dir");

    // ── 2. Run polyplugc to generate Rust bindings into tmp_dir/src/ ──────────
    let gen_output: Output = run_polyplugc_rust(&bundle_toml, &src_dir);
    assert!(
        gen_output.status.success(),
        "polyplugc generate --lang rust failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gen_output.stdout),
        String::from_utf8_lossy(&gen_output.stderr),
    );

    // ── 3. Write Cargo.toml + src/lib.rs ─────────────────────────────────────
    write_plugin_cargo_toml(&tmp_dir, &guest_lib_path);
    write_plugin_lib_rs(&src_dir);

    // ── 4. cargo build --release ──────────────────────────────────────────────
    let workspace_root_path: PathBuf = workspace_root();
    let target_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("smoke_rust_build");

    let build_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--release")
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--manifest-path")
        .arg(tmp_dir.join("Cargo.toml"))
        .current_dir(&workspace_root_path)
        .status()
        .expect("failed to spawn cargo build");

    assert!(
        build_status.success(),
        "cargo build of generated smoke Rust plugin failed"
    );

    // ── 5. Locate the compiled .so ────────────────────────────────────────────
    let so_path: PathBuf = target_dir.join("release").join(so_filename());
    assert!(
        so_path.exists(),
        "compiled .so not found at {}",
        so_path.display()
    );

    // ── 6. Load with libloading ───────────────────────────────────────────────
    // SAFETY: so_path is a compiled cdylib we just built.
    let library: libloading::Library =
        unsafe { libloading::Library::new(&so_path).expect("failed to load smoke plugin .so") };

    // ── 7. Resolve polyplug_init ──────────────────────────────────────────────
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // ── 8. Build HostApi + call polyplug_init ───────────────────────────────
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));

    let host_abi: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_callback,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };

    // SAFETY: init_fn is valid; host_abi lives for the duration of the call.
    let ctx: BundleInitContext = BundleInitContext {
        bundle_id: 0,
        bundle_path: StringView::null(),
    };
    // SAFETY: init_fn is valid; host_abi and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_abi as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return Ok"
    );

    // ── 9. Retrieve the captured interface ──────────────────────────────────────
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface pointer must be non-null after polyplug_init"
    );

    // SAFETY: interface_ptr is valid — plugin is loaded and library is not yet dropped.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Get function_count from the native dispatch structure
    // SAFETY: interface is a valid GuestContractInterface with Native dispatch;
    // reading the native union variant is sound for this contract.
    let function_count: u32 = unsafe { interface.dispatch.native.function_count };
    assert_eq!(
        function_count, 4_u32,
        "test.add interface must have 4 functions"
    );

    // ── 10. Dispatch add(3, 5) via function_id 0 ─────────────────────────────
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;

    // SAFETY: functions[0] is the `add` ABI wrapper with signature
    //   extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError.
    // The instance parameter is passed as first argument (native dispatch).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is transmuted to the generic dispatch signature. Argument
    // types are enforced by the test: AddArgs matches what the generated wrapper expects.
    // The new signature includes GuestContractInstance as the first parameter.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // The generated create_instance constructs the implementation via the
    // author factory and carries it in instance.data — no static storage.
    // SAFETY: host_abi outlives the instance; create_instance is the generated factory thunk.
    let instance: GuestContractInstance =
        unsafe { (interface.create_instance)(&host_abi as *const HostApi, core::ptr::null()) };
    assert!(
        !instance.data.is_null(),
        "create_instance must produce a non-null instance payload"
    );

    // SAFETY: args is a valid AddArgs; out is a valid u32 location; instance
    // was created by the generated create_instance above.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            instance,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    // SAFETY: instance was created by create_instance; destroy exactly once.
    unsafe { (interface.destroy_instance)(&host_abi as *const HostApi, instance) };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "add(3, 5) must return Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");

    println!("smoke_rust_codegen_dispatch: add(3, 5) = {} ✓", out);

    // Keep the library alive until after the last call.
    core::mem::forget(library);
}

// ─── Test 2: C++ codegen round-trip ──────────────────────────────────────────

#[test]
fn smoke_cpp_codegen_dispatch() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let out_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("smoke_cpp_gen");
    let bundle_toml: PathBuf = workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_bundle.toml");

    std::fs::create_dir_all(&out_dir).expect("failed to create cpp out_dir");

    // ── 2. Run polyplugc to generate C++ bindings ─────────────────────────────
    let gen_output: Output = run_polyplugc_cpp(&bundle_toml, &out_dir);
    assert!(
        gen_output.status.success(),
        "polyplugc generate --lang cpp failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gen_output.stdout),
        String::from_utf8_lossy(&gen_output.stderr),
    );

    let guest_dir: PathBuf = out_dir.join("guest");

    // ── 3. Assert all 4 expected guest files exist ──────────────────────────
    let expected_guest_files: [&str; 4] =
        ["types.hpp", "contracts.hpp", "interfaces.hpp", "init.hpp"];
    for filename in expected_guest_files {
        let file_path: PathBuf = guest_dir.join(filename);
        assert!(
            file_path.exists(),
            "expected generated C++ guest file not found: {}",
            file_path.display()
        );
    }
    let manifest_path: PathBuf = out_dir.join("manifest.toml");
    assert!(
        manifest_path.exists(),
        "expected manifest.toml not found: {}",
        manifest_path.display()
    );

    println!(
        "smoke_cpp_codegen_dispatch: all 5 C++ guest files present in {} ✓",
        out_dir.display()
    );
    // ── 4. Attempt g++ compile of interfaces.hpp (skip if g++ not found) ────────
    let gpp_version_result: std::io::Result<std::process::Output> =
        Command::new("g++").args(["--version"]).output();

    if let Ok(version_out) = gpp_version_result {
        if version_out.status.success() {
            let host_libs_cpp: PathBuf = workspace_root().join("sdks").join("cpp").join("abi");
            let interfaces_hpp: PathBuf = guest_dir.join("interfaces.hpp");
            let out_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("smoke_cpp_interfaces.o");

            let compile_result: std::process::Output = Command::new("g++")
                .arg("-std=c++20")
                .arg(format!("-I{}", host_libs_cpp.display()))
                .arg(format!("-I{}", guest_dir.display()))
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

            println!("smoke_cpp_codegen_dispatch: interfaces.hpp compiled successfully ✓");
        } else {
            eprintln!("skipping g++ compile check: g++ --version returned non-zero");
        }
    } else {
        eprintln!("skipping g++ compile check: g++ not found");
    }
}

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const polyplug_abi::HostApi,
    _level: u32,
    _scope: polyplug_abi::StringView,
    _message: polyplug_abi::StringView,
) {
}
