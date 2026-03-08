//! Integration test: run polyplugc to generate C++ bindings, assert all 6 expected
//! files are present, optionally compile with g++, and dispatch through the pre-built
//! C++ test plugin vtable when TEST_PLUGIN_CPP_SO is non-empty.
//!
//! This test crate is the crate root for the `integration_codegen_cpp` test binary.
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use polyplug_runtime::abi::AbiError;
use polyplug_runtime::abi::PluginDescriptor;
use polyplug_runtime::abi::PluginHandle;
use polyplug_runtime::abi::PluginRegistrar;
use polyplug_runtime::abi::PluginVTable;
use polyplug_runtime::abi::StringView;
use polyplug_runtime::abi::ABI_OK;
use polyplug_runtime::registry::Registry;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

// ─── Env vars set by build.rs ─────────────────────────────────────────────────

/// Path to the pre-compiled C++ test plugin .so, or empty if g++ was unavailable.
const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Workspace root resolved from `CARGO_MANIFEST_DIR` (`crates/polyplug-runtime`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug-runtime")
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

// ─── Registrar callback that stores vtable into a thread-local Registry ──────

/// A minimal registrar callback that stores vtable entries into the thread-local
/// Registry for dispatch testing.
///
/// # Safety
/// `registrar`, `descriptor`, and `vtable` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1_u32,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    let vt: &PluginVTable = unsafe { &*vtable };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name.ptr points to valid UTF-8 bytes for desc.contract_name.len bytes.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };

    // Register with thread-local Registry.
    let result: Result<PluginHandle, _> = CPP_DISPATCH_REGISTRY.with(|reg_cell| {
        let registry: std::cell::Ref<'_, Registry> = reg_cell.borrow();
        registry.register(
            *desc,
            vtable as *const PluginVTable,
            contract_name.to_owned(),
            vt.contract_id,
        )
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

std::thread_local! {
    static CPP_DISPATCH_REGISTRY: std::cell::RefCell<Registry> =
        std::cell::RefCell::new(Registry::new());
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
    let api_toml: PathBuf = workspace_root()
        .join("tests")
        .join("fixtures")
        .join("test_api.toml");

    std::fs::create_dir_all(&out_dir).expect("failed to create out_dir");

    // ── 2. Run polyplugc to generate C++ bindings ─────────────────────────────
    let gen_output: Output = run_polyplugc_cpp(&api_toml, &out_dir);
    assert!(
        gen_output.status.success(),
        "polyplugc generate --lang cpp failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gen_output.stdout),
        String::from_utf8_lossy(&gen_output.stderr),
    );

    // ── 3. Assert all 6 expected files exist ─────────────────────────────────
    let expected_files: [&str; 6] = [
        "types.hpp",
        "contracts.hpp",
        "vtables.hpp",
        "init.hpp",
        "host_callers.hpp",
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
        "test_cpp_codegen_files_exist: all 6 files present in {} ✓",
        out_dir.display()
    );

    // ── 4. Attempt g++ compile of vtables.hpp (skip if g++ not found) ────────
    let gpp_version_result: std::io::Result<std::process::Output> =
        Command::new("g++").args(["--version"]).output();

    if let Ok(version_out) = gpp_version_result {
        if version_out.status.success() {
            let host_libs_cpp: PathBuf = workspace_root().join("host-libs").join("cpp");
            let vtables_hpp: PathBuf = out_dir.join("vtables.hpp");
            let out_obj: PathBuf =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_codegen_vtables.o");

            let compile_result: std::process::Output = Command::new("g++")
                .arg("-std=c++17")
                .arg(format!("-I{}", host_libs_cpp.display()))
                .arg(format!("-I{}", out_dir.display()))
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
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };

    // ── 3. Reset the thread-local registry ───────────────────────────────────
    CPP_DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    // ── 4. Build registrar + call polyplug_init ───────────────────────────────
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    // SAFETY: init_fn is valid; registrar lives for the duration of the call.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");

    // ── 5. Look up vtable for test.add by contract_id ─────────────────────────
    let handle: PluginHandle = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(TEST_ADD_CONTRACT_ID, 0_u32)
            .expect("test.add must be registered after polyplug_init")
    });

    let vtable_ptr: *const PluginVTable = CPP_DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("vtable must be resolvable from handle")
    });

    // SAFETY: vtable_ptr is valid — plugin is loaded and library is not yet dropped.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    assert_eq!(
        vtable.function_count, 1_u32,
        "C++ test.add vtable must have 1 function"
    );

    // ── 6. Get function pointer from vtable.functions[0] ─────────────────────
    // SAFETY: functions[0] is the cpp_test_add ABI wrapper with signature
    //   extern "C" AbiError(const void* args, void* out).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };

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
