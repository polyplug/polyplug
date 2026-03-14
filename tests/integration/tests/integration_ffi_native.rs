//! FFI native loader integration tests.
//!
//! These tests exercise the polyplug C FFI surface (the same API that Lua and Deno
//! hosts use) for native plugin loading. They require `polyplug_native` to register
//! the native loader.

#![allow(clippy::expect_used)]

use polyplug::ffi::{
    polyplug_error_message_len, polyplug_get_vtable, polyplug_guard_free, polyplug_last_error,
    polyplug_load_bundle, polyplug_rt_find_all_by_contract, polyplug_rt_find_by_contract,
    polyplug_rt_resolve_plugin, polyplug_runtime_free, polyplug_runtime_new,
    polyplug_runtime_register_loader, OpaqueGuard, OpaqueRuntime,
};
use polyplug_native::ffi::{polyplug_native_loader_create, PolyplugNativeConfig};

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");
const TEST_ADD_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B;
const NULL_HANDLE: u64 = u64::MAX;

fn read_last_error() -> String {
    let len: usize = unsafe { polyplug_error_message_len() };
    if len == 0 {
        return String::new();
    }
    let mut buf: Vec<u8> = vec![0u8; len];
    let _written: usize = unsafe { polyplug_last_error(buf.as_mut_ptr(), len) };
    String::from_utf8_lossy(&buf).into_owned()
}

fn create_runtime_with_native_loader() -> *mut OpaqueRuntime {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null(), "polyplug_runtime_new returned null");

    let native_loader: *mut std::ffi::c_void = unsafe {
        polyplug_native_loader_create(core::ptr::null::<PolyplugNativeConfig>() as *mut _)
    };
    assert!(!native_loader.is_null(), "native loader create failed");

    let reg_result: u32 = unsafe { polyplug_runtime_register_loader(rt, native_loader) };
    assert_eq!(
        reg_result,
        0,
        "register native loader failed: {}",
        read_last_error()
    );

    rt
}

#[test]
fn test_load_bundle_succeeds() {
    let rt: *mut OpaqueRuntime = create_runtime_with_native_loader();
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let result: u32 = unsafe { polyplug_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(result, 0, "load_bundle failed: {}", read_last_error());
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_find_by_contract_returns_valid_handle() {
    let rt: *mut OpaqueRuntime = create_runtime_with_native_loader();
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let load_result: u32 =
        unsafe { polyplug_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(load_result, 0, "load failed: {}", read_last_error());
    let handle: u64 = unsafe { polyplug_rt_find_by_contract(rt, TEST_ADD_CONTRACT_ID, 0) };
    assert_ne!(
        handle, NULL_HANDLE,
        "Expected valid handle, got NULL_HANDLE"
    );
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_resolve_plugin_returns_guard() {
    let rt: *mut OpaqueRuntime = create_runtime_with_native_loader();
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let _: u32 = unsafe { polyplug_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    let handle: u64 = unsafe { polyplug_rt_find_by_contract(rt, TEST_ADD_CONTRACT_ID, 0) };
    assert_ne!(handle, NULL_HANDLE);
    let guard: *mut OpaqueGuard = unsafe { polyplug_rt_resolve_plugin(rt, handle) };
    assert!(
        !guard.is_null(),
        "polyplug_rt_resolve_plugin returned null: {}",
        read_last_error()
    );
    unsafe { polyplug_guard_free(guard) };
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_guard_vtable_nonnull() {
    let rt: *mut OpaqueRuntime = create_runtime_with_native_loader();
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let _: u32 = unsafe { polyplug_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    let handle: u64 = unsafe { polyplug_rt_find_by_contract(rt, TEST_ADD_CONTRACT_ID, 0) };
    let guard: *mut OpaqueGuard = unsafe { polyplug_rt_resolve_plugin(rt, handle) };
    assert!(!guard.is_null());
    let vt: *const () = unsafe { polyplug_get_vtable(guard) };
    assert!(!vt.is_null(), "polyplug_get_vtable returned null");
    unsafe { polyplug_guard_free(guard) };
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_find_all_by_contract_returns_results() {
    let rt: *mut OpaqueRuntime = create_runtime_with_native_loader();
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let _: u32 = unsafe { polyplug_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    let mut out_buf: [u64; 8] = [0u64; 8];
    let count: usize = unsafe {
        polyplug_rt_find_all_by_contract(rt, TEST_ADD_CONTRACT_ID, 0, out_buf.as_mut_ptr(), 8)
    };
    assert!(
        count >= 1,
        "Expected at least 1 result for test.add contract"
    );
    assert_ne!(out_buf[0], NULL_HANDLE);
    unsafe { polyplug_runtime_free(rt) };
}
