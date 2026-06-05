#![allow(clippy::expect_used)]

//! Integration tests for per-runtime LAST_ERROR: isolation, clearing, truncation,
//! and large message handling via the HostApi API.
//!
//! These tests exercise `HostApi.get_last_error` and `HostApi.get_error_len`
//! with per-runtime error storage.

use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug::runtime::host_get_last_error;
use polyplug_abi::HostApi;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Drains the runtime's LAST_ERROR, returning it as a `Vec<u8>`.
fn drain_last_error(host: *const HostApi) -> Vec<u8> {
    let mut buf: [u8; 4096] = [0_u8; 4096];
    // SAFETY: buf is a valid stack buffer; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };
    buf[..n].to_vec()
}

/// Returns the length reported by `get_error_len` without clearing.
fn peek_error_len(host: *const HostApi) -> usize {
    // SAFETY: host is valid.
    unsafe { ((*host).get_error_len)(host) }
}

/// Clears any pre-existing error on the runtime.
fn clear_error(host: *const HostApi) {
    drain_last_error(host);
}

/// Triggers a `set_last_error` call on the runtime by calling
/// `load_bundle` with a non-existent path.
fn trigger_error(host: *const HostApi) {
    let path: &[u8] = b"/nonexistent/path/that/does/not/exist";
    // SAFETY: host is valid; path is valid bytes.
    let result: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, path.as_ptr(), path.len()) };
    assert_ne!(
        result.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "load_bundle with non-existent path must return error"
    );
}

// ─── tests ────────────────────────────────────────────────────────────────────

/// `get_last_error` returns 0 when no error has been set.
#[test]
fn last_error_empty_on_fresh_runtime() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);

    let mut buf: [u8; 64] = [0_u8; 64];
    // SAFETY: buf is a valid stack buffer of length 64; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };
    assert_eq!(
        n, 0,
        "get_last_error must return 0 when no error is pending"
    );

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// After `get_last_error` is called (which clears the error), a second
/// call must return 0 — the error is cleared after the first read.
#[test]
fn last_error_cleared_after_read() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);
    trigger_error(host);

    // First read: must return a non-empty message.
    let first: Vec<u8> = drain_last_error(host);
    assert!(
        !first.is_empty(),
        "first read of get_last_error must return non-empty message"
    );

    // Second read: error was cleared; must return 0.
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: buf is a valid stack buffer of length 256; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };
    assert_eq!(
        n, 0,
        "get_last_error must return 0 after the error was already read and cleared"
    );

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// `get_error_len` does NOT clear the error; a subsequent
/// `get_last_error` must still return the full message.
#[test]
fn error_len_does_not_clear_error() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);
    trigger_error(host);

    let len_before: usize = peek_error_len(host);
    assert!(
        len_before > 0,
        "get_error_len must report a non-zero length after trigger_error"
    );

    // Reading via get_last_error still returns the same length.
    let msg: Vec<u8> = drain_last_error(host);
    assert_eq!(
        msg.len(),
        len_before,
        "get_last_error length must match the value reported by get_error_len"
    );

    // Now cleared — get_error_len must report 0.
    let len_after: usize = peek_error_len(host);
    assert_eq!(
        len_after, 0,
        "get_error_len must report 0 after get_last_error has been called"
    );

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// `get_last_error` truncates to `buf_len` and returns the number of
/// bytes actually written (capped at `buf_len`), not the full message length.
/// The error is still cleared after the partial read.
#[test]
fn last_error_truncates_to_buf_len() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);
    trigger_error(host);

    // First peek full length.
    let full_len: usize = peek_error_len(host);
    assert!(full_len > 0, "error must be non-empty for truncation test");

    if full_len < 2 {
        // Can't truncate a single-byte message; skip gracefully.
        drain_last_error(host);
        // SAFETY: host was allocated by polyplug_runtime_create.
        unsafe { polyplug_runtime_destroy(host) };
        return;
    }

    let truncated_len: usize = full_len - 1;
    let mut buf: Vec<u8> = vec![0xAA_u8; truncated_len];
    // SAFETY: buf is a valid heap-allocated buffer of exactly `truncated_len` bytes; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };

    assert_eq!(
        n, truncated_len,
        "get_last_error must return the number of bytes written (truncated to buf_len)"
    );

    // Error must be cleared even after a partial (truncated) read.
    let mut probe: [u8; 8] = [0_u8; 8];
    // SAFETY: probe is a valid stack buffer of length 8; host is valid.
    let n2: usize = unsafe { ((*host).get_last_error)(host, probe.as_mut_ptr(), probe.len()) };
    assert_eq!(
        n2, 0,
        "get_last_error must be cleared even after a truncated read"
    );

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// `get_last_error` with `buf_len == 0` returns 0 bytes written but
/// still clears the error (write_n = 0 because min(len, 0) == 0).
#[test]
fn last_error_zero_buf_len_clears_error() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);
    trigger_error(host);

    // Sanity: error is present.
    assert!(
        peek_error_len(host) > 0,
        "error must be set before zero-buf-len test"
    );

    let mut byte: u8 = 0xBB_u8;
    // SAFETY: buf_len = 0 means zero bytes are written; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, &mut byte as *mut u8, 0) };
    assert_eq!(
        n, 0,
        "get_last_error with buf_len=0 must return 0 written bytes"
    );

    // The error is cleared (write_n == 0 branch: no copy, but clear still runs).
    let n2: usize = peek_error_len(host);
    assert_eq!(
        n2, 0,
        "get_last_error with buf_len=0 must still clear the error"
    );

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// `get_last_error` with a null buffer and len=0 is the canonical
/// "just clear the error" call; must return the error length and must not crash.
#[test]
fn last_error_null_buf_zero_len_clears_error() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);
    trigger_error(host);

    let error_len: usize = peek_error_len(host);
    assert!(error_len > 0, "error must be set before null-buf test");

    // SAFETY: buf=null, buf_len=0 — no write occurs; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, core::ptr::null_mut(), 0) };
    assert_eq!(
        n, error_len,
        "get_last_error(null buf, 0) must return error length"
    );

    let n2: usize = peek_error_len(host);
    assert_eq!(n2, 0, "get_last_error(null buf, 0) must clear the error");

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// Multiple runtimes have independent error storage.
#[test]
fn last_error_per_runtime_isolation() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host1: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host1.is_null(), "runtime 1 creation must succeed");
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host2: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host2.is_null(), "runtime 2 creation must succeed");

    clear_error(host1);
    clear_error(host2);

    // Trigger error on host1 only
    trigger_error(host1);

    // host1 has an error
    let host1_len: usize = peek_error_len(host1);
    assert!(host1_len > 0, "host1 must have an error");

    // host2 has no error
    let host2_len: usize = peek_error_len(host2);
    assert_eq!(host2_len, 0, "host2 must have no error");

    // Clear host1's error
    drain_last_error(host1);

    // Both now have no error
    assert_eq!(peek_error_len(host1), 0, "host1 error cleared");
    assert_eq!(peek_error_len(host2), 0, "host2 still has no error");

    // SAFETY: host1 was allocated by polyplug_runtime_create and is destroyed once.
    unsafe { polyplug_runtime_destroy(host1) };
    // SAFETY: host2 was allocated by polyplug_runtime_create and is destroyed once.
    unsafe { polyplug_runtime_destroy(host2) };
}

/// When no error is present, `get_last_error` writes nothing and the
/// buffer contents must remain unchanged.
#[test]
fn last_error_no_write_when_empty() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);

    let sentinel: u8 = 0xDE_u8;
    let mut buf: [u8; 16] = [sentinel; 16];
    // SAFETY: buf is a valid stack buffer of length 16; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };
    assert_eq!(n, 0, "must return 0 when no error is pending");

    // The FFI layer must not modify the buffer when there is nothing to write.
    for (i, &byte) in buf.iter().enumerate() {
        assert_eq!(
            byte, sentinel,
            "buf[{i}] was modified even though get_last_error had nothing to write"
        );
    }

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// Verifies that a full-capacity read (buf_len == message_len) writes exactly
/// the message bytes and returns the correct count.
#[test]
fn last_error_exact_buf_len_writes_full_message() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);
    trigger_error(host);

    let full_len: usize = peek_error_len(host);
    assert!(full_len > 0, "error must be non-empty for exact-len test");

    let mut buf: Vec<u8> = vec![0_u8; full_len];
    // SAFETY: buf is a heap-allocated buffer with exactly `full_len` bytes; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };

    assert_eq!(
        n, full_len,
        "get_last_error with buf_len == message_len must return full_len"
    );

    // Validate that the written bytes are valid UTF-8.
    let msg_result: Result<&str, core::str::Utf8Error> = core::str::from_utf8(&buf);
    assert!(msg_result.is_ok(), "LAST_ERROR must be valid UTF-8");
    let msg: &str = msg_result.unwrap_or("");
    assert!(!msg.is_empty(), "decoded error message must be non-empty");

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// A large error message (> typical stack buffer) is handled correctly.
#[test]
fn last_error_large_message_handling() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "polyplug_runtime_create must succeed");

    clear_error(host);

    // A 512-byte path — long enough to exercise message formatting with the path included.
    let long_path: Vec<u8> = core::iter::repeat_n(b'x', 512).collect();
    // SAFETY: host is valid; long_path is a valid non-null byte slice.
    let result: polyplug_abi::AbiError =
        unsafe { ((*host).load_bundle)(host, long_path.as_ptr(), long_path.len()) };
    assert_ne!(
        result.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "load_bundle with non-existent path must fail"
    );

    let msg_len: usize = peek_error_len(host);
    if msg_len == 0 {
        // Some error paths do not embed the path; accept gracefully.
        // SAFETY: host is valid and was allocated by polyplug_runtime_create.
        unsafe { polyplug_runtime_destroy(host) };
        return;
    }

    // Read into a heap buffer sized exactly to the reported length.
    let mut buf: Vec<u8> = vec![0_u8; msg_len];
    // SAFETY: buf is a heap-allocated buffer with exactly `msg_len` bytes; host is valid.
    let n: usize = unsafe { ((*host).get_last_error)(host, buf.as_mut_ptr(), buf.len()) };

    assert_eq!(n, msg_len, "large-message read must return msg_len bytes");
    assert!(
        core::str::from_utf8(&buf).is_ok(),
        "large LAST_ERROR message must be valid UTF-8"
    );

    // Verify cleared.
    let after: usize = peek_error_len(host);
    assert_eq!(
        after, 0,
        "LAST_ERROR must be cleared after large-message read"
    );

    // SAFETY: host is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// Repeated set-then-read cycles on the same runtime must produce consistent,
/// independent error messages without accumulation or interference.
#[test]
fn last_error_repeated_cycles_independent() {
    // SAFETY: polyplug_runtime_create returns a HostApi or null on OOM.
    let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
    assert!(!host.is_null(), "runtime creation must succeed");

    clear_error(host);

    for _round in 0_u32..16_u32 {
        trigger_error(host);

        let len: usize = peek_error_len(host);
        assert!(len > 0, "each cycle must produce a non-empty error");

        let msg: Vec<u8> = drain_last_error(host);
        assert_eq!(
            msg.len(),
            len,
            "drained message length must match peek_error_len"
        );

        let after: usize = peek_error_len(host);
        assert_eq!(
            after, 0,
            "LAST_ERROR must be empty after drain in each cycle"
        );
    }

    // SAFETY: host was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(host) };
}

/// Null host returns 0 (no host to have an error).
#[test]
fn last_error_null_host_returns_zero() {
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: buf is a valid stack buffer; null host is valid for this call.
    // Call the underlying host_get_last_error function directly.
    let n: usize = unsafe { host_get_last_error(core::ptr::null(), buf.as_mut_ptr(), buf.len()) };
    assert!(
        n == 0,
        "get_last_error with null host must return 0 (no host to have an error)"
    );
}
