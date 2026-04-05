#![allow(clippy::expect_used)]

//! Integration tests: non-UTF-8 bytes passed to polyplug_runtime_load_bundle / polyplug_runtime_reload_bundle
//! must produce a non-zero return code and a last_error message, not a panic or UB.

use polyplug::ffi::OpaqueRuntime;
use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug::ffi::polyplug_runtime_last_error;
use polyplug::ffi::polyplug_runtime_load_bundle;
use polyplug::ffi::polyplug_runtime_reload_bundle;

/// Helper: read last_error into a String.
fn read_last_error(rt: *const OpaqueRuntime) -> String {
    let mut buf: Vec<u8> = vec![0_u8; 512];
    // SAFETY: buf is valid for 512 bytes.
    let n: usize = unsafe { polyplug_runtime_last_error(rt, buf.as_mut_ptr(), buf.len()) };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[test]
fn test_load_bundle_invalid_utf8_path() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime_new must succeed");
    // Construct a path with invalid UTF-8: \xff\xfe are invalid UTF-8 lead bytes
    let bad_path: &[u8] = &[0xff_u8, 0xfe_u8, b'/', b'p', b'a', b't', b'h'];
    // SAFETY: rt is non-null, bad_path.as_ptr() valid for bad_path.len() bytes.
    let rc: u32 = unsafe { polyplug_runtime_load_bundle(rt, bad_path.as_ptr(), bad_path.len()) };
    assert_ne!(
        rc, 0,
        "load_bundle with invalid UTF-8 path must return non-zero"
    );
    let err: String = read_last_error(rt as *const OpaqueRuntime);
    assert!(
        !err.is_empty(),
        "last_error must be set after invalid UTF-8 path"
    );
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_reload_bundle_invalid_utf8_path() {
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime_new must succeed");
    let bad_path: &[u8] = &[0xff_u8, 0xfe_u8, b'/', b'p', b'l', b'u', b'g'];
    // SAFETY: rt is non-null, bad_path.as_ptr() valid for bad_path.len() bytes.
    let rc: u32 = unsafe { polyplug_runtime_reload_bundle(rt, bad_path.as_ptr(), bad_path.len()) };
    assert_ne!(
        rc, 0,
        "reload_bundle with invalid UTF-8 path must return non-zero"
    );
    let err: String = read_last_error(rt as *const OpaqueRuntime);
    assert!(
        !err.is_empty(),
        "last_error must be set after invalid UTF-8 path"
    );
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_runtime_healthy_after_invalid_utf8() {
    // After a failed load, runtime must still accept a valid load attempt.
    // We test this by attempting a second load with a valid (but non-existent) ASCII path.
    // The second call should fail with a 'file not found' error, NOT a panic.
    // SAFETY: polyplug_runtime_create() has no preconditions.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    let bad_path: &[u8] = &[0xff_u8, 0xfe_u8];
    // SAFETY: rt non-null, bad_path valid for 2 bytes.
    let _ = unsafe { polyplug_runtime_load_bundle(rt, bad_path.as_ptr(), bad_path.len()) };
    // Now try a valid ASCII path (non-existent file is OK — just proves runtime didn't break)
    let good_path: &[u8] = b"/tmp/nonexistent_plugin_dir";
    // SAFETY: rt non-null, good_path valid for its len bytes.
    let rc2: u32 = unsafe { polyplug_runtime_load_bundle(rt, good_path.as_ptr(), good_path.len()) };
    // We expect a 'path not found' error, not a panic. rc2 != 0 is expected.
    let err2: String = read_last_error(rt as *const OpaqueRuntime);
    assert!(
        !err2.is_empty(),
        "runtime must be healthy and set last_error on second call"
    );
    let _ = rc2;
    // SAFETY: rt was returned by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}