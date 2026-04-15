#![allow(clippy::expect_used)]

//! Integration tests: null pointer safety of all C facade FFI functions.
//! Every function that takes a pointer must handle null without panicking.

use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::HostInterface;

// Import the host_* functions directly for null-host tests
use polyplug::{host_get_error_len, host_get_last_error, host_load_bundle};

#[test]
fn test_runtime_free_null() {
    // polyplug_runtime_destroy(null) must be a no-op, not a crash
    // SAFETY: passing null is explicitly part of the null-safety contract being tested.
    unsafe { polyplug_runtime_destroy(core::ptr::null()) };
}

#[test]
fn test_load_bundle_null_host() {
    let path: &[u8] = b"/some/path";
    // SAFETY: passing null host is explicitly part of the null-safety contract being tested.
    // We call the underlying host_load_bundle function directly since we don't have a HostInterface.
    let result: polyplug_abi::AbiError =
        unsafe { host_load_bundle(core::ptr::null(), path.as_ptr(), path.len()) };
    assert_eq!(
        result.code,
        polyplug_abi::AbiErrorCode::InvalidPointer,
        "load_bundle(null host) must return InvalidPointer"
    );
}

#[test]
fn test_load_bundle_null_path() {
    // SAFETY: polyplug_runtime_create(core::ptr::null()) returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    // SAFETY: host is valid (asserted above); null path tests the null-safety contract.
    let result: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, core::ptr::null(), 0) };
    assert_eq!(
        result.code,
        polyplug_abi::AbiErrorCode::InvalidPointer,
        "load_bundle(null path) must return InvalidPointer"
    );
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_find_all_guest_contracts_empty_registry() {
    // SAFETY: polyplug_runtime_create(core::ptr::null()) returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    // SAFETY: host is valid (asserted above).
    let arr: polyplug_abi::Array<GuestContractHandle> =
        unsafe { ((*host).find_all_guest_contracts)(host, 0xDEAD_BEEF_u64, 0) };
    // No plugins loaded, so len == 0. Point is: no crash, no panic.
    assert_eq!(
        arr.len, 0,
        "find_all_guest_contracts on empty registry must return empty array"
    );
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_resolve_guest_contract_null_handle() {
    // NULL_HANDLE (GuestContractHandle::null()) — must return null ptr, must NOT set last_error
    // SAFETY: polyplug_runtime_create(core::ptr::null()) returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null());
    // SAFETY: host is valid (asserted above); null handle is the sentinel value.
    let interface: *const polyplug_abi::GuestContractInterface =
        unsafe { ((*host).resolve_guest_contract)(host, GuestContractHandle::null()) };
    assert!(
        interface.is_null(),
        "resolve_guest_contract(null handle) must return null"
    );
    // Verify no last_error was set
    let len: usize = unsafe { ((*host).get_error_len)(host) };
    assert_eq!(len, 0, "error_len must be 0 after null handle resolve");
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(core::ptr::null()).
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_get_last_error_null_host() {
    // get_last_error with null host returns 0 (no host to have an error)
    // SAFETY: passing null host is explicitly part of the null-safety contract being tested.
    // We call the underlying host_get_last_error function directly.
    let n: usize = unsafe { host_get_last_error(core::ptr::null(), core::ptr::null_mut(), 0) };
    assert_eq!(n, 0, "get_last_error(null host) must return 0");
}

#[test]
fn test_get_error_len_null_host() {
    // SAFETY: passing null host is explicitly part of the null-safety contract being tested.
    // We call the underlying host_get_error_len function directly.
    let n: usize = unsafe { host_get_error_len(core::ptr::null()) };
    // Returns length of the null host error message
    assert!(
        n > 0,
        "get_error_len(null host) must return a non-zero length"
    );
}
