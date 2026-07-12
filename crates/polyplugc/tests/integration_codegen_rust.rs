//! Integration test: use polyplug_codegen library to generate Rust bindings,
//! compile them as a cdylib, load with libloading, dispatch `add(3, 5)` through
//! the interface, assert == 8.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;

use core::cell::Cell;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use libloading::Library;
use libloading::Symbol;
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
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_abi::ffi as abi_ffi;

use polyplug_abi::string_view_null;
use polyplug_codegen::{GenerateConfig, Lang, Side};
use polyplug_utils::BundleId;

mod cli_support;
use cli_support::cli_generate;

// ─── Helper: compile target dir ──────────────────────────────────────────────

/// Workspace root resolved from `CARGO_MANIFEST_DIR` (`crates/polyplug_codegen`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug_codegen")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Platform-specific shared library filename for the generated plugin.
fn so_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libcodegen_rust_test_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "codegen_rust_test_plugin.dll"
    } else {
        "libcodegen_rust_test_plugin.so"
    }
}

// ─── Helper: generate code using library API ─────────────────────────────────

/// Use polyplugc::generate() to generate Rust bindings.
fn generate_rust_bindings(api_toml: &Path, out_dir: &Path, side: Side) {
    let config = GenerateConfig {
        api_toml: api_toml.to_path_buf(),
        lang: Lang::Rust,
        side,
        out_dir: out_dir.to_path_buf(),
    };

    let output = cli_generate(&config).expect("polyplugc::generate failed");

    // Write generated files to disk
    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        fs::write(&file_path, &file.content).expect("failed to write generated file");
    }
}

// ─── Helper: write Cargo.toml for the generated cdylib ───────────────────────

/// Write a `Cargo.toml` for a cdylib crate that depends on `polyplug_abi`,
/// `polyplug_guest`, and `polyplug_utils`.
fn write_plugin_cargo_toml(crate_dir: &Path, guest_lib_path: &Path) {
    let abi_lib_path: PathBuf = workspace_root().join("crates").join("polyplug_abi");
    let utils_lib_path: PathBuf = workspace_root().join("crates").join("polyplug_utils");
    let content: String = format!(
        r#"[package]
name    = "codegen_rust_test_plugin"
version = "0.1.0"
edition = "2021"

[lib]
name      = "codegen_rust_test_plugin"
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
    fs::write(&cargo_toml_path, content).expect("failed to write plugin Cargo.toml");
}

// ─── Helper: write src/lib.rs for the generated cdylib ───────────────────────

/// Write a `src/lib.rs` that:
///   - Declares generated modules (types, contracts, interfaces) but NOT init.
///   - Defines `MyPlugin` and implements `TestAddGuestContract`.
///   - Exports a custom `polyplug_init` that sets `TEST_ADDER_IMPL` then registers the interface.
fn write_plugin_lib_rs(src_dir: &Path) {
    let content: &str = r#"// THIS FILE IS WRITTEN BY integration_codegen_rust TEST — DO NOT EDIT BY HAND

mod guest {
    pub mod types;
    pub mod contracts;
    pub mod interfaces;
}

#[allow(unused_imports)]
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::PluginDescriptor;
use polyplug_guest::GuestError;
use polyplug_abi::HostApi;
use polyplug_abi::BundleInitContext;
use polyplug_abi::StringView;
use polyplug_abi::Version;
use polyplug_abi::string_view_null;
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
        return AbiError { code: AbiErrorCode::Generic as u32, message: string_view_null() };
    }

    // SAFETY: host is non-null and valid per ABI contract.
    let host: &HostApi = unsafe { &*host };

    let desc: PluginDescriptor = PluginDescriptor {
        name: StringView { ptr: b"codegen_test_plugin".as_ptr(), len: 19_usize },
        contract_name: StringView { ptr: b"test.add".as_ptr(), len: 8_usize },
        version: Version { major: 1, minor: 0, patch: 0 },
    };

    // Out-param ABI: register_guest_contract writes its AbiError through a
    // trailing pointer and returns void; init surfaces it by value.
    let mut err: AbiError = AbiError::ok();
    // SAFETY: desc and TEST_ADDER_INTERFACE are 'static; host is valid; &mut err is valid.
    unsafe {
        (host.register_guest_contract)(
            host as *const _,
            &desc as *const PluginDescriptor,
            &TEST_ADDER_INTERFACE as *const _,
            &mut err as *mut AbiError,
        )
    };
    err
}
"#;
    let lib_rs_path: PathBuf = src_dir.join("lib.rs");
    fs::write(&lib_rs_path, content).expect("failed to write plugin src/lib.rs");
}

// ─── HostApi callback capturing the interface pointer ───────────────────────

// Captured interface pointer from the register_guest_contract callback, stored in a thread-local.
thread_local! {
    static CAPTURED_INTERFACE: Cell<*const GuestContractInterface> =
        const { Cell::new(ptr::null()) };
}

/// register_guest_contract callback that captures the interface pointer into `CAPTURED_INTERFACE`.
///
/// # Safety
/// `descriptor` and `interface` must be valid for the duration of the call.
unsafe extern "C" fn capture_interface_callback(
    _host: *const HostApi,
    _descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    CAPTURED_INTERFACE.with(|cell| cell.set(interface));
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

// ─── AddArgs mirrors the generated repr(C) struct ────────────────────────────

/// `AddArgs` — must match generated `types.rs` layout (`#[repr(C)]`).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── HostApi stub functions ─────────────────────────────────────────────────

unsafe extern "C" fn stub_alloc(_host: *const HostApi, size: usize, align: usize) -> *mut u8 {
    abi_ffi::polyplug_host_alloc(size, align)
}

unsafe extern "C" fn stub_free(_host: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: This is an unsafe extern "C" function. The caller ensures ptr is valid.
    unsafe {
        abi_ffi::polyplug_host_free(ptr, size, align);
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
    ptr::null()
}

unsafe extern "C" fn stub_get_host_contract(
    _host: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance {
        data: ptr::null_mut(),
    }
}

unsafe extern "C" fn stub_resolve_host_contract_interface(
    _host: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    ptr::null()
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
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn stub_reload_bundle(
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

unsafe extern "C" fn stub_register_host_contract(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn stub_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
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

unsafe extern "C" fn stub_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_codegen_compile_and_run() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let tmp_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codegen_rust_test");
    let src_dir: PathBuf = tmp_dir.join("src");
    let api_toml: PathBuf = workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_bundle.toml");
    let guest_lib_path: PathBuf = workspace_root().join("sdks").join("rust").join("guest");

    fs::create_dir_all(&src_dir).expect("failed to create src dir");

    // ── 2. Generate Rust bindings using library API ───────────────────────────
    generate_rust_bindings(&api_toml, &src_dir, Side::Guest);

    // ── 3. Write Cargo.toml + src/lib.rs ─────────────────────────────────────
    write_plugin_cargo_toml(&tmp_dir, &guest_lib_path);
    write_plugin_lib_rs(&src_dir);

    // ── 4. cargo build --release ──────────────────────────────────────────────
    let workspace_root_path: PathBuf = workspace_root();
    let target_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codegen_rust_build");

    let build_status: ExitStatus = Command::new(env!("CARGO"))
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
        "cargo build of generated plugin failed"
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
    let library: Library =
        unsafe { Library::new(&so_path).expect("failed to load generated plugin .so") };

    // ── 7. Resolve polyplug_init ──────────────────────────────────────────────
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // ── 8. Build HostApi + call polyplug_init ───────────────────────────────
    CAPTURED_INTERFACE.with(|cell| cell.set(ptr::null()));

    let host_abi: HostApi = HostApi {
        runtime: ptr::null_mut(),
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
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    };

    // SAFETY: init_fn is valid; host_abi lives for the duration of the call.
    let ctx: BundleInitContext = BundleInitContext {
        bundle_id: 0,
        bundle_path: string_view_null(),
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

    // SAFETY: functions[0] is the `add` ABI wrapper with the out-param signature
    //   extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError).
    // The instance parameter is passed as first argument (native dispatch).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is transmuted to the generic out-param dispatch signature. Argument
    // types are enforced by the test: AddArgs matches what the generated wrapper expects.
    // The signature carries GuestContractInstance first and the AbiError out-param last.
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = unsafe { mem::transmute(fn_ptr) };

    // The generated create_instance constructs the implementation via the
    // author factory and carries it in instance.data — no static storage.
    let mut instance: GuestContractInstance = GuestContractInstance::null();
    // SAFETY: host_abi outlives the instance; create_instance is the generated factory thunk.
    unsafe {
        (interface.create_instance)(
            interface.adapter_context,
            VmLoaderData::null(),
            &host_abi as *const HostApi,
            ptr::null(),
            &mut instance as *mut GuestContractInstance,
        )
    };
    assert!(
        !instance.data.is_null(),
        "create_instance must produce a non-null instance payload"
    );

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid AddArgs; out is a valid u32 location; instance
    // was created by the generated create_instance above; call_result receives
    // the AbiError through the trailing out-param.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            instance,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result as *mut AbiError,
        )
    };

    // SAFETY: instance was created by create_instance; destroy exactly once.
    unsafe {
        (interface.destroy_instance)(
            interface.adapter_context,
            VmLoaderData::null(),
            &host_abi as *const HostApi,
            instance,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "add(3, 5) must return Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");

    println!("test_rust_codegen_compile_and_run: add(3, 5) = {} ✓", out);

    // Keep the library alive until after the last call.
    mem::forget(library);
}

// ─── Enum types codegen test ─────────────────────────────────────────────────

#[test]
fn test_rust_codegen_generates_enum_types() {
    // ── 1. Paths ──────────────────────────────────────────────────────────────
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_rust_enum");
    let api_toml: PathBuf = workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_bundle.toml");

    fs::create_dir_all(&out_dir).expect("failed to create out_dir");

    // ── 2. Generate Rust bindings using library API ───────────────────────────
    generate_rust_bindings(&api_toml, &out_dir, Side::Host);

    // ── 3. Read host/types.rs and assert enum content ─────────────────────────
    let types_file: PathBuf = out_dir.join("host").join("types.rs");
    let content: String = fs::read_to_string(&types_file).expect("read types file");

    assert!(
        content.contains("#[repr(u32)]"),
        "types.rs must contain #[repr(u32)]: {}",
        types_file.display()
    );
    assert!(
        content.contains("pub enum PixelFormat"),
        "types.rs must contain pub enum PixelFormat"
    );
    assert!(
        content.contains("pub mod image_flags"),
        "types.rs must contain pub mod image_flags"
    );
    assert!(
        content.contains("pub struct ImageDesc"),
        "types.rs must contain pub struct ImageDesc"
    );

    println!("test_rust_codegen_generates_enum_types: all enum assertions passed ✓");
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
