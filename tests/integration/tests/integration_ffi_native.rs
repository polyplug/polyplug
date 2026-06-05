//! FFI native loader integration tests.
//!
//! These tests exercise the polyplug C FFI surface (the same API that Lua and Deno
//! hosts use) for native plugin loading. The FFI exposes only two free functions —
//! `polyplug_runtime_create` and `polyplug_runtime_destroy` — and routes every other
//! operation through the `HostApi` function-pointer table.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use core::ffi::c_void;

use polyplug::ffi::{polyplug_runtime_create, polyplug_runtime_destroy};
use polyplug::loader::BundleLoader;
use polyplug_abi::{Array, GuestContractHandle, GuestContractInterface, HostApi, StringView};
use polyplug_native::NativeLoader;

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");

fn test_add_contract_id() -> u64 {
    polyplug_utils::guest_contract_id("test.add", 1)
}

fn read_last_error(host: *const HostApi) -> String {
    // SAFETY: host is a valid HostApi pointer for the lifetime of this call.
    let len: usize = unsafe { ((*host).get_error_len)(host) };
    if len == 0 {
        return String::new();
    }
    let mut buf: Vec<u8> = vec![0u8; len];
    // SAFETY: buf has capacity `len` and host is valid.
    let written: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), len) };
    buf.truncate(written);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Register the native loader through the HostApi `register_loader` pointer.
fn register_native_loader(host: *const HostApi) {
    // Double-box so the fat trait-object pointer survives the thin `*mut c_void`.
    let trait_obj: Box<dyn BundleLoader> = Box::new(NativeLoader::new(Default::default()));
    let loader_ptr: *mut c_void = Box::into_raw(Box::new(trait_obj)) as *mut c_void;
    let runtime_name: StringView = StringView::from_static(b"native");
    // SAFETY: host is valid, runtime_name borrows a 'static slice, loader_ptr is a freshly
    // leaked Box<Box<dyn BundleLoader>> that the runtime takes ownership of.
    let err = unsafe { ((*host).register_loader)(host, runtime_name, loader_ptr) };
    assert_eq!(
        err.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "register_loader failed: {}",
        read_last_error(host)
    );
}

fn load_bundle(host: *const HostApi, dir: &str) -> polyplug_abi::AbiError {
    let bytes: &[u8] = dir.as_bytes();
    // SAFETY: host is valid; bytes points to `len` valid UTF-8 bytes.
    unsafe { ((*host).load_bundle)(host, bytes.as_ptr(), bytes.len()) }
}

#[test]
fn test_runtime_create_and_destroy() {
    // SAFETY: null config selects defaults.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "polyplug_runtime_create returned null");

    // SAFETY: host is non-null and points to a valid HostApi.
    let runtime: *mut c_void = unsafe { (*host).runtime };
    assert!(!runtime.is_null(), "HostApi.runtime must be set");

    // SAFETY: host was produced by polyplug_runtime_create and not yet destroyed.
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_load_bundle_fails_without_registered_loader() {
    // SAFETY: null config selects defaults.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "polyplug_runtime_create returned null");

    let err: polyplug_abi::AbiError = load_bundle(host, TEST_PLUGIN_DIR);
    assert_ne!(
        err.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "load_bundle must fail when no loader is registered"
    );
    assert!(
        !read_last_error(host).is_empty(),
        "an error message must be retrievable after a failed load"
    );

    // SAFETY: host is valid and not yet destroyed.
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_native_loader_ffi_workflow() {
    // SAFETY: null config selects defaults.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "polyplug_runtime_create returned null");

    register_native_loader(host);

    // 1. Load the native test bundle.
    let err: polyplug_abi::AbiError = load_bundle(host, TEST_PLUGIN_DIR);
    assert_eq!(
        err.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "load_bundle failed: {}",
        read_last_error(host)
    );

    let contract_id: u64 = test_add_contract_id();

    // 2. Find the contract by id.
    // SAFETY: host is valid.
    let handle: GuestContractHandle =
        unsafe { ((*host).find_guest_contract)(host, contract_id, 0) };
    assert!(!handle.is_null(), "expected a valid handle for test.add");

    // 3. Resolve the handle to a vtable pointer.
    // SAFETY: host is valid and handle came from find_guest_contract.
    let vtable: *const GuestContractInterface =
        unsafe { ((*host).resolve_guest_contract)(host, handle) };
    assert!(!vtable.is_null(), "resolve_guest_contract returned null");

    // 4. Find all providers and free the returned array via the host allocator.
    // SAFETY: host is valid.
    let all: Array<GuestContractHandle> =
        unsafe { ((*host).find_all_guest_contracts)(host, contract_id, 0) };
    assert!(all.len >= 1, "expected at least one provider for test.add");
    assert!(!all.items.is_null(), "provider array must be allocated");
    // SAFETY: `all` was allocated by the host allocator; free with matching size/align.
    unsafe {
        ((*host).free)(
            host,
            all.items as *mut u8,
            all.len * core::mem::size_of::<GuestContractHandle>(),
            all.align,
        )
    };

    // SAFETY: host is valid and not yet destroyed.
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_unknown_contract_returns_null_handle() {
    // SAFETY: null config selects defaults.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "polyplug_runtime_create returned null");

    register_native_loader(host);

    let err: polyplug_abi::AbiError = load_bundle(host, TEST_PLUGIN_DIR);
    assert_eq!(
        err.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "load_bundle failed: {}",
        read_last_error(host)
    );

    // SAFETY: host is valid; the id is intentionally unknown.
    let handle: GuestContractHandle =
        unsafe { ((*host).find_guest_contract)(host, 0xDEAD_BEEF_DEAD_BEEF, 0) };
    assert!(
        handle.is_null(),
        "unknown contract must yield the null handle"
    );

    // SAFETY: host is valid and not yet destroyed.
    unsafe { polyplug_runtime_destroy(host) };
}

#[test]
fn test_resolve_null_handle_returns_null() {
    // SAFETY: null config selects defaults.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "polyplug_runtime_create returned null");

    // SAFETY: host is valid; resolving the null handle must not dispatch.
    let vtable: *const GuestContractInterface =
        unsafe { ((*host).resolve_guest_contract)(host, GuestContractHandle::null()) };
    assert!(
        vtable.is_null(),
        "resolving the null handle must yield null"
    );

    // SAFETY: host is valid and not yet destroyed.
    unsafe { polyplug_runtime_destroy(host) };
}
