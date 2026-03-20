#![allow(clippy::expect_used)]

//! Integration test: verify the `PluginContext.bundle_path` round-trip for the Rust fixture plugin.
//!
//! Loads the test_plugin shared library, calls `polyplug_init` with a crafted
//! `PluginContext` containing a known `bundle_path`, then calls
//! `polyplug_get_last_bundle_path()` and asserts the returned `StringView` matches.
//!
//! This test crate is the crate root for the `integration_context` test binary.

use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::POLYPLUG_ABI_VERSION;
use polyplug_abi::PluginContext;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;

/// Path to the compiled test_plugin shared library — set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── No-op registrar callback ─────────────────────────────────────────────────

/// No-op registrar callback — accepts the registration and returns ABI_OK.
///
/// # Safety
/// `registrar`, `descriptor`, and `vtable` must be valid non-null pointers for
/// the duration of this call (guaranteed by the ABI contract).
unsafe extern "C" fn noop_register(
    _registrar: *mut PluginRegistrar,
    _descriptor: *const PluginDescriptor,
    _vtable: *const PluginVTable,
) -> AbiError {
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn rust_plugin_receives_bundle_path() {
    // SAFETY: TEST_PLUGIN_SO is an absolute path to a compiled cdylib.
    // libloading loads it with RTLD_NOW | RTLD_LOCAL semantics.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve the two-arg polyplug_init symbol.
    // SAFETY: polyplug_init is a C function with signature
    //   `unsafe extern "C" fn(*mut PluginRegistrar, *const PluginContext) -> AbiError`.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*mut PluginRegistrar, *const PluginContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Resolve polyplug_get_last_bundle_path symbol.
    // SAFETY: polyplug_get_last_bundle_path is a C function with signature
    //   `unsafe extern "C" fn() -> StringView`.
    let get_path_fn: libloading::Symbol<'_, unsafe extern "C" fn() -> StringView> = unsafe {
        library
            .get(b"polyplug_get_last_bundle_path\0")
            .expect("polyplug_get_last_bundle_path symbol not found")
    };

    // Build a no-op registrar.
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: noop_register,
        host: core::ptr::null(),
    };

    // Build a PluginContext with a known bundle_path string.
    let bundle_path_str: &str = "/tmp/test_bundle_dir";
    let ctx: PluginContext = PluginContext {
        bundle_path: StringView {
            ptr: bundle_path_str.as_ptr(),
            len: bundle_path_str.len(),
        },
        host_abi_version: POLYPLUG_ABI_VERSION,
    };

    // Call polyplug_init with the crafted context.
    // SAFETY: init_fn is a valid function pointer. registrar and ctx are valid
    // stack-allocated values whose lifetimes span this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &mut registrar as *mut PluginRegistrar,
            &ctx as *const PluginContext,
        )
    };

    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");

    // Call polyplug_get_last_bundle_path to retrieve the stored StringView.
    // SAFETY: get_path_fn is a valid function pointer. The bundle_path_str
    // memory is valid for the duration of this test (stack-allocated above).
    let returned_sv: StringView = unsafe { get_path_fn() };

    // Verify the returned StringView length matches.
    assert_eq!(
        returned_sv.len,
        bundle_path_str.len(),
        "returned StringView.len must match the original bundle_path length"
    );

    // Verify the returned StringView bytes match.
    // SAFETY: returned_sv.ptr points to the same bundle_path_str bytes that
    // were stored by polyplug_init. bundle_path_str is live for the duration
    // of this test. len was just verified to equal bundle_path_str.len().
    let returned_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(returned_sv.ptr, returned_sv.len) };
    assert_eq!(
        returned_bytes,
        bundle_path_str.as_bytes(),
        "returned StringView bytes must match the original bundle_path"
    );

    println!(
        "rust_plugin_receives_bundle_path: bundle_path round-trip verified for {:?} ✓",
        bundle_path_str
    );

    // Leak the library — keeping vtable pointers valid until process exit.
    core::mem::forget(library);
}
