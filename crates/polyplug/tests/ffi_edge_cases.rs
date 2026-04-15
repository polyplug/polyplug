#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

//! Edge case tests for the FFI layer.
//!
//! Tests null pointers, stale handles, and buffer boundary conditions
//! for HostInterface operations (find_guest_contract, find_all_guest_contracts, resolve_guest_contract).

use std::path::PathBuf;

use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::HostInterface;

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");
const RELOAD_PLUGIN_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");

const TEST_ADD_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B_u64;

// ─────────────────────────────────────────────────────────────────────────────
// resolve_guest_contract edge cases
// ─────────────────────────────────────────────────────────────────────────────

/// Test `resolve_guest_contract` with null HostInterface pointer.
/// Expected: Returns null.
#[test]
fn test_resolve_plugin_null_host() {
    // SAFETY: Passing null host is explicitly testing the null-safety contract.
    // Call the underlying host_resolve_guest_contract function directly.
    let handle: GuestContractHandle = GuestContractHandle::null();
    let interface: *const polyplug_abi::GuestContractInterface =
        unsafe { polyplug::runtime::host_resolve_guest_contract(core::ptr::null(), handle) };
    assert!(
        interface.is_null(),
        "resolve_guest_contract(null host) must return null"
    );
}

/// Test `resolve_guest_contract` with null handle.
/// Expected: Returns null without setting last_error.
#[test]
fn test_resolve_plugin_null_handle() {
    // SAFETY: polyplug_runtime_create returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    // SAFETY: host is valid; null handle is the sentinel.
    let interface: *const polyplug_abi::GuestContractInterface =
        unsafe { ((*host).resolve_guest_contract)(host, GuestContractHandle::null()) };
    assert!(
        interface.is_null(),
        "resolve_guest_contract(null handle) must return null"
    );

    // SAFETY: host is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// Test `resolve_guest_contract` with stale/invalid handle.
/// Expected: Returns null.
#[test]
fn test_resolve_plugin_stale_handle() {
    // SAFETY: polyplug_runtime_create returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    // Load a plugin to get a valid slot
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    // SAFETY: host is valid; path_bytes is valid UTF-8 for the duration of the call.
    let rc: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(
        rc.code,
        polyplug_abi::AbiErrorCode::Ok,
        "plugin load must succeed"
    );

    // Find the plugin to get a valid handle
    let contract_id: u64 = TEST_ADD_CONTRACT_ID;
    // SAFETY: host is valid.
    let handle: GuestContractHandle =
        unsafe { ((*host).find_guest_contract)(host, contract_id, 0) };
    assert!(!handle.is_null(), "plugin must be found");

    // Create an invalid handle by using an out-of-bounds index
    let invalid_handle: GuestContractHandle = GuestContractHandle {
        index: 999_999_999_u32,
    };

    // SAFETY: host is valid; invalid_handle is a deliberately invalid handle.
    let interface: *const polyplug_abi::GuestContractInterface =
        unsafe { ((*host).resolve_guest_contract)(host, invalid_handle) };
    assert!(
        interface.is_null(),
        "resolve_guest_contract(invalid handle) must return null"
    );

    // SAFETY: host is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

// ─────────────────────────────────────────────────────────────────────────────
// find_all_guest_contracts edge cases
// ─────────────────────────────────────────────────────────────────────────────

/// Test `find_all_guest_contracts` with empty registry.
/// Expected: Returns empty array.
#[test]
fn test_find_all_guest_contracts_empty_registry() {
    // SAFETY: polyplug_runtime_create returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    // SAFETY: host is valid.
    let arr: polyplug_abi::Array<GuestContractHandle> =
        unsafe { ((*host).find_all_guest_contracts)(host, 0xDEAD_BEEF_u64, 0) };
    assert_eq!(
        arr.len, 0,
        "find_all on empty registry must return empty array"
    );

    // SAFETY: host is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// Test `find_all_guest_contracts` with single plugin.
/// Expected: Returns array with one handle.
#[test]
fn test_find_all_guest_contracts_single_plugin() {
    // SAFETY: polyplug_runtime_create returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    // Load reload_plugin_v1 which provides "reload.test@1"
    let v1_path_bytes: &[u8] = RELOAD_PLUGIN_V1_DIR.as_bytes();
    // SAFETY: host is valid; v1_path_bytes is valid UTF-8.
    let rc: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, v1_path_bytes.as_ptr(), v1_path_bytes.len()) };
    assert_eq!(
        rc.code,
        polyplug_abi::AbiErrorCode::Ok,
        "reload_plugin_v1 load must succeed"
    );

    // reload.test@1 contract_id from build.rs
    let contract_id: u64 = 16526955377754357857_u64;

    // SAFETY: host is valid.
    let arr: polyplug_abi::Array<GuestContractHandle> =
        unsafe { ((*host).find_all_guest_contracts)(host, contract_id, 0) };
    assert_eq!(arr.len, 1, "find_all must return exactly 1 result");
    assert!(!arr.items.is_null(), "array items must not be null");
    // SAFETY: arr.items is valid for arr.len elements.
    let handles: &[GuestContractHandle] =
        unsafe { core::slice::from_raw_parts(arr.items, arr.len) };
    assert!(!handles[0].is_null(), "handle must not be null");

    // Free the array via host->free
    // SAFETY: arr.items was allocated by host->alloc with appropriate size/align.
    unsafe {
        ((*host).free)(
            host,
            arr.items as *mut u8,
            arr.len * core::mem::size_of::<GuestContractHandle>(),
            core::mem::align_of::<GuestContractHandle>(),
        );
    }

    // SAFETY: host is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// Test `find_all_guest_contracts` with multiple plugins.
/// Expected: Returns array with multiple handles.
#[test]
fn test_find_all_guest_contracts_multiple_plugins() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        eprintln!(
            "Skipping test_find_all_guest_contracts_multiple_plugins: TEST_PLUGIN_CPP_SO not set"
        );
        return;
    }

    // SAFETY: polyplug_runtime_create returns a valid HostInterface or null on OOM.
    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    // Load test_plugin (Rust) which provides "test.add@1"
    let rust_path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    // SAFETY: host is valid; rust_path_bytes is valid UTF-8.
    let rc_rust: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, rust_path_bytes.as_ptr(), rust_path_bytes.len()) };
    assert_eq!(
        rc_rust.code,
        polyplug_abi::AbiErrorCode::Ok,
        "test_plugin load must succeed"
    );

    // Create a temporary bundle directory for the C++ plugin
    let temp_dir: tempfile::TempDir = tempfile::TempDir::new().expect("failed to create temp dir");
    let cpp_bundle_dir: PathBuf = temp_dir.path().join("cpp_test_plugin");
    std::fs::create_dir_all(&cpp_bundle_dir).expect("failed to create cpp bundle dir");

    // Copy the C++ .so to the bundle directory
    let cpp_so_path: PathBuf = PathBuf::from(TEST_PLUGIN_CPP_SO);
    let cpp_so_filename: &str = cpp_so_path
        .file_name()
        .expect("cpp so has filename")
        .to_str()
        .unwrap();
    std::fs::copy(&cpp_so_path, cpp_bundle_dir.join(cpp_so_filename))
        .expect("failed to copy cpp so");

    let manifest_toml: String = format!(
        "id = 1\nname = \"cpp_test_adder\"\nruntime = \"native\"\nfile = \"{}\"\nversion = \"1.0\"\nprovides = [\"test.add\"]\nfunction_count = {{ \"test.add@1\" = 1 }}\n",
        cpp_so_filename
    );
    std::fs::write(cpp_bundle_dir.join("manifest.toml"), manifest_toml)
        .expect("failed to write manifest");

    // Load the C++ plugin (also provides "test.add@1")
    let cpp_path_str: String = cpp_bundle_dir.to_string_lossy().into_owned();
    let cpp_path_bytes: &[u8] = cpp_path_str.as_bytes();
    // SAFETY: host is valid; cpp_path_bytes is valid UTF-8.
    let rc_cpp: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, cpp_path_bytes.as_ptr(), cpp_path_bytes.len()) };
    if rc_cpp.code != polyplug_abi::AbiErrorCode::Ok {
        let mut err_buf: [u8; 512] = [0_u8; 512];
        // SAFETY: err_buf is a valid stack-allocated buffer; host is valid.
        let err_len: usize =
            unsafe { ((*host).get_last_error)(host, err_buf.as_mut_ptr(), err_buf.len()) };
        let err_msg: &str = core::str::from_utf8(&err_buf[..err_len]).unwrap_or("invalid UTF-8");
        panic!(
            "cpp_test_plugin load failed: {} (path: {})",
            err_msg, cpp_path_str
        );
    }

    // SAFETY: host is valid.
    let arr: polyplug_abi::Array<GuestContractHandle> =
        unsafe { ((*host).find_all_guest_contracts)(host, TEST_ADD_CONTRACT_ID, 0) };
    assert_eq!(arr.len, 2, "find_all must find both plugins");

    // Free the array via host->free
    // SAFETY: arr.items was allocated by host->alloc with appropriate size/align.
    unsafe {
        ((*host).free)(
            host,
            arr.items as *mut u8,
            arr.len * core::mem::size_of::<GuestContractHandle>(),
            core::mem::align_of::<GuestContractHandle>(),
        );
    }

    // SAFETY: host is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}
