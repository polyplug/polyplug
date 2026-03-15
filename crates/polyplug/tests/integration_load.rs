//! Integration test: load the test_plugin .so, verify ABI version, verify vtable registration.
//!
//! This test crate is the crate root for the `integration_load` test binary.

use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::POLYPLUG_ABI_VERSION;
use polyplug::abi::PluginContext;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;

/// Path to the compiled test_plugin shared library — set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Minimal registrar for integration tests ─────────────────────────────────

/// Registrar callback that captures the registered vtable pointer for inspection.
///
/// # Safety
/// `registrar`, `descriptor`, and `vtable` must be valid non-null pointers for
/// the duration of this call (guaranteed by the ABI contract).
unsafe extern "C" fn capture_register(
    registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if registrar.is_null() || descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1, // ABI_ERROR_GENERIC
            message: polyplug::abi::StringView::null(),
        };
    }
    // SAFETY: vtable is valid for the call duration. We store the contract_id for
    // later verification. The vtable itself lives in the plugin's static memory.
    let contract_id: u64 = unsafe { (*vtable).contract_id };
    // SAFETY: vtable is valid for this call (ABI contract); reading a plain u32 field.
    let function_count: u32 = unsafe { (*vtable).function_count };

    // Store results in thread-local for the test to read back.
    CAPTURED_CONTRACT_ID.with(|cell| {
        *cell.borrow_mut() = Some(contract_id);
    });
    CAPTURED_FUNCTION_COUNT.with(|cell| {
        *cell.borrow_mut() = Some(function_count);
    });

    AbiError {
        code: ABI_OK,
        message: polyplug::abi::StringView::null(),
    }
}

std::thread_local! {
    static CAPTURED_CONTRACT_ID: core::cell::RefCell<Option<u64>> =
        const { core::cell::RefCell::new(None) };
    static CAPTURED_FUNCTION_COUNT: core::cell::RefCell<Option<u32>> =
        const { core::cell::RefCell::new(None) };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_load_and_abi_version() {
    // SAFETY: TEST_PLUGIN_SO is an absolute path to a compiled cdylib.
    // libloading loads it with RTLD_NOW | RTLD_LOCAL semantics.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve and call polyplug_abi_version
    // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
    let abi_version_fn: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
        library
            .get(b"polyplug_abi_version\0")
            .expect("polyplug_abi_version symbol not found")
    };

    // SAFETY: symbol was just resolved and is a valid C function pointer.
    let version: u32 = unsafe { abi_version_fn() };
    assert_eq!(version, 1, "polyplug_abi_version() must return 1");

    // Leak the library — vtable pointers must remain valid.
    core::mem::forget(library);
}

#[test]
fn test_init_registers_vtable() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve polyplug_init symbol.
    // SAFETY: polyplug_init is a C function with the PluginRegistrar ABI.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*mut PluginRegistrar, *const PluginContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Build a registrar that captures registration data.
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_register,
        host: core::ptr::null(),
    };

    // Clear thread-locals before calling init.
    CAPTURED_CONTRACT_ID.with(|cell| *cell.borrow_mut() = None);
    CAPTURED_FUNCTION_COUNT.with(|cell| *cell.borrow_mut() = None);

    // SAFETY: init_fn is valid; registrar and ctx live for the duration of this call.
    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: POLYPLUG_ABI_VERSION,
    };
    // SAFETY: init_fn is valid; registrar and ctx live for the duration of this call.
    let result: AbiError = unsafe {
        init_fn(
            &mut registrar as *mut PluginRegistrar,
            &ctx as *const PluginContext,
        )
    };

    assert_eq!(result.code, ABI_OK, "polyplug_init must return ABI_OK");

    // Verify the vtable was registered with correct data.
    let captured_id: u64 = CAPTURED_CONTRACT_ID
        .with(|cell| *cell.borrow())
        .expect("vtable was not registered during init");

    let captured_count: u32 = CAPTURED_FUNCTION_COUNT
        .with(|cell| *cell.borrow())
        .expect("function_count was not captured");

    // FNV-1a("test.add@1") = 0xCC4232FAB0410D2B (verified at compile time)
    let expected_contract_id: u64 = polyplug::abi::contract_id("test.add", 1);
    assert_eq!(
        captured_id, expected_contract_id,
        "contract_id must match FNV-1a(\"test.add@1\")"
    );
    assert_eq!(
        captured_count, 1,
        "test.add vtable must have function_count = 1"
    );

    // Leak the library.
    core::mem::forget(library);
}

#[test]
fn test_missing_symbol_returns_error() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // A non-existent symbol should return Err.
    // SAFETY: library is loaded; querying a symbol name is safe even if it doesn't exist.
    let result: Result<libloading::Symbol<'_, unsafe extern "C" fn()>, _> =
        unsafe { library.get(b"nonexistent_symbol_xyz\0") };
    assert!(result.is_err(), "non-existent symbol must return Err");

    core::mem::forget(library);
}
