#![allow(clippy::expect_used)]

//! Integration tests: null pointer safety of all C facade FFI functions.
//! Every function that takes a pointer must handle null without panicking.

use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::mem::offset_of;
use core::ptr;

use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::Version;

// Import the host_* functions directly for null-host tests
use polyplug::runtime::{host_get_error_len, host_get_last_error, host_load_bundle};

#[test]
fn test_runtime_free_null() {
    // SAFETY: passing null is explicitly part of the null-safety contract being tested.
    assert!(unsafe { polyplug_runtime_destroy(ptr::null()) });
}

#[test]
fn test_load_bundle_null_host() {
    let path: &[u8] = b"/some/path";
    let mut result: AbiError = AbiError::ok();
    // SAFETY: passing null host is explicitly part of the null-safety contract being tested.
    // We call the underlying host_load_bundle function directly since we don't have a HostApi.
    unsafe { host_load_bundle(ptr::null(), path.as_ptr(), path.len(), &mut result) };
    assert_eq!(
        result.code,
        AbiErrorCode::InvalidPointer as u32,
        "load_bundle(null host) must return InvalidPointer"
    );
}

#[test]
fn test_load_bundle_null_path() {
    // SAFETY: polyplug_runtime_create(ptr::null()) returns a valid HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
    assert!(!host.is_null());
    let mut result: AbiError = AbiError::ok();
    // SAFETY: host is valid (asserted above); null path tests the null-safety contract.
    unsafe { ((*host).load_bundle)(host, ptr::null(), 0, &mut result) };
    assert_eq!(
        result.code,
        AbiErrorCode::InvalidPointer as u32,
        "load_bundle(null path) must return InvalidPointer"
    );
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(ptr::null()).
    assert!(unsafe { polyplug_runtime_destroy(host) });
}

#[test]
fn test_find_all_guest_contracts_empty_registry() {
    // SAFETY: polyplug_runtime_create(ptr::null()) returns a valid HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
    assert!(!host.is_null());
    let mut arr: Array<GuestContractHandle> = Array::empty();
    // SAFETY: host is valid and `arr` is a writable output slot.
    unsafe {
        ((*host).find_all_guest_contracts)(host, 0xDEAD_BEEF_u64, 0, &raw mut arr);
    }
    // No plugins loaded, so len == 0. Point is: no crash, no panic.
    assert_eq!(
        arr.len, 0,
        "find_all_guest_contracts on empty registry must return empty array"
    );
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(ptr::null()).
    assert!(unsafe { polyplug_runtime_destroy(host) });
}

#[test]
fn test_resolve_guest_contract_null_handle() {
    // NULL_HANDLE (GuestContractHandle::null()) — must return null ptr, must NOT set last_error
    // SAFETY: polyplug_runtime_create(ptr::null()) returns a valid HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
    assert!(!host.is_null());
    // SAFETY: host is valid (asserted above); null handle is the sentinel value.
    let interface: *const GuestContractInterface =
        unsafe { ((*host).resolve_guest_contract)(host, GuestContractHandle::null()) };
    assert!(
        interface.is_null(),
        "resolve_guest_contract(null handle) must return null"
    );
    // Verify no last_error was set
    // SAFETY: host is valid (asserted above) and was returned by polyplug_runtime_create.
    let len: usize = unsafe { ((*host).get_error_len)(host) };
    assert_eq!(len, 0, "error_len must be 0 after null handle resolve");
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(ptr::null()).
    assert!(unsafe { polyplug_runtime_destroy(host) });
}

#[test]
fn test_get_last_error_null_host() {
    // get_last_error with null host returns 0 (no host to have an error)
    // SAFETY: passing null host is explicitly part of the null-safety contract being tested.
    // We call the underlying host_get_last_error function directly.
    let n: usize = unsafe { host_get_last_error(ptr::null(), ptr::null_mut(), 0) };
    assert_eq!(n, 0, "get_last_error(null host) must return 0");
}

#[test]
fn test_get_error_len_null_host() {
    // SAFETY: passing null host is explicitly part of the null-safety contract being tested.
    // We call the underlying host_get_error_len function directly.
    let n: usize = unsafe { host_get_error_len(ptr::null()) };
    // Returns length of the null host error message
    assert!(
        n > 0,
        "get_error_len(null host) must return a non-zero length"
    );
}

// ─── Null fn-pointer rejection at registration (Option-nullability sweep) ────
//
// The REQUIRED fn-pointer fields (`create_instance`, `destroy_instance`,
// `dispatch.vm.call`, the populated prefix of `dispatch.native.functions`)
// stay bare `fn` types in the ABI: failure is signalled through null *instance
// handles*, never null callbacks. A foreign producer can still hand the
// runtime null bits in those slots; registration must reject them with a
// precise InvalidPointer error instead of deferring the crash to first use
// (the Wave-3 null-create_instance crash class). The interfaces below are
// built as raw byte images because a typed Rust struct cannot even represent
// a null bare `fn` value.

/// Build a zeroed GuestContractInterface image and stamp raw bits into the
/// chosen slots. Returned as MaybeUninit so the null fn-pointer slots are
/// never materialized at their typed form.
fn guest_iface_image(
    create_bits: usize,
    destroy_bits: usize,
    dispatch_type: u32,
    dispatch_word0: usize,
    dispatch_word1: usize,
) -> Box<MaybeUninit<GuestContractInterface>> {
    let mut image: Box<MaybeUninit<GuestContractInterface>> = Box::new(MaybeUninit::zeroed());
    let base: *mut u8 = image.as_mut_ptr().cast::<u8>();
    // SAFETY: every write below is in-bounds for the boxed interface image and
    // touches raw integer/pointer bits only — the fn-pointer slots are never
    // read (or written) at their `fn` type.
    unsafe {
        base.add(offset_of!(GuestContractInterface, dispatch_type))
            .cast::<u32>()
            .write(dispatch_type);
        base.add(offset_of!(GuestContractInterface, create_instance))
            .cast::<usize>()
            .write(create_bits);
        base.add(offset_of!(GuestContractInterface, destroy_instance))
            .cast::<usize>()
            .write(destroy_bits);
        let dispatch_base: *mut u8 = base.add(offset_of!(GuestContractInterface, dispatch));
        dispatch_base.cast::<usize>().write(dispatch_word0);
        dispatch_base.add(8).cast::<usize>().write(dispatch_word1);
    }
    image
}

/// Any non-null code address to stamp into a slot that must pass the
/// non-null check (the function is never invoked by these tests).
fn nonnull_fn_bits() -> usize {
    test_get_error_len_null_host as fn() as *const c_void as usize
}

fn make_descriptor(name: &'static [u8]) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name),
        contract_name: StringView::from_static(name),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    }
}

fn register_and_expect_rejection(
    image: &MaybeUninit<GuestContractInterface>,
    expected_fragment: &str,
) {
    // SAFETY: polyplug_runtime_create(ptr::null()) returns a valid HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
    assert!(!host.is_null());
    let descriptor: PluginDescriptor = make_descriptor(b"null_fn_test");
    let mut result: AbiError = AbiError::ok();
    // SAFETY: host is valid; descriptor and the interface image are live for
    // the duration of the call. The image intentionally carries null bits in
    // REQUIRED fn-pointer slots — the call must reject, not crash.
    unsafe { ((*host).register_guest_contract)(host, &descriptor, image.as_ptr(), &mut result) };
    assert_eq!(
        result.code,
        AbiErrorCode::InvalidPointer as u32,
        "registration must reject with InvalidPointer"
    );
    // SAFETY: the message is a static StringView produced by the runtime.
    let message: &str = unsafe { result.message.try_as_str() }.expect("static UTF-8 message");
    assert!(
        message.contains(expected_fragment),
        "error message must name the offending field; got: {message}"
    );
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(ptr::null()).
    assert!(unsafe { polyplug_runtime_destroy(host) });
}

#[test]
fn register_guest_contract_rejects_null_create_instance() {
    let image: Box<MaybeUninit<GuestContractInterface>> =
        guest_iface_image(0, nonnull_fn_bits(), 0, 0, 0);
    register_and_expect_rejection(&image, "create_instance");
}

#[test]
fn register_guest_contract_rejects_null_destroy_instance() {
    let image: Box<MaybeUninit<GuestContractInterface>> =
        guest_iface_image(nonnull_fn_bits(), 0, 0, 0, 0);
    register_and_expect_rejection(&image, "destroy_instance");
}

#[test]
fn register_guest_contract_rejects_null_vm_call() {
    // dispatch_type 1 = VirtualMachine; dispatch.vm.call (union offset 0) is null.
    let image: Box<MaybeUninit<GuestContractInterface>> =
        guest_iface_image(nonnull_fn_bits(), nonnull_fn_bits(), 1, 0, 0);
    register_and_expect_rejection(&image, "dispatch.vm.call");
}

#[test]
fn register_guest_contract_rejects_null_native_function_table() {
    // dispatch_type 0 = Native; function_count = 3 but functions pointer is null.
    let image: Box<MaybeUninit<GuestContractInterface>> =
        guest_iface_image(nonnull_fn_bits(), nonnull_fn_bits(), 0, 3, 0);
    register_and_expect_rejection(&image, "dispatch.native.functions");
}

#[test]
fn register_guest_contract_rejects_null_native_function_entry() {
    // dispatch_type 0 = Native; the table is non-null but entry [1] of 2 is null.
    let table: [usize; 2] = [nonnull_fn_bits(), 0];
    let image: Box<MaybeUninit<GuestContractInterface>> = guest_iface_image(
        nonnull_fn_bits(),
        nonnull_fn_bits(),
        0,
        2,
        table.as_ptr() as usize,
    );
    register_and_expect_rejection(&image, "null entry");
}

#[test]
fn register_host_contract_rejects_null_create_instance() {
    // Build a HostContractInterface image with null create_instance bits.
    let mut image: Box<MaybeUninit<HostContractInterface>> = Box::new(MaybeUninit::zeroed());
    let base: *mut u8 = image.as_mut_ptr().cast::<u8>();
    // SAFETY: in-bounds raw writes into the boxed image; the null fn-pointer
    // slot is never read at its `fn` type.
    unsafe {
        base.add(offset_of!(HostContractInterface, destroy_instance))
            .cast::<usize>()
            .write(nonnull_fn_bits());
    }

    // SAFETY: polyplug_runtime_create(ptr::null()) returns a valid HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
    assert!(!host.is_null());
    let mut result: AbiError = AbiError::ok();
    // SAFETY: host is valid; the image is live for the call and intentionally
    // carries null create_instance bits — the call must reject, not crash (the
    // pre-validation behaviour deferred this to a crash inside get_host_contract).
    unsafe { ((*host).register_host_contract)(host, image.as_ptr(), &mut result) };
    assert_eq!(
        result.code,
        AbiErrorCode::InvalidPointer as u32,
        "register_host_contract must reject a null create_instance with InvalidPointer"
    );
    // SAFETY: the message is a static StringView produced by the runtime.
    let message: &str = unsafe { result.message.try_as_str() }.expect("static UTF-8 message");
    assert!(
        message.contains("create_instance"),
        "error message must name the offending field; got: {message}"
    );
    // SAFETY: host is valid and was allocated by polyplug_runtime_create(ptr::null()).
    assert!(unsafe { polyplug_runtime_destroy(host) });
}
