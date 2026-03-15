//! FFI native loader integration tests.
//!
//! These tests exercise the polyplug C FFI surface (the same API that Lua and Deno
//! hosts use) for native plugin loading. They require `polyplug_native` to register
//! the native loader.

#![allow(clippy::expect_used)]

use polyplug::ffi::{
    OpaquePluginGuard, OpaqueRuntime, polyplug_runtime_create, polyplug_runtime_destroy,
    polyplug_runtime_error_message_len, polyplug_runtime_find_all_by_contract,
    polyplug_runtime_find_by_contract, polyplug_runtime_last_error, polyplug_runtime_load_bundle,
    polyplug_runtime_plugin_release, polyplug_runtime_plugin_vtable,
    polyplug_runtime_register_loader, polyplug_runtime_resolve_plugin,
};
use polyplug_native::ffi::{PolyplugNativeConfig, polyplug_native_loader_create};

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");
const TEST_ADD_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B;
const NULL_HANDLE: u64 = u64::MAX;

fn read_last_error() -> String {
    let len: usize = unsafe { polyplug_runtime_error_message_len() };
    if len == 0 {
        return String::new();
    }
    let mut buf: Vec<u8> = vec![0u8; len];
    let _written: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), len) };
    String::from_utf8_lossy(&buf).into_owned()
}

fn create_runtime_with_native_loader() -> *mut OpaqueRuntime {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "polyplug_runtime_create returned null");
    rt
}

#[test]
fn test_native_loader_ffi_workflow() {
    let rt: *mut OpaqueRuntime = create_runtime_with_native_loader();

    // 1. Load bundle
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let result: u32 =
        unsafe { polyplug_runtime_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(result, 0, "load_bundle failed: {}", read_last_error());

    // 2. Find by contract
    let handle: u64 = unsafe { polyplug_runtime_find_by_contract(rt, TEST_ADD_CONTRACT_ID, 0) };
    assert_ne!(
        handle, NULL_HANDLE,
        "Expected valid handle, got NULL_HANDLE"
    );

    // 3. Resolve to guard
    let guard: *mut OpaquePluginGuard = unsafe { polyplug_runtime_resolve_plugin(rt, handle) };
    assert!(
        !guard.is_null(),
        "polyplug_rt_resolve_plugin returned null: {}",
        read_last_error()
    );

    // 4. Check vtable non-null
    let vt: *const () = unsafe { polyplug_runtime_plugin_vtable(guard) };
    assert!(!vt.is_null(), "polyplug_get_vtable returned null");

    // Cleanup
    unsafe { polyplug_runtime_plugin_release(guard) };

    // 5. Find all by contract
    let mut out_buf: [u64; 8] = [0u64; 8];
    let count: usize = unsafe {
        polyplug_runtime_find_all_by_contract(rt, TEST_ADD_CONTRACT_ID, 0, out_buf.as_mut_ptr(), 8)
    };
    assert!(
        count >= 1,
        "Expected at least 1 result for test.add contract"
    );
    assert_ne!(out_buf[0], NULL_HANDLE);

    // Cleanup runtime
    unsafe { polyplug_runtime_destroy(rt) };
}
