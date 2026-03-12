//! Integration tests for the polyplug C facade (polyplug_ffi_* symbols).
//!
//! These tests call the C ABI functions directly from Rust (same process),
//! exercising the same API surface that Lua and Deno use via FFI.
//!
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use polyplug::ffi::{
    OpaqueGuard, OpaqueRuntime, polyplug_error_message_len, polyplug_get_vtable,
    polyplug_guard_free, polyplug_last_error, polyplug_load_bundle,
    polyplug_rt_find_all_by_contract, polyplug_rt_find_by_contract, polyplug_rt_resolve_plugin,
    polyplug_runtime_free, polyplug_runtime_new,
};

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");
// FNV-1a hash of "test.add@1" = 0xCC4232FAB0410D2B
const TEST_ADD_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B;
const NULL_HANDLE: u64 = u64::MAX;

// Helper: reads the current LAST_ERROR string (clears after read).
fn read_last_error() -> String {
    let len: usize = unsafe { polyplug_error_message_len() };
    if len == 0 {
        return String::new();
    }
    let mut buf: Vec<u8> = vec![0u8; len];
    let _written: usize = unsafe { polyplug_last_error(buf.as_mut_ptr(), len) };
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn test_runtime_new_succeeds() {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null(), "polyplug_runtime_new returned null");
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_load_bundle_succeeds() {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let result: u32 = unsafe { polyplug_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(result, 0, "load_bundle failed: {}", read_last_error());
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_find_by_contract_returns_valid_handle() {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    let load_result: u32 =
        unsafe { polyplug_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(load_result, 0);
    let handle: u64 = unsafe { polyplug_rt_find_by_contract(rt, TEST_ADD_CONTRACT_ID, 0) };
    assert_ne!(
        handle, NULL_HANDLE,
        "Expected valid handle, got NULL_HANDLE"
    );
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_resolve_plugin_returns_guard() {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
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
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
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
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
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

#[test]
fn test_null_handle_for_missing_contract() {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    // Contract ID 0 is not a valid FNV-1a hash of any real contract name.
    let handle: u64 = unsafe { polyplug_rt_find_by_contract(rt, 0u64, 0) };
    assert_eq!(
        handle, NULL_HANDLE,
        "Expected NULL_HANDLE for non-existent contract"
    );
    unsafe { polyplug_runtime_free(rt) };
}

#[test]
fn test_last_error_after_failed_load() {
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
    assert!(!rt.is_null());
    let bad_path: &[u8] = b"/does/not/exist";
    let result: u32 = unsafe { polyplug_load_bundle(rt, bad_path.as_ptr(), bad_path.len()) };
    assert_ne!(result, 0, "Expected failure for non-existent path");
    let err: String = read_last_error();
    assert!(
        !err.is_empty(),
        "Expected non-empty error string after failed load"
    );
    unsafe { polyplug_runtime_free(rt) };
}
