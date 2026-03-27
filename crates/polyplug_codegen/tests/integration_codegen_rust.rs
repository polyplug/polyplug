//! Integration test: use polyplug_codegen library to generate Rust bindings,
//! compile them as a cdylib, load with libloading, dispatch `add(3, 5)` through
//! the vtable, assert == 8.

#![allow(clippy::expect_used)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginContext;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::StringView;
use polyplug_abi::ABI_OK;
use polyplug_codegen::{generate, GenerateConfig, Lang, Side};

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

/// Use polyplug_codegen::generate() to generate Rust bindings.
fn generate_rust_bindings(api_toml: &Path, out_dir: &Path, side: Side) {
    let config = GenerateConfig {
        api_toml: api_toml.to_path_buf(),
        lang: Lang::Rust,
        side,
        out_dir: out_dir.to_path_buf(),
    };

    let output = generate(config).expect("polyplug_codegen::generate failed");

    // Write generated files to disk
    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }
}

// ─── Helper: write Cargo.toml for the generated cdylib ───────────────────────

/// Write a `Cargo.toml` for a cdylib crate that depends on `polyplug_guest`.
fn write_plugin_cargo_toml(crate_dir: &Path, guest_lib_path: &Path) {
    let content: String = format!(
        r#"[package]
name    = "codegen_rust_test_plugin"
version = "0.1.0"
edition = "2021"

[lib]
name      = "codegen_rust_test_plugin"
crate-type = ["cdylib"]

[dependencies]
polyplug_guest = {{ path = "{}" }}

[workspace]
"#,
        guest_lib_path.display()
    );
    let cargo_toml_path: PathBuf = crate_dir.join("Cargo.toml");
    std::fs::write(&cargo_toml_path, content).expect("failed to write plugin Cargo.toml");
}

// ─── Helper: write src/lib.rs for the generated cdylib ───────────────────────

/// Write a `src/lib.rs` that:
///   - Declares generated modules (types, contracts, vtables) but NOT init.
///   - Defines `MyPlugin` and implements `TestAddPlugin`.
///   - Exports a custom `polyplug_init` that sets `TEST_ADDER_IMPL` then registers the vtable.
fn write_plugin_lib_rs(src_dir: &Path) {
    let content: &str = r#"// THIS FILE IS WRITTEN BY integration_codegen_rust TEST — DO NOT EDIT BY HAND

mod guest {
    pub mod types;
    pub mod contracts;
    pub mod vtables;
}

#[allow(unused_imports)]
use polyplug_guest::ABI_ERROR_GENERIC;
use polyplug_guest::AbiError;
use polyplug_guest::PluginDescriptor;
use polyplug_guest::PluginError;
use polyplug_guest::HostVTable;
use polyplug_guest::PluginContext;
use polyplug_guest::StringView;
use core::ffi::c_void;
use guest::contracts::TestAddPlugin;
use guest::types::AddArgs;
use guest::vtables::TEST_ADDER_VTABLE;
use guest::vtables::set_test_adder_impl;

struct MyPlugin;

impl TestAddPlugin for MyPlugin {
    fn add(&self, args: &AddArgs) -> Result<u32, PluginError> {
        Ok(args.a.wrapping_add(args.b))
    }

    fn add_primitive(&self, a: u32, b: u32) -> Result<u32, PluginError> {
        Ok(a.wrapping_add(b))
    }

    fn version(&self) -> Result<StringView, PluginError> {
        Ok(StringView { ptr: b"1.0.0".as_ptr(), len: 5_usize })
    }

    fn reset(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

/// # Safety
/// `rt_ctx` and `host` must be valid non-null pointers provided by the host.
#[no_mangle]
pub unsafe extern "C" fn polyplug_init(
    rt_ctx: *mut c_void,
    host: *const HostVTable,
    _ctx: *const PluginContext,
) -> AbiError {
    if host.is_null() {
        return AbiError { code: ABI_ERROR_GENERIC, message: StringView::null() };
    }

    // Set the implementation before registering
    let _ = set_test_adder_impl(Box::new(MyPlugin));

    // SAFETY: host is non-null and valid per ABI contract.
    let host: &HostVTable = unsafe { &*host };

    let desc: PluginDescriptor = PluginDescriptor {
        name: StringView { ptr: b"codegen_test_plugin".as_ptr(), len: 19_usize },
        contract_name: StringView { ptr: b"test.add".as_ptr(), len: 8_usize },
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    };

    // SAFETY: desc and TEST_ADDER_VTABLE are 'static; host is valid.
    unsafe {
        (host.register_plugin)(
            rt_ctx,
            &desc as *const PluginDescriptor,
            &TEST_ADDER_VTABLE as *const _,
        )
    }
}
"#;
    let lib_rs_path: PathBuf = src_dir.join("lib.rs");
    std::fs::write(&lib_rs_path, content).expect("failed to write plugin src/lib.rs");
}

// ─── HostVTable callback capturing the vtable pointer ─────────────────────────

// Captured vtable pointer from the register_plugin callback, stored in a thread-local.
std::thread_local! {
    static CAPTURED_VTABLE: core::cell::Cell<*const PluginInterface> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

/// Register_plugin callback that captures the vtable pointer into `CAPTURED_VTABLE`.
///
/// # Safety
/// `descriptor` and `vtable` must be valid for the duration of the call.
unsafe extern "C" fn capture_vtable_callback(
    _rt_ctx: *mut core::ffi::c_void,
    _descriptor: *const PluginDescriptor,
    vtable: *const PluginInterface,
) -> AbiError {
    CAPTURED_VTABLE.with(|cell| cell.set(vtable));
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

// ─── AddArgs mirrors the generated repr(C) struct ────────────────────────────

/// `AddArgs` — must match generated `types.rs` layout (`#[repr(C)]`).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── HostVTable stub functions ─────────────────────────────────────────────────

unsafe extern "C" fn stub_alloc(
    _rt_ctx: *mut core::ffi::c_void,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

unsafe extern "C" fn stub_free(
    _rt_ctx: *mut core::ffi::c_void,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: This is an unsafe extern "C" function. The caller ensures ptr is valid.
    unsafe {
        polyplug_abi::ffi::polyplug_host_free(ptr, size, align);
    }
}

unsafe extern "C" fn stub_find_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle {
        index: u32::MAX,
        generation: 0,
    }
}

unsafe extern "C" fn stub_find_by_bundle(
    _rt_ctx: *mut core::ffi::c_void,
    _bundle_id: u64,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle {
        index: u32::MAX,
        generation: 0,
    }
}

unsafe extern "C" fn stub_find_all_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

unsafe extern "C" fn stub_resolve_plugin(
    _rt_ctx: *mut core::ffi::c_void,
    _handle: PluginHandle,
) -> *const PluginInterface {
    core::ptr::null()
}

unsafe extern "C" fn stub_get_host_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractVTable {
    core::ptr::null()
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
    let guest_lib_path: PathBuf = workspace_root().join("crates").join("polyplug_guest");

    std::fs::create_dir_all(&src_dir).expect("failed to create src dir");

    // ── 2. Generate Rust bindings using library API ───────────────────────────
    generate_rust_bindings(&api_toml, &src_dir, Side::Guest);

    // ── 3. Write Cargo.toml + src/lib.rs ─────────────────────────────────────
    write_plugin_cargo_toml(&tmp_dir, &guest_lib_path);
    write_plugin_lib_rs(&src_dir);

    // ── 4. cargo build --release ──────────────────────────────────────────────
    let workspace_root_path: PathBuf = workspace_root();
    let target_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codegen_rust_build");

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
    let library: libloading::Library =
        unsafe { libloading::Library::new(&so_path).expect("failed to load generated plugin .so") };

    // ── 7. Resolve polyplug_init ──────────────────────────────────────────────
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
            .expect("polyplug_init symbol not found")
    };

    // ── 8. Build HostVTable + call polyplug_init ───────────────────────────────
    CAPTURED_VTABLE.with(|cell| cell.set(core::ptr::null()));

    let host_vtable: HostVTable = HostVTable {
        register_plugin: capture_vtable_callback,
        alloc: stub_alloc,
        free: stub_free,
        find_by_contract: stub_find_by_contract,
        find_by_bundle: stub_find_by_bundle,
        find_all_by_contract: stub_find_all_by_contract,
        resolve_plugin: stub_resolve_plugin,
        get_host_contract: stub_get_host_contract,
    };

    // SAFETY: init_fn is valid; host_vtable lives for the duration of the call.
    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: polyplug_abi::POLYPLUG_ABI_VERSION,
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

    // ── 9. Retrieve the captured vtable ──────────────────────────────────────
    let vtable_ptr: *const PluginInterface = CAPTURED_VTABLE.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable pointer must be non-null after polyplug_init"
    );

    // SAFETY: vtable_ptr is valid — plugin is loaded and library is not yet dropped.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };

    assert_eq!(
        vtable.function_count, 4_u32,
        "test.add vtable must have 4 functions"
    );

    // ── 10. Dispatch add(3, 5) via function_id 0 ─────────────────────────────
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;

    // SAFETY: functions[0] is the `add` ABI wrapper with signature
    //   extern "C" fn(*const (), *mut ()) -> AbiError.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is transmuted to the generic dispatch signature. Argument
    // types are enforced by the test: AddArgs matches what the generated wrapper expects.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid AddArgs; out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(call_result.code, ABI_OK, "add(3, 5) must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");

    println!("test_rust_codegen_compile_and_run: add(3, 5) = {} ✓", out);

    // Keep the library alive until after the last call.
    core::mem::forget(library);
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

    std::fs::create_dir_all(&out_dir).expect("failed to create out_dir");

    // ── 2. Generate Rust bindings using library API ───────────────────────────
    generate_rust_bindings(&api_toml, &out_dir, Side::Host);

    // ── 3. Read host/types.rs and assert enum content ─────────────────────────
    let types_file: PathBuf = out_dir.join("host").join("types.rs");
    let content: String = std::fs::read_to_string(&types_file).expect("read types file");

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
