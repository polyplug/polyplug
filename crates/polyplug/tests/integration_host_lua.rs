//! Integration tests for the polyplug C facade (polyplug_ffi_* symbols).
//!
//! These tests call the C ABI functions directly from Rust (same process),
//! exercising the same API surface that Lua and Deno use via FFI.
//!
//! Tests requiring native plugin loading are in tests/integration/ffi_native.rs.

use polyplug::ffi::{
    OpaqueRuntime, polyplug_runtime_create, polyplug_runtime_destroy,
    polyplug_runtime_error_message_len, polyplug_runtime_last_error, polyplug_runtime_load_bundle,
};

fn read_last_error() -> String {
    // SAFETY: polyplug_runtime_error_message_len has no pointer preconditions.
    let len: usize = unsafe { polyplug_runtime_error_message_len() };
    if len == 0 {
        return String::new();
    }
    let mut buf: Vec<u8> = vec![0u8; len];
    // SAFETY: buf is a valid allocation of `len` bytes.
    let _written: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), len) };
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn test_runtime_new_succeeds() {
    // SAFETY: polyplug_runtime_create has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "polyplug_runtime_create returned null");
    // SAFETY: rt is non-null, returned by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_last_error_after_failed_load() {
    // SAFETY: polyplug_runtime_create has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    let bad_path: &[u8] = b"/does/not/exist";
    // SAFETY: rt is non-null; bad_path ptr/len are valid for the slice.
    let result: u32 =
        unsafe { polyplug_runtime_load_bundle(rt, bad_path.as_ptr(), bad_path.len()) };
    assert_ne!(result, 0, "Expected failure for non-existent path");
    let err: String = read_last_error();
    assert!(
        !err.is_empty(),
        "Expected non-empty error string after failed load"
    );
    // SAFETY: rt is non-null, returned by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}
