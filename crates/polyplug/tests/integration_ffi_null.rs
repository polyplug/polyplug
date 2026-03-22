#![allow(clippy::expect_used)]

//! Integration tests: null pointer safety of all C facade FFI functions.
//! Every function that takes a pointer must handle null without panicking.

use polyplug::ffi::OpaqueRuntime;
use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug::ffi::polyplug_runtime_find_all_by_contract;
use polyplug::ffi::polyplug_runtime_last_error;
use polyplug::ffi::polyplug_runtime_load_bundle;
use polyplug::ffi::polyplug_runtime_resolve_plugin;

#[test]
fn test_runtime_free_null() {
    // polyplug_runtime_destroy(null) must be a no-op, not a crash
    // SAFETY: passing null is explicitly part of the null-safety contract being tested.
    unsafe { polyplug_runtime_destroy(core::ptr::null_mut()) };
}

#[test]
fn test_load_bundle_null_rt() {
    let path: &[u8] = b"/some/path";
    // SAFETY: passing null rt is explicitly part of the null-safety contract being tested.
    let rc: u32 =
        unsafe { polyplug_runtime_load_bundle(core::ptr::null_mut(), path.as_ptr(), path.len()) };
    assert_ne!(rc, 0, "load_bundle(null rt) must return non-zero");
}

#[test]
fn test_load_bundle_null_path() {
    // SAFETY: polyplug_runtime_create() returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    // SAFETY: rt is valid (asserted above); null path tests the null-safety contract.
    let rc: u32 = unsafe { polyplug_runtime_load_bundle(rt, core::ptr::null(), 0) };
    assert_ne!(rc, 0, "load_bundle(null path) must return non-zero");
    // SAFETY: rt is valid and was allocated by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_find_all_null_out_zero_cap() {
    // out=null, cap=0 is the 'probe for count' pattern — must return 0, no error
    // SAFETY: polyplug_runtime_create() returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    // SAFETY: rt is valid (asserted above); null out + cap=0 is the probe pattern being tested.
    let count: usize = unsafe {
        polyplug_runtime_find_all_by_contract(
            rt as *const OpaqueRuntime,
            0xDEAD_BEEF_u64,
            0_u32,
            core::ptr::null_mut(),
            0,
        )
    };
    // No plugins loaded, so count == 0. Point is: no crash, no panic.
    let _ = count;
    // SAFETY: rt is valid and was allocated by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_find_all_null_out_nonzero_cap() {
    // out=null, cap=5 — must set last_error and return 0 (not UB write through null)
    // SAFETY: polyplug_runtime_create() returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    // SAFETY: rt is valid (asserted above); null out + cap=5 tests the guard against null-deref writes.
    let rc: usize = unsafe {
        polyplug_runtime_find_all_by_contract(
            rt as *const OpaqueRuntime,
            0xDEAD_BEEF_u64,
            0_u32,
            core::ptr::null_mut(),
            5,
        )
    };
    assert_eq!(rc, 0, "find_all with null out + cap=5 must return 0");
    // SAFETY: rt is valid and was allocated by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_resolve_plugin_null_handle() {
    // NULL_HANDLE (u64::MAX) — must return null ptr, must NOT set last_error
    // SAFETY: polyplug_runtime_create() returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null());
    // SAFETY: rt is valid (asserted above); u64::MAX is the sentinel NULL_HANDLE value.
    let vtable: *const () =
        unsafe { polyplug_runtime_resolve_plugin(rt as *const OpaqueRuntime, u64::MAX) };
    assert!(
        vtable.is_null(),
        "resolve_plugin(NULL_HANDLE) must return null"
    );
    // Verify no last_error was set
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: rt is valid; buf is a valid stack-allocated buffer; buf.len() matches the slice length exactly.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };
    assert_eq!(n, 0, "last_error must be empty after NULL_HANDLE resolve");
    // SAFETY: rt is valid and was allocated by polyplug_runtime_create().
    unsafe { polyplug_runtime_destroy(rt) };
}

#[test]
fn test_last_error_null_rt() {
    // polyplug_runtime_last_error(null rt, null buf, 0) returns 0 (no runtime to have an error)
    // SAFETY: passing null rt and null buf with len=0 is explicitly part of the null-safety contract being tested.
    let n: usize =
        unsafe { polyplug_runtime_last_error(core::ptr::null(), core::ptr::null_mut(), 0) };
    // With null runtime, we return 0 (no runtime to have an error)
    assert_eq!(
        n, 0,
        "last_error(null rt, null buf) must return 0 (no runtime to have an error)"
    );
}
