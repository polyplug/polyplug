#![allow(clippy::expect_used)]

//! Integration tests: non-UTF-8 bytes passed to HostInterface.load_bundle / reload_bundle
//! must produce an error code and a last_error message, not a panic or UB.

use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug_abi::HostInterface;

/// Helper: read last_error into a String.
fn read_last_error(host: *const HostInterface) -> String {
    let mut buf: Vec<u8> = vec![0_u8; 512];
    // SAFETY: buf is valid for 512 bytes.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[test]
fn test_load_bundle_invalid_utf8_path() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let host: *const HostInterface = unsafe { polyplug_runtime_create() };
    assert!(!host.is_null(), "runtime_new must succeed");
    // Construct a path with invalid UTF-8: \xff\xfe are invalid UTF-8 lead bytes
    let bad_path: &[u8] = &[0xff_u8, 0xfe_u8, b'/', b'p', b'a', b't', b'h'];
    // SAFETY: host is non-null, bad_path.as_ptr() valid for bad_path.len() bytes.
    let result: polyplug_abi::AbiError = unsafe { ((*host).load_bundle)(host, bad_path.as_ptr(), bad_path.len()) };
    assert_ne!(
        result.code, polyplug_abi::AbiErrorCode::Ok,
        "load_bundle with invalid UTF-8 path must return error"
    );
    let err: String = read_last_error(host);
    assert!(
        !err.is_empty(),
        "last_error must be set after invalid UTF-8 path"
    );
    // SAFETY: host was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_reload_bundle_invalid_utf8_path() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let host: *const HostInterface = unsafe { polyplug_runtime_create() };
    assert!(!host.is_null(), "runtime_new must succeed");
    let bad_path: &[u8] = &[0xff_u8, 0xfe_u8, b'/', b'p', b'l', b'u', b'g'];
    // SAFETY: host is non-null, bad_path.as_ptr() valid for bad_path.len() bytes.
    let result: polyplug_abi::AbiError = unsafe { ((*host).reload_bundle)(host, bad_path.as_ptr(), bad_path.len()) };
    assert_ne!(
        result.code, polyplug_abi::AbiErrorCode::Ok,
        "reload_bundle with invalid UTF-8 path must return error"
    );
    let err: String = read_last_error(host);
    assert!(
        !err.is_empty(),
        "last_error must be set after invalid UTF-8 path"
    );
    // SAFETY: host was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_runtime_healthy_after_invalid_utf8() {
    // After a failed load, runtime must still accept a valid load attempt.
    // We test this by attempting a second load with a valid (but non-existent) ASCII path.
    // The second call should fail with a 'file not found' error, NOT a panic.
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let host: *const HostInterface = unsafe { polyplug_runtime_create() };
    assert!(!host.is_null());
    let bad_path: &[u8] = &[0xff_u8, 0xfe_u8];
    // SAFETY: host non-null, bad_path valid for 2 bytes.
    let _ = unsafe { ((*host).load_bundle)(host, bad_path.as_ptr(), bad_path.len()) };
    // Now try a valid ASCII path (non-existent file is OK — just proves runtime didn't break)
    let good_path: &[u8] = b"/tmp/nonexistent_plugin_dir";
    // SAFETY: host non-null, good_path valid for its len bytes.
    let result2: polyplug_abi::AbiError = unsafe { ((*host).load_bundle)(host, good_path.as_ptr(), good_path.len()) };
    // We expect a 'path not found' error, not a panic. result2.code != Ok is expected.
    let err2: String = read_last_error(host);
    assert!(
        !err2.is_empty(),
        "runtime must be healthy and set last_error on second call"
    );
    let _ = result2;
    // SAFETY: host was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(host) };
}