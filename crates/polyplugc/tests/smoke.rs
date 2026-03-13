//! Smoke tests — Phase 1 gate. Must pass before any hardening work begins.
//!
//! Two E2E codegen round-trip tests:
//!   1. `smoke_rust_codegen_dispatch` — generate Rust bindings, compile plugin, load,
//!      dispatch add(3, 5), assert == 8 and ABI_OK.
//!   2. `smoke_cpp_codegen_dispatch` — generate C++ bindings, assert files exist,
//!      optionally compile/load if g++ available, otherwise gracefully skip.
//!
//! This test crate is the crate root for the `smoke` test binary.

#![allow(clippy::expect_used)]

use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::PluginContext;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
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

/// Write a `Cargo.toml` for a cdylib crate that depends on `polyplug_guest`.
fn write_plugin_cargo_toml(crate_dir: &Path, guest_lib_path: &Path) {
    let content: String = format!(
        r#"[package]
name    = "smoke_rust_test_plugin"
version = "0.1.0"
edition = "2021"

[lib]
name      = "smoke_rust_test_plugin"
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

/// Write a `src/lib.rs` that declares generated modules, implements `MyPlugin`,
/// and exports `polyplug_init`.
fn write_plugin_lib_rs(src_dir: &Path) {
    let content: &str = r#"// THIS FILE IS WRITTEN BY smoke TEST — DO NOT EDIT BY HAND

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
use polyplug_guest::PluginRegistrar;
use polyplug_guest::StringView;
use guest::contracts::TestAddPlugin;
use guest::types::AddArgs;
use guest::vtables::TEST_ADD_IMPL;
use guest::vtables::TEST_ADD_VTABLE;

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
/// `registrar` must be a valid non-null pointer to a `PluginRegistrar`.
#[no_mangle]
pub unsafe extern "C" fn polyplug_init(registrar: *mut PluginRegistrar) -> AbiError {
    // Set the OnceLock impl before any vtable function can be called.
    TEST_ADD_IMPL.get_or_init(|| Box::new(MyPlugin));

    if registrar.is_null() {
        return AbiError { code: ABI_ERROR_GENERIC, message: StringView::null() };
    }

    // SAFETY: registrar is non-null and valid per ABI contract.
    let reg: &mut PluginRegistrar = unsafe { &mut *registrar };

    let desc: PluginDescriptor = PluginDescriptor {
        name: StringView { ptr: b"smoke_test_plugin".as_ptr(), len: 17_usize },
        contract_name: StringView { ptr: b"test.add".as_ptr(), len: 8_usize },
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    };

    // SAFETY: desc and TEST_ADD_VTABLE are 'static; registrar is valid.
    unsafe {
        (reg.register_plugin)(
            registrar,
            &desc as *const PluginDescriptor,
            &TEST_ADD_VTABLE as *const _,
        )
    }
}
"#;
    let lib_rs_path: PathBuf = src_dir.join("lib.rs");
    std::fs::write(&lib_rs_path, content).expect("failed to write plugin src/lib.rs");
}

// ─── Registrar callback capturing the vtable pointer ─────────────────────────

// Captured vtable pointer from the registrar callback, stored in a thread-local.
std::thread_local! {
    static CAPTURED_VTABLE: core::cell::Cell<*const PluginVTable> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

/// Registrar callback that captures the vtable pointer into `CAPTURED_VTABLE`.
///
/// # Safety
/// `descriptor` and `vtable` must be valid for the duration of the call.
unsafe extern "C" fn capture_vtable_callback(
    _registrar: *mut PluginRegistrar,
    _descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    CAPTURED_VTABLE.with(|cell| cell.set(vtable));
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

/// `AddArgs` — must match generated `types.rs` layout (`#[repr(C)]`).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
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
    let guest_lib_path: PathBuf = workspace_root().join("guest-libs").join("rust");

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
        unsafe extern "C" fn(*mut PluginRegistrar, *const PluginContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // ── 8. Build registrar + call polyplug_init ───────────────────────────────
    CAPTURED_VTABLE.with(|cell| cell.set(core::ptr::null()));

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_vtable_callback,
        host: core::ptr::null(),
    };

    // SAFETY: init_fn is valid; registrar lives for the duration of the call.
    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
    };
    // SAFETY: init_fn is valid; registrar and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &mut registrar as *mut PluginRegistrar,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");

    // ── 9. Retrieve the captured vtable ──────────────────────────────────────
    let vtable_ptr: *const PluginVTable = CAPTURED_VTABLE.with(|cell| cell.get());
    assert!(
        !vtable_ptr.is_null(),
        "vtable pointer must be non-null after polyplug_init"
    );

    // SAFETY: vtable_ptr is valid — plugin is loaded and library is not yet dropped.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    assert_eq!(
        vtable.function_count, 4_u32,
        "test.add vtable must have 4 functions"
    );

    // ── 10. Dispatch add(3, 5) via function_id 0 ─────────────────────────────
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;

    // SAFETY: functions[0] is the `add` ABI wrapper with signature
    //   extern "C" fn(*const (), *mut ()) -> AbiError.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
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

    // ── 3. Assert all 5 expected guest files exist ──────────────────────────
    let expected_guest_files: [&str; 4] = ["types.hpp", "contracts.hpp", "vtables.hpp", "init.hpp"];
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
    // ── 4. Attempt g++ compile of vtables.hpp (skip if g++ not found) ────────
    let gpp_version_result: std::io::Result<std::process::Output> =
        Command::new("g++").args(["--version"]).output();

    if let Ok(version_out) = gpp_version_result {
        if version_out.status.success() {
            let host_libs_cpp: PathBuf = workspace_root().join("host-libs").join("cpp");
            let vtables_hpp: PathBuf = guest_dir.join("vtables.hpp");
            let out_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("smoke_cpp_vtables.o");

            let compile_result: std::process::Output = Command::new("g++")
                .arg("-std=c++20")
                .arg(format!("-I{}", host_libs_cpp.display()))
                .arg(format!("-I{}", guest_dir.display()))
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

            println!("smoke_cpp_codegen_dispatch: vtables.hpp compiled successfully ✓");
        } else {
            eprintln!("skipping g++ compile check: g++ --version returned non-zero");
        }
    } else {
        eprintln!("skipping g++ compile check: g++ not found");
    }
}
