#![allow(clippy::expect_used)]

//! Integration tests for the polyplug C facade (HostInterface-based API).
//!
//! These tests call the HostInterface methods directly from Rust (same process),
//! exercising the same API surface that Lua and Deno use via FFI.
//!
//! Tests requiring native plugin loading are in tests/integration/ffi_native.rs.

use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug_abi::HostInterface;

fn read_last_error(host: *const HostInterface) -> String {
    // SAFETY: host is a valid HostInterface pointer.
    let len: usize = unsafe { ((*host).get_error_len)(host) };
    if len == 0 {
        return String::new();
    }
    let mut buf: Vec<u8> = vec![0u8; len];
    // SAFETY: host is valid; buf is a valid allocation of `len` bytes.
    let _written: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), len) };
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn test_runtime_new_succeeds() {
    // SAFETY: polyplug_runtime_create has no preconditions.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "polyplug_runtime_create returned null");
    // SAFETY: host is non-null, returned by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_last_error_after_failed_load() {
    // SAFETY: polyplug_runtime_create has no preconditions.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    let bad_path: &[u8] = b"/does/not/exist";
    // SAFETY: host is non-null; bad_path ptr/len are valid for the slice.
    let result: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, bad_path.as_ptr(), bad_path.len()) };
    assert_ne!(result.code, polyplug_abi::AbiErrorCode::Ok, "Expected failure for non-existent path");
    let err: String = read_last_error(host);
    assert!(
        !err.is_empty(),
        "Expected non-empty error string after failed load"
    );
    // SAFETY: host is non-null, returned by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}