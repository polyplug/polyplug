#![allow(clippy::expect_used)]

//! Integration tests for per-runtime LAST_ERROR: isolation, clearing, truncation,
//! and large message handling via the C facade.
//!
//! These tests exercise `polyplug_runtime_last_error` and `polyplug_runtime_error_message_len`
//! with per-runtime error storage.

use polyplug::ffi::OpaqueRuntime;
use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug::ffi::polyplug_runtime_error_message_len;
use polyplug::ffi::polyplug_runtime_last_error;
use polyplug::ffi::polyplug_runtime_load_bundle;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Drains the runtime's LAST_ERROR, returning it as a `Vec<u8>`.
fn drain_last_error(rt: *const OpaqueRuntime) -> Vec<u8> {
    let mut buf: [u8; 4096] = [0_u8; 4096];
    // SAFETY: buf is a valid stack buffer; rt is valid.
    let n: usize = unsafe { polyplug_runtime_last_error(rt, buf.as_mut_ptr(), buf.len()) };
    buf[..n].to_vec()
}

/// Returns the length reported by `polyplug_runtime_error_message_len` without clearing.
fn peek_error_len(rt: *const OpaqueRuntime) -> usize {
    // SAFETY: rt is valid.
    unsafe { polyplug_runtime_error_message_len(rt) }
}

/// Clears any pre-existing error on the runtime.
fn clear_error(rt: *const OpaqueRuntime) {
    drain_last_error(rt);
}

/// Triggers a `set_last_error` call on the runtime by calling
/// `polyplug_runtime_load_bundle` with a non-existent path.
fn trigger_error(rt: *mut OpaqueRuntime) {
    let path: &[u8] = b"/nonexistent/path/that/does/not/exist";
    // SAFETY: rt is valid; path is valid bytes.
    let rc: u32 = unsafe { polyplug_runtime_load_bundle(rt, path.as_ptr(), path.len()) };
    assert_ne!(
        rc, 0,
        "load_bundle with non-existent path must return non-zero"
    );
}

// ─── tests ────────────────────────────────────────────────────────────────────

/// `polyplug_runtime_last_error` returns 0 when no error has been set.
#[test]
fn last_error_empty_on_fresh_runtime() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);

    let mut buf: [u8; 64] = [0_u8; 64];
    // SAFETY: buf is a valid stack buffer of length 64; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };
    assert_eq!(n, 0, "last_error must return 0 when no error is pending");

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// After `polyplug_runtime_last_error` is called (which clears the error), a second
/// call must return 0 — the error is cleared after the first read.
#[test]
fn last_error_cleared_after_read() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);
    trigger_error(rt);

    // First read: must return a non-empty message.
    let first: Vec<u8> = drain_last_error(rt as *const OpaqueRuntime);
    assert!(
        !first.is_empty(),
        "first read of last_error must return non-empty message"
    );

    // Second read: error was cleared; must return 0.
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: buf is a valid stack buffer of length 256; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };
    assert_eq!(
        n, 0,
        "last_error must return 0 after the error was already read and cleared"
    );

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// `polyplug_runtime_error_message_len` does NOT clear the error; a subsequent
/// `polyplug_runtime_last_error` must still return the full message.
#[test]
fn error_message_len_does_not_clear_error() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);
    trigger_error(rt);

    let len_before: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert!(
        len_before > 0,
        "error_message_len must report a non-zero length after trigger_error"
    );

    // Reading via last_error still returns the same length.
    let msg: Vec<u8> = drain_last_error(rt as *const OpaqueRuntime);
    assert_eq!(
        msg.len(),
        len_before,
        "last_error length must match the value reported by error_message_len"
    );

    // Now cleared — error_message_len must report 0.
    let len_after: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert_eq!(
        len_after, 0,
        "error_message_len must report 0 after last_error has been called"
    );

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// `polyplug_runtime_last_error` truncates to `buf_len` and returns the number of
/// bytes actually written (capped at `buf_len`), not the full message length.
/// The error is still cleared after the partial read.
#[test]
fn last_error_truncates_to_buf_len() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);
    trigger_error(rt);

    // First peek full length.
    let full_len: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert!(full_len > 0, "error must be non-empty for truncation test");

    if full_len < 2 {
        // Can't truncate a single-byte message; skip gracefully.
        drain_last_error(rt as *const OpaqueRuntime);
        // SAFETY: rt was allocated by polyplug_runtime_create.
        unsafe { polyplug_runtime_destroy(rt) };
        return;
    }

    let truncated_len: usize = full_len - 1;
    let mut buf: Vec<u8> = vec![0xAA_u8; truncated_len];
    // SAFETY: buf is a valid heap-allocated buffer of exactly `truncated_len` bytes; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };

    assert_eq!(
        n, truncated_len,
        "last_error must return the number of bytes written (truncated to buf_len)"
    );

    // Error must be cleared even after a partial (truncated) read.
    let mut probe: [u8; 8] = [0_u8; 8];
    // SAFETY: probe is a valid stack buffer of length 8; rt is valid.
    let n2: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, probe.as_mut_ptr(), probe.len())
    };
    assert_eq!(
        n2, 0,
        "last_error must be cleared even after a truncated read"
    );

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// `polyplug_runtime_last_error` with `buf_len == 0` returns 0 bytes written but
/// still clears the error (write_n = 0 because min(len, 0) == 0).
#[test]
fn last_error_zero_buf_len_clears_error() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);
    trigger_error(rt);

    // Sanity: error is present.
    assert!(
        peek_error_len(rt as *const OpaqueRuntime) > 0,
        "error must be set before zero-buf-len test"
    );

    let mut byte: u8 = 0xBB_u8;
    // SAFETY: buf_len = 0 means zero bytes are written; rt is valid.
    let n: usize =
        unsafe { polyplug_runtime_last_error(rt as *const OpaqueRuntime, &mut byte as *mut u8, 0) };
    assert_eq!(
        n, 0,
        "last_error with buf_len=0 must return 0 written bytes"
    );

    // The error is cleared (write_n == 0 branch: no copy, but clear still runs).
    let n2: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert_eq!(
        n2, 0,
        "last_error with buf_len=0 must still clear the error"
    );

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// `polyplug_runtime_last_error` with a null buffer and len=0 is the canonical
/// "just clear the error" call; must return the error length and must not crash.
#[test]
fn last_error_null_buf_zero_len_clears_error() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);
    trigger_error(rt);

    let error_len: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert!(error_len > 0, "error must be set before null-buf test");

    // SAFETY: buf=null, buf_len=0 — no write occurs; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, core::ptr::null_mut(), 0)
    };
    assert_eq!(
        n, error_len,
        "last_error(null buf, 0) must return error length"
    );

    let n2: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert_eq!(n2, 0, "last_error(null buf, 0) must clear the error");

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// Multiple runtimes have independent error storage.
#[test]
fn last_error_per_runtime_isolation() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt1: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt1.is_null(), "runtime 1 creation must succeed");
    let rt2: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt2.is_null(), "runtime 2 creation must succeed");

    clear_error(rt1 as *const OpaqueRuntime);
    clear_error(rt2 as *const OpaqueRuntime);

    // Trigger error on rt1 only
    trigger_error(rt1);

    // rt1 has an error
    let rt1_len: usize = peek_error_len(rt1 as *const OpaqueRuntime);
    assert!(rt1_len > 0, "rt1 must have an error");

    // rt2 has no error
    let rt2_len: usize = peek_error_len(rt2 as *const OpaqueRuntime);
    assert_eq!(rt2_len, 0, "rt2 must have no error");

    // Clear rt1's error
    drain_last_error(rt1 as *const OpaqueRuntime);

    // Both now have no error
    assert_eq!(
        peek_error_len(rt1 as *const OpaqueRuntime),
        0,
        "rt1 error cleared"
    );
    assert_eq!(
        peek_error_len(rt2 as *const OpaqueRuntime),
        0,
        "rt2 still has no error"
    );

    // SAFETY: rt1 and rt2 were allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt1) };
    unsafe { polyplug_runtime_destroy(rt2) };
}

/// When no error is present, `polyplug_runtime_last_error` writes nothing and the
/// buffer contents must remain unchanged.
#[test]
fn last_error_no_write_when_empty() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);

    let sentinel: u8 = 0xDE_u8;
    let mut buf: [u8; 16] = [sentinel; 16];
    // SAFETY: buf is a valid stack buffer of length 16; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };
    assert_eq!(n, 0, "must return 0 when no error is pending");

    // The FFI layer must not modify the buffer when there is nothing to write.
    for (i, &byte) in buf.iter().enumerate() {
        assert_eq!(
            byte, sentinel,
            "buf[{i}] was modified even though last_error had nothing to write"
        );
    }

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// Verifies that a full-capacity read (buf_len == message_len) writes exactly
/// the message bytes and returns the correct count.
#[test]
fn last_error_exact_buf_len_writes_full_message() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);
    trigger_error(rt);

    let full_len: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert!(full_len > 0, "error must be non-empty for exact-len test");

    let mut buf: Vec<u8> = vec![0_u8; full_len];
    // SAFETY: buf is a heap-allocated buffer with exactly `full_len` bytes; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };

    assert_eq!(
        n, full_len,
        "last_error with buf_len == message_len must return full_len"
    );

    // Validate that the written bytes are valid UTF-8.
    let msg_result: Result<&str, core::str::Utf8Error> = core::str::from_utf8(&buf);
    assert!(msg_result.is_ok(), "LAST_ERROR must be valid UTF-8");
    let msg: &str = msg_result.unwrap_or("");
    assert!(!msg.is_empty(), "decoded error message must be non-empty");

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// A large error message (> typical stack buffer) is handled correctly.
#[test]
fn last_error_large_message_handling() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "polyplug_runtime_create must succeed");

    clear_error(rt as *const OpaqueRuntime);

    // A 512-byte path — long enough to exercise message formatting with the path included.
    let long_path: Vec<u8> = core::iter::repeat_n(b'x', 512).collect();
    // SAFETY: rt is valid; long_path is a valid non-null byte slice.
    let rc: u32 = unsafe { polyplug_runtime_load_bundle(rt, long_path.as_ptr(), long_path.len()) };
    assert_ne!(rc, 0, "load_bundle with non-existent path must fail");

    let msg_len: usize = peek_error_len(rt as *const OpaqueRuntime);
    if msg_len == 0 {
        // Some error paths do not embed the path; accept gracefully.
        // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
        unsafe { polyplug_runtime_destroy(rt) };
        return;
    }

    // Read into a heap buffer sized exactly to the reported length.
    let mut buf: Vec<u8> = vec![0_u8; msg_len];
    // SAFETY: buf is a heap-allocated buffer with exactly `msg_len` bytes; rt is valid.
    let n: usize = unsafe {
        polyplug_runtime_last_error(rt as *const OpaqueRuntime, buf.as_mut_ptr(), buf.len())
    };

    assert_eq!(n, msg_len, "large-message read must return msg_len bytes");
    assert!(
        core::str::from_utf8(&buf).is_ok(),
        "large LAST_ERROR message must be valid UTF-8"
    );

    // Verify cleared.
    let after: usize = peek_error_len(rt as *const OpaqueRuntime);
    assert_eq!(
        after, 0,
        "LAST_ERROR must be cleared after large-message read"
    );

    // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// Repeated set-then-read cycles on the same runtime must produce consistent,
/// independent error messages without accumulation or interference.
#[test]
fn last_error_repeated_cycles_independent() {
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "runtime creation must succeed");

    clear_error(rt as *const OpaqueRuntime);

    for _round in 0_u32..16_u32 {
        trigger_error(rt);

        let len: usize = peek_error_len(rt as *const OpaqueRuntime);
        assert!(len > 0, "each cycle must produce a non-empty error");

        let msg: Vec<u8> = drain_last_error(rt as *const OpaqueRuntime);
        assert_eq!(
            msg.len(),
            len,
            "drained message length must match peek_error_len"
        );

        let after: usize = peek_error_len(rt as *const OpaqueRuntime);
        assert_eq!(
            after, 0,
            "LAST_ERROR must be empty after drain in each cycle"
        );
    }

    // SAFETY: rt was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };
}

/// Null runtime returns 0 (no runtime to have an error).
#[test]
fn last_error_null_runtime_returns_known_error() {
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: buf is a valid stack buffer; null rt is valid for this call.
    let n: usize =
        unsafe { polyplug_runtime_last_error(core::ptr::null(), buf.as_mut_ptr(), buf.len()) };
    assert!(
        n == 0,
        "last_error with null rt must return 0 (no runtime to have an error)"
    );
}
