//! Integration tests for the polyplug C facade (polyplug_ffi_* symbols).
//!
//! These tests call the C ABI functions directly from Rust (same process),
//! exercising the same API surface that Lua and Deno use via FFI.
//!
//! Tests requiring native plugin loading are in tests/integration/ffi_native.rs.

#![allow(clippy::expect_used)]

use polyplug::ffi::{
    polyplug_error_message_len, polyplug_last_error, polyplug_load_bundle, polyplug_runtime_free,
    polyplug_runtime_new, OpaqueRuntime,
};

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
