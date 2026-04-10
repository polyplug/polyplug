#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

//! Edge case tests for the FFI layer.
//!
//! Tests null pointers, stale handles, and buffer boundary conditions
//! for `polyplug_runtime_resolve_guest_contract` and `polyplug_runtime_find_all_by_contract`.

use std::path::PathBuf;

use polyplug::ffi::OpaqueRuntime;
use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug::ffi::polyplug_runtime_find_all_by_contract;
use polyplug::ffi::polyplug_runtime_find_guest_contract;
use polyplug::ffi::polyplug_runtime_last_error;
use polyplug::ffi::polyplug_runtime_load_bundle;
use polyplug::ffi::polyplug_runtime_resolve_guest_contract;

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");
const RELOAD_PLUGIN_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");

const TEST_ADD_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B_u64;

// ─────────────────────────────────────────────────────────────────────────────
// resolve_plugin edge cases
// ─────────────────────────────────────────────────────────────────────────────

/// Test `resolve_plugin` with null runtime pointer.
/// Expected: Returns null, last_error returns 0 for null runtime.
#[test]
fn test_resolve_plugin_null_runtime() {
    // SAFETY: Passing null runtime is explicitly testing the null-safety contract.
    // polyplug_runtime_resolve_guest_contract returns *const GuestContractInterface now.
    let interface: *const polyplug_abi::GuestContractInterface =
        unsafe { polyplug_runtime_resolve_guest_contract(core::ptr::null(), 0x1234_5678_u64) };
    assert!(interface.is_null(), "resolve_plugin(null rt) must return null");

    // Verify last_error returns 0 for null runtime (no runtime to have an error)
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: buf is a valid stack-allocated buffer; null rt is valid for this call.
    let n: usize =
        unsafe { polyplug_runtime_last_error(core::ptr::null(), buf.as_mut_ptr(), buf.len()) };
    assert!(
        n == 0,
        "last_error must return 0 for null runtime (no runtime to have an error)"
    );
}

/// Test `resolve_plugin` with null handle (u64::MAX).
/// Expected: Returns null without setting last_error.
#[test]
fn test_resolve_plugin_null_handle() {
    // SAFETY: polyplug_runtime_create returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    // SAFETY: rt is valid; u64::MAX is the sentinel NULL_HANDLE value.
    let interface: *const polyplug_abi::GuestContractInterface =
        unsafe { polyplug_runtime_resolve_guest_contract(rt as *const OpaqueRuntime, u64::MAX) };
    assert!(
        interface.is_null(),
        "resolve_plugin(NULL_HANDLE) must return null"
    );

    // Verify no last_error was set (null handle is a valid sentinel, not an error)
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: buf is a valid stack-allocated buffer; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };
    assert_eq!(n, 0, "last_error must be empty after NULL_HANDLE resolve");

    // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// Test `resolve_plugin` with stale/invalid handle.
/// Expected: Returns null, may set last_error.
/// Note: With the new GuestContractHandle (index only, no generation), stale handles
/// are detected differently. An out-of-bounds index returns null.
#[test]
fn test_resolve_plugin_stale_handle() {
    // SAFETY: polyplug_runtime_create returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    // Load a plugin to get a valid slot
    let path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    // SAFETY: rt is valid; path_bytes is valid UTF-8 for the duration of the call.
    let rc: u32 =
        unsafe { polyplug_runtime_load_bundle(rt, path_bytes.as_ptr(), path_bytes.len()) };
    assert_eq!(rc, 0, "plugin load must succeed");

    // Find the plugin to get a valid handle
    let contract_id: u64 = TEST_ADD_CONTRACT_ID;
    // SAFETY: rt is valid.
    let packed_handle: u64 =
        unsafe { polyplug_runtime_find_guest_contract(rt as *const OpaqueRuntime, contract_id, 0) };
    assert_ne!(packed_handle, u64::MAX, "plugin must be found");

    // Create an invalid handle by using an out-of-bounds index
    // packed_handle format: just index as u64
    let invalid_index: u64 = 999_999_999_u64; // Clearly out of bounds

    // SAFETY: rt is valid; invalid_index is a deliberately invalid handle.
    let interface: *const polyplug_abi::GuestContractInterface =
        unsafe { polyplug_runtime_resolve_guest_contract(rt as *const OpaqueRuntime, invalid_index) };
    assert!(
        interface.is_null(),
        "resolve_plugin(invalid handle) must return null"
    );

    // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

// ─────────────────────────────────────────────────────────────────────────────
// find_all_by_contract edge cases
// ─────────────────────────────────────────────────────────────────────────────

/// Test `find_all_by_contract` with zero capacity buffer.
/// Expected: Returns 0, no crash.
#[test]
fn test_find_all_by_contract_zero_capacity() {
    // SAFETY: polyplug_runtime_create returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    // SAFETY: rt is valid; null out with cap=0 is the probe pattern.
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
    assert_eq!(count, 0, "find_all with zero capacity must return 0");

    // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// Test `find_all_by_contract` with exact capacity match.
/// Expected: All results fit, returns correct count.
#[test]
fn test_find_all_by_contract_exact_capacity() {
    // SAFETY: polyplug_runtime_create returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    // Load reload_plugin_v1 which provides "reload.test@1"
    let v1_path_bytes: &[u8] = RELOAD_PLUGIN_V1_DIR.as_bytes();
    // SAFETY: rt is valid; v1_path_bytes is valid UTF-8.
    let rc: u32 =
        unsafe { polyplug_runtime_load_bundle(rt, v1_path_bytes.as_ptr(), v1_path_bytes.len()) };
    assert_eq!(rc, 0, "reload_plugin_v1 load must succeed");

    // reload.test@1 contract_id from build.rs
    let contract_id: u64 = 16526955377754357857_u64;

    // Buffer with capacity 1 (exact match for single plugin)
    let mut handles: [u64; 1] = [0_u64; 1];
    // SAFETY: rt is valid; handles is a valid buffer for 1 u64.
    let count: usize = unsafe {
        polyplug_runtime_find_all_by_contract(
            rt as *const OpaqueRuntime,
            contract_id,
            0_u32,
            handles.as_mut_ptr(),
            handles.len(),
        )
    };
    assert_eq!(count, 1, "find_all must return exactly 1 result");
    assert_ne!(handles[0], u64::MAX, "handle must not be null sentinel");

    // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// Test `find_all_by_contract` with overflow (more plugins than buffer).
/// Expected: Returns only what fits in buffer.
#[test]
fn test_find_all_by_contract_overflow() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        eprintln!("Skipping test_find_all_by_contract_overflow: TEST_PLUGIN_CPP_SO not set");
        return;
    }

    // SAFETY: polyplug_runtime_create returns a valid heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    // Load test_plugin (Rust) which provides "test.add@1"
    let rust_path_bytes: &[u8] = TEST_PLUGIN_DIR.as_bytes();
    // SAFETY: rt is valid; rust_path_bytes is valid UTF-8.
    let rc_rust: u32 = unsafe {
        polyplug_runtime_load_bundle(rt, rust_path_bytes.as_ptr(), rust_path_bytes.len())
    };
    assert_eq!(rc_rust, 0, "test_plugin load must succeed");

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
    // SAFETY: rt is valid; cpp_path_bytes is valid UTF-8.
    let rc_cpp: u32 =
        unsafe { polyplug_runtime_load_bundle(rt, cpp_path_bytes.as_ptr(), cpp_path_bytes.len()) };
    if rc_cpp != 0 {
        let mut err_buf: [u8; 512] = [0_u8; 512];
        // SAFETY: err_buf is a valid stack-allocated buffer; rt is valid.
        let err_len: usize = unsafe {
            polyplug_runtime_last_error(
                rt as *const OpaqueRuntime,
                err_buf.as_mut_ptr(),
                err_buf.len(),
            )
        };
        let err_msg: &str = core::str::from_utf8(&err_buf[..err_len]).unwrap_or("invalid UTF-8");
        panic!(
            "cpp_test_plugin load failed: {} (path: {})",
            err_msg, cpp_path_str
        );
    }

    // Buffer with capacity 1, but there are 2 plugins
    let mut handles: [u64; 1] = [0_u64; 1];
    // SAFETY: rt is valid; handles is a valid buffer for 1 u64.
    let count: usize = unsafe {
        polyplug_runtime_find_all_by_contract(
            rt as *const OpaqueRuntime,
            TEST_ADD_CONTRACT_ID,
            0_u32,
            handles.as_mut_ptr(),
            handles.len(),
        )
    };
    assert_eq!(count, 1, "find_all must return only what fits in buffer");
    assert_ne!(handles[0], u64::MAX, "handle must not be null sentinel");

    // Verify with larger buffer that there are actually 2 plugins
    let mut large_handles: [u64; 4] = [0_u64; 4];
    // SAFETY: rt is valid; large_handles is a valid buffer for 4 u64.
    let full_count: usize = unsafe {
        polyplug_runtime_find_all_by_contract(
            rt as *const OpaqueRuntime,
            TEST_ADD_CONTRACT_ID,
            0_u32,
            large_handles.as_mut_ptr(),
            large_handles.len(),
        )
    };
    assert_eq!(full_count, 2, "full buffer should find both plugins");

    // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}