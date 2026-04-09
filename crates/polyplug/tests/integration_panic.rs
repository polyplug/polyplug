#![allow(clippy::expect_used)]

//! Integration test: verify the generated catch_unwind ABI wrapper catches a panic
//! and returns ABI_ERROR_PANIC (= 3) WITHOUT aborting the process.
//!
//! This test crate is the crate root for the `integration_panic` test binary.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;

use polyplug_abi::AbiErrorCode;
use polyplug_abi::AbiError;
use polyplug_abi::HostInterface;
use polyplug_abi::PluginContext;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;

// ─── Host functions for integration tests ─────────────────────────────────────

/// Global storage for the interface pointer captured during `polyplug_init`.
///
/// # Safety
/// Only written by `capture_register_callback` which is called once during
/// `polyplug_init`, before the interface pointer is read in the test.
static mut CAPTURED_INTERFACE_PTR: *const GuestContractInterface = core::ptr::null();

/// A register_contract callback that captures the interface pointer.
///
/// # Safety
/// `this`, `_descriptor`, and `interface` must be valid for the duration of the call.
unsafe extern "C" fn capture_register_callback(
    _this: *const HostInterface,
    _descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    // SAFETY: CAPTURED_INTERFACE_PTR is only written here, during polyplug_init,
    // before the test reads it. Single-threaded test execution ensures no data race.
    unsafe {
        CAPTURED_INTERFACE_PTR = interface;
    }
    AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(
    _this: *const HostInterface,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(
    _this: *const HostInterface,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::PluginHandle {
    polyplug_abi::PluginHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<polyplug_abi::PluginHandle> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_contract callback.
unsafe extern "C" fn noop_resolve_contract(
    _this: *const HostInterface,
    _handle: polyplug_abi::PluginHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// No-op call_guest_method callback.
unsafe extern "C" fn noop_call_guest_method(
    _this: *const HostInterface,
    _instance: polyplug_abi::GuestContractInstance,
    _method_id: u32,
    _args: *const (),
    _out: *mut (),
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic,
        message: StringView::null(),
    }
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    polyplug_abi::Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[test]
fn test_panic_returns_abi_error_panic() {
    // -- Step 1: Locate workspace root and polyplugc binary --
    let manifest_dir: &Path = Path::new(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR = crates/polyplug; workspace root is two up.
    let workspace_root: &Path = manifest_dir
        .parent()
        .expect("crates parent")
        .parent()
        .expect("workspace root");

    let api_toml: PathBuf = workspace_root
        .join("tests")
        .join("fixtures")
        .join("test_panic_api.toml");

    // -- Step 2: Create a temp directory for the panic plugin crate --
    let tmp_dir: PathBuf = std::env::temp_dir().join("polyplug_panic_plugin_test");
    std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");
    std::fs::create_dir_all(tmp_dir.join("src")).expect("create tmp src dir");

    // -- Step 2b: Create a minimal bundle.toml referencing the API --
    let bundle_toml_content: String = format!(
        "[bundle]\n\
         name = \"panic_plugin\"\n\
         version = \"1.0.0\"\n\
         runtime = \"native\"\n\
         api = \"{}\"\n\
         \n\
         [bundle.file]\n\
         linux.x86_64 = \"libpanic_plugin.so\"\n\
         \n\
         [[plugin]]\n\
         name = \"panic_plugin\"\n\
         version = \"1.0.0\"\n\
         implements = [\"test.panic@1.0\"]\n",
        api_toml.display()
    );
    let bundle_toml_path: PathBuf = tmp_dir.join("bundle.toml");
    std::fs::write(&bundle_toml_path, bundle_toml_content).expect("write bundle.toml");

    // -- Step 3: Run polyplugc generate into tmp_dir/src --
    let polyplugc_bin: &str = env!("CARGO_BIN_EXE_polyplugc");
    let gen_status: ExitStatus = Command::new(polyplugc_bin)
        .arg("generate")
        .arg("--bundle")
        .arg(&bundle_toml_path)
        .arg("--lang")
        .arg("rust")
        .arg("--out")
        .arg(tmp_dir.join("src"))
        .status()
        .expect("polyplugc generate failed to run");

    assert!(
        gen_status.success(),
        "polyplugc generate exited with non-zero status"
    );

    // -- Step 4: Write Cargo.toml for the cdylib crate --
    // Only depend on polyplug_guest; polyplug is an indirect dep.
    // We do NOT add polyplug as a direct dep to avoid duplicate
    // `polyplug_abi_version` symbol (it is defined in polyplug/src/lib.rs).
    let guest_lib_path: PathBuf = workspace_root.join("crates").join("polyplug_guest");
    let cargo_toml_content: String = format!(
        "[package]\n\
         name = \"panic_plugin\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         [lib]\n\
         name = \"panic_plugin\"\n\
         crate-type = [\"cdylib\"]\n\
         \n\
         [dependencies]\n\
         polyplug_guest = {{ path = \"{}\" }}\n",
        guest_lib_path.display()
    );
    std::fs::write(tmp_dir.join("Cargo.toml"), &cargo_toml_content).expect("write Cargo.toml");

    // -- Step 5: Write src/lib.rs implementing TestPanicPlugin --
    // The generated src/guest/interfaces.rs, src/guest/contracts.rs, src/guest/types.rs already exist.
    // We provide the crate root lib.rs that declares the guest submodule and implements the trait.
    // We do NOT include mod guest::init -- we write our own polyplug_init here.
    // We do NOT define polyplug_abi_version -- it comes from polyplug rlib.
    let lib_rs_content: &str = concat!(
        "// THIS FILE IS WRITTEN BY THE integration_panic TEST -- NOT generated by polyplugc.\n",
        "// It implements the TestPanicPlugin trait with a function that always panics.\n",
        "\n",
        "mod guest {\n",
        "    pub mod types;\n",
        "    pub mod contracts;\n",
        "    pub mod interfaces;\n",
        "}\n",
        "\n",
        "use polyplug_guest::AbiError;\n",
        "use polyplug_guest::HostInterface;\n",
        "use polyplug_guest::PluginContext;\n",
        "use polyplug_guest::PluginDescriptor;\n",
        "use polyplug_guest::PluginError;\n",
        "use polyplug_guest::GuestContractInterface;\n",
        "use polyplug_guest::StringView;\n",
        "use polyplug_guest::AbiErrorCode;\n",
        "use guest::interfaces::PANIC_PLUGIN_IMPL;\n",
        "use guest::interfaces::PANIC_PLUGIN_INTERFACE;\n",
        "use guest::contracts::TestPanicPlugin;\n",
        "\n",
        "struct PanicPlugin;\n",
        "\n",
        "impl TestPanicPlugin for PanicPlugin {\n",
        "    fn do_panic(&self) -> Result<(), PluginError> {\n",
        "        panic!(\"intentional test panic\");\n",
        "    }\n",
        "}\n",
        "\n",
        "/// Register the panic plugin interface with the host.\n",
        "///\n",
        "/// # Safety\n",
        "/// `host` and `ctx` must be valid pointers.\n",
        "#[unsafe(no_mangle)]\n",
        "pub unsafe extern \"C\" fn polyplug_init(\n",
        "    host: *const HostInterface,\n",
        "    ctx: *const PluginContext,\n",
        ") -> AbiError {\n",
        "    PANIC_PLUGIN_IMPL.get_or_init(|| Box::new(PanicPlugin));\n",
        "    if host.is_null() || ctx.is_null() {\n",
        "        return AbiError {\n",
        "            code: AbiErrorCode::Generic,\n",
        "            message: StringView::null(),\n",
        "        };\n",
        "    }\n",
        "    // SAFETY: host is non-null and valid per ABI contract.\n",
        "    let host_iface: &HostInterface = unsafe { &*host };\n",
        "    let desc: PluginDescriptor = PluginDescriptor {\n",
        "        name: StringView {\n",
        "            ptr: b\"panic_plugin\".as_ptr(),\n",
        "            len: 12_usize,\n",
        "        },\n",
        "        contract_name: StringView {\n",
        "            ptr: b\"test.panic\".as_ptr(),\n",
        "            len: 10_usize,\n",
        "        },\n",
        "        version: polyplug_guest::Version { major: 1, minor: 0, patch: 0 },\n",
        "    };\n",
        "    // SAFETY: desc and interface are valid for the duration of the call.\n",
        "    unsafe {\n",
        "        (host_iface.register_contract)(\n",
        "            host,\n",
        "            &desc as *const PluginDescriptor,\n",
        "            &PANIC_PLUGIN_INTERFACE as *const GuestContractInterface,\n",
        "        )\n",
        "    }\n",
        "}\n",
    );
    std::fs::write(tmp_dir.join("src").join("lib.rs"), lib_rs_content).expect("write src/lib.rs");

    // -- Step 6: Build the cdylib --
    let build_status: ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--manifest-path")
        .arg(tmp_dir.join("Cargo.toml"))
        .arg("--release")
        .status()
        .expect("cargo build failed to run");

    assert!(
        build_status.success(),
        "cargo build for panic_plugin cdylib failed"
    );

    // -- Step 7: Locate the compiled .so --
    let lib_filename: &str = if cfg!(target_os = "macos") {
        "libpanic_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "panic_plugin.dll"
    } else {
        "libpanic_plugin.so"
    };

    let so_path: PathBuf = tmp_dir.join("target").join("release").join(lib_filename);

    // -- Step 8: Load with libloading --
    // SAFETY: so_path is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(&so_path).expect("failed to load panic_plugin shared library")
    };

    // -- Step 9: Resolve and call polyplug_init --
    // SAFETY: polyplug_init matches the expected ABI signature (2-arg).
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *const HostInterface,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_interface: HostInterface = HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: capture_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_contract: noop_resolve_contract,
        call_guest_method: noop_call_guest_method,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostInterface,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok, "polyplug_init must succeed (code Ok)");

    // SAFETY: CAPTURED_INTERFACE_PTR was written by capture_register_callback above.
    // Single-threaded; no race condition.
    let interface_ptr: *const GuestContractInterface = unsafe { CAPTURED_INTERFACE_PTR };
    assert!(!interface_ptr.is_null(), "interface pointer must be non-null");

    // SAFETY: interface_ptr is valid (plugin library is loaded, not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // -- Step 10: Call function_id 0 (do_panic) through the interface --
    // The generated ABI wrapper uses catch_unwind internally, so the panic is
    // caught inside the extern "C" boundary. The host sees AbiError { code: Panic }.

    // SAFETY: fn_ptr is function 0 in the interface (do_panic).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is the do_panic ABI wrapper -- extern "C" with no
        // meaningful args/out (void, no params). The catch_unwind wrapper
        // inside the plugin catches the panic before it crosses the FFI boundary.
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: do_panic ignores args and out entirely (void function, no params).
    let call_result: AbiError = unsafe { dispatch_fn(core::ptr::null(), core::ptr::null_mut()) };

    // -- Step 11: Assert panic was caught and returned ABI_ERROR_PANIC --
    assert_eq!(
        call_result.code, AbiErrorCode::Panic,
        "do_panic ABI wrapper must return AbiErrorCode::Panic, got {:?}",
        call_result.code
    );

    // Process continues here -- no abort occurred. Test completing IS the proof.

    // Leak the library to avoid dlclose issues on some platforms.
    core::mem::forget(library);
}
