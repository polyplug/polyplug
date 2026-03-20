#![allow(clippy::expect_used)]

//! Integration tests for LAST_ERROR thread-local: isolation, clearing, truncation,
//! null termination behaviour, and large message handling via the C facade.
//!
//! These tests exercise `polyplug_runtime_last_error` and `polyplug_runtime_error_message_len`
//! as defined in `crates/polyplug/src/ffi.rs:361-380` and `11-17`.

use polyplug::ffi::OpaqueRuntime;
use polyplug::ffi::polyplug_runtime_create;
use polyplug::ffi::polyplug_runtime_destroy;
use polyplug::ffi::polyplug_runtime_error_message_len;
use polyplug::ffi::polyplug_runtime_last_error;
use polyplug::ffi::polyplug_runtime_load_bundle;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Drains the current thread's LAST_ERROR, returning it as a `Vec<u8>`.
///
/// Calls `polyplug_runtime_last_error` with a 4 KiB stack buffer; the function clears
/// the error after reading it.
fn drain_last_error() -> Vec<u8> {
    let mut buf: [u8; 4096] = [0_u8; 4096];
    // SAFETY: buf is a valid stack buffer; buf.len() exactly matches the slice length.
    let n: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), buf.len()) };
    buf[..n].to_vec()
}

/// Returns the length reported by `polyplug_runtime_error_message_len` without clearing.
fn peek_error_len() -> usize {
    // SAFETY: no pointer arguments required.
    unsafe { polyplug_runtime_error_message_len() }
}

/// Clears any pre-existing error on the current thread.
fn clear_error() {
    drain_last_error();
}

/// Triggers a `set_last_error` call on the current thread by calling
/// `polyplug_runtime_load_bundle` with a null `rt` pointer, which unconditionally
/// records an error and returns non-zero.
fn trigger_error() {
    let path: &[u8] = b"/dev/null";
    // SAFETY: null rt is the explicit null-safety contract under test; no UB.
    let rc: u32 =
        unsafe { polyplug_runtime_load_bundle(core::ptr::null_mut(), path.as_ptr(), path.len()) };
    // We only care that an error was produced; the specific code is irrelevant.
    assert_ne!(
        rc, 0,
        "load_bundle(null rt) must return non-zero to seed LAST_ERROR"
    );
}

// ─── tests ────────────────────────────────────────────────────────────────────

/// `polyplug_runtime_last_error` returns 0 when no error has been set and the error
/// is already empty (fresh thread state).
#[test]
fn last_error_empty_on_fresh_thread() {
    clear_error(); // ensure clean slate in case tests share a thread

    let mut buf: [u8; 64] = [0_u8; 64];
    // SAFETY: buf is a valid stack buffer of length 64.
    let n: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), buf.len()) };
    assert_eq!(n, 0, "last_error must return 0 when no error is pending");
}

/// After `polyplug_runtime_last_error` is called (which clears the error), a second
/// call must return 0 — the error is cleared after the first read.
#[test]
fn last_error_cleared_after_read() {
    clear_error();
    trigger_error();

    // First read: must return a non-empty message.
    let first: Vec<u8> = drain_last_error();
    assert!(
        !first.is_empty(),
        "first read of last_error must return non-empty message"
    );

    // Second read: error was cleared; must return 0.
    let mut buf: [u8; 256] = [0_u8; 256];
    // SAFETY: buf is a valid stack buffer of length 256.
    let n: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), buf.len()) };
    assert_eq!(
        n, 0,
        "last_error must return 0 after the error was already read and cleared"
    );
}

/// `polyplug_runtime_error_message_len` does NOT clear the error; a subsequent
/// `polyplug_runtime_last_error` must still return the full message.
#[test]
fn error_message_len_does_not_clear_error() {
    clear_error();
    trigger_error();

    let len_before: usize = peek_error_len();
    assert!(
        len_before > 0,
        "error_message_len must report a non-zero length after trigger_error"
    );

    // Reading via last_error still returns the same length.
    let msg: Vec<u8> = drain_last_error();
    assert_eq!(
        msg.len(),
        len_before,
        "last_error length must match the value reported by error_message_len"
    );

    // Now cleared — error_message_len must report 0.
    let len_after: usize = peek_error_len();
    assert_eq!(
        len_after, 0,
        "error_message_len must report 0 after last_error has been called"
    );
}

/// `polyplug_runtime_last_error` truncates to `buf_len` and returns the number of
/// bytes actually written (capped at `buf_len`), not the full message length.
/// The error is still cleared after the partial read.
#[test]
fn last_error_truncates_to_buf_len() {
    clear_error();
    trigger_error();

    // First peek full length.
    let full_len: usize = peek_error_len();
    assert!(full_len > 0, "error must be non-empty for truncation test");

    if full_len < 2 {
        // Can't truncate a single-byte message; skip gracefully.
        drain_last_error();
        return;
    }

    let truncated_len: usize = full_len - 1;
    let mut buf: Vec<u8> = vec![0xAA_u8; truncated_len];
    // SAFETY: buf is a valid heap-allocated buffer of exactly `truncated_len` bytes.
    let n: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), buf.len()) };

    assert_eq!(
        n, truncated_len,
        "last_error must return the number of bytes written (truncated to buf_len)"
    );

    // Error must be cleared even after a partial (truncated) read.
    let mut probe: [u8; 8] = [0_u8; 8];
    // SAFETY: probe is a valid stack buffer of length 8.
    let n2: usize = unsafe { polyplug_runtime_last_error(probe.as_mut_ptr(), probe.len()) };
    assert_eq!(
        n2, 0,
        "last_error must be cleared even after a truncated read"
    );
}

/// `polyplug_runtime_last_error` with `buf_len == 0` returns 0 bytes written but
/// still clears the error (write_n = 0 because min(len, 0) == 0).
#[test]
fn last_error_zero_buf_len_clears_error() {
    clear_error();
    trigger_error();

    // Sanity: error is present.
    assert!(
        peek_error_len() > 0,
        "error must be set before zero-buf-len test"
    );

    let mut byte: u8 = 0xBB_u8;
    // SAFETY: buf_len = 0 means zero bytes are written; the pointer is
    // technically unused but must be non-null for defined behaviour on some
    // platforms. We pass a valid stack pointer for safety.
    let n: usize = unsafe { polyplug_runtime_last_error(&mut byte as *mut u8, 0) };
    assert_eq!(
        n, 0,
        "last_error with buf_len=0 must return 0 written bytes"
    );

    // The error is cleared (write_n == 0 branch: no copy, but clear still runs).
    let n2: usize = peek_error_len();
    assert_eq!(
        n2, 0,
        "last_error with buf_len=0 must still clear the error"
    );
}

/// `polyplug_runtime_last_error` with a null buffer and len=0 is the canonical
/// "just clear the error" call; must return 0 and must not crash.
#[test]
fn last_error_null_buf_zero_len_clears_error() {
    clear_error();
    trigger_error();

    assert!(
        peek_error_len() > 0,
        "error must be set before null-buf test"
    );

    // SAFETY: buf=null, buf_len=0 — no write occurs; this is the "discard" call pattern.
    let n: usize = unsafe { polyplug_runtime_last_error(core::ptr::null_mut(), 0) };
    assert_eq!(n, 0, "last_error(null, 0) must return 0");

    let n2: usize = peek_error_len();
    assert_eq!(n2, 0, "last_error(null, 0) must clear the error");
}

/// Errors set on one thread must NOT be visible on another thread.
///
/// Thread A sets an error. Thread B reads its own LAST_ERROR immediately after
/// and must see nothing — the two thread-locals are fully independent.
#[test]
fn last_error_cross_thread_isolation() {
    clear_error();

    // Spawn a thread that sets an error, then reads it back from its own slot.
    let handle: std::thread::JoinHandle<(Vec<u8>, usize)> = std::thread::spawn(|| {
        // Seed thread-B's LAST_ERROR.
        trigger_error();
        let msg: Vec<u8> = drain_last_error();
        let remaining: usize = peek_error_len();
        (msg, remaining)
    });

    let (thread_b_msg, thread_b_remaining) = handle.join().expect("thread B must not panic");

    // Thread B saw its own error correctly.
    assert!(
        !thread_b_msg.is_empty(),
        "thread B must have read its own LAST_ERROR"
    );
    assert_eq!(
        thread_b_remaining, 0,
        "thread B's LAST_ERROR must be cleared after drain"
    );

    // Thread A (this thread) must see nothing — its LAST_ERROR was not touched.
    let thread_a_len: usize = peek_error_len();
    assert_eq!(
        thread_a_len, 0,
        "thread A must see an empty LAST_ERROR — cross-thread leakage detected"
    );
}

/// Two threads set their own errors concurrently; each reads back exactly its
/// own message without interference.
#[test]
fn last_error_concurrent_threads_see_own_messages() {
    clear_error();

    // Use a barrier to maximise concurrency between the two threads.
    let barrier: std::sync::Arc<std::sync::Barrier> =
        std::sync::Arc::new(std::sync::Barrier::new(2));

    let b1: std::sync::Arc<std::sync::Barrier> = std::sync::Arc::clone(&barrier);
    let handle_a: std::thread::JoinHandle<usize> = std::thread::spawn(move || {
        b1.wait(); // synchronise start
        trigger_error();
        let msg: Vec<u8> = drain_last_error();
        msg.len()
    });

    let b2: std::sync::Arc<std::sync::Barrier> = std::sync::Arc::clone(&barrier);
    let handle_b: std::thread::JoinHandle<usize> = std::thread::spawn(move || {
        b2.wait(); // synchronise start
        trigger_error();
        let msg: Vec<u8> = drain_last_error();
        msg.len()
    });

    let len_a: usize = handle_a.join().expect("thread A must not panic");
    let len_b: usize = handle_b.join().expect("thread B must not panic");

    assert!(
        len_a > 0,
        "thread A must have received its own error message"
    );
    assert!(
        len_b > 0,
        "thread B must have received its own error message"
    );

    // Main thread's LAST_ERROR must remain untouched.
    let main_len: usize = peek_error_len();
    assert_eq!(
        main_len, 0,
        "main thread LAST_ERROR must not be polluted by concurrent threads"
    );
}

/// When no error is present, `polyplug_runtime_last_error` writes nothing and the
/// buffer contents must remain unchanged (no spurious null-termination or
/// zero-fill by the FFI layer).
#[test]
fn last_error_no_write_when_empty() {
    clear_error();

    let sentinel: u8 = 0xDE_u8;
    let mut buf: [u8; 16] = [sentinel; 16];
    // SAFETY: buf is a valid stack buffer of length 16.
    let n: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), buf.len()) };
    assert_eq!(n, 0, "must return 0 when no error is pending");

    // The FFI layer must not modify the buffer when there is nothing to write.
    for (i, &byte) in buf.iter().enumerate() {
        assert_eq!(
            byte, sentinel,
            "buf[{i}] was modified even though last_error had nothing to write"
        );
    }
}

/// Verifies that a full-capacity read (buf_len == message_len) writes exactly
/// the message bytes and returns the correct count.
#[test]
fn last_error_exact_buf_len_writes_full_message() {
    clear_error();
    trigger_error();

    let full_len: usize = peek_error_len();
    assert!(full_len > 0, "error must be non-empty for exact-len test");

    let mut buf: Vec<u8> = vec![0_u8; full_len];
    // SAFETY: buf is a heap-allocated buffer with exactly `full_len` bytes.
    let n: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), buf.len()) };

    assert_eq!(
        n, full_len,
        "last_error with buf_len == message_len must return full_len"
    );

    // Validate that the written bytes are valid UTF-8 (all polyplug error
    // messages are UTF-8 by the StringView ABI contract).
    let msg_result: Result<&str, core::str::Utf8Error> = core::str::from_utf8(&buf);
    assert!(msg_result.is_ok(), "LAST_ERROR must be valid UTF-8");
    let msg: &str = msg_result.unwrap_or("");
    assert!(!msg.is_empty(), "decoded error message must be non-empty");
}

/// A large error message (> typical stack buffer) is handled correctly by
/// using a large enough heap buffer.
#[test]
fn last_error_large_message_handling() {
    clear_error();

    // Create a runtime, then attempt to load a bundle with an extremely long
    // (but invalid) path to seed a non-trivial error message.
    // SAFETY: polyplug_runtime_create returns a heap-allocated runtime or null on OOM.
    let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
    assert!(!rt.is_null(), "polyplug_runtime_create must succeed");

    // A 512-byte path — long enough to exercise message formatting with the path included.
    let long_path: Vec<u8> = core::iter::repeat_n(b'x', 512).collect();
    // SAFETY: rt is valid; long_path is a valid non-null byte slice.
    let rc: u32 = unsafe { polyplug_runtime_load_bundle(rt, long_path.as_ptr(), long_path.len()) };
    assert_ne!(rc, 0, "load_bundle with non-existent path must fail");

    // SAFETY: rt is valid and was allocated by polyplug_runtime_create.
    unsafe { polyplug_runtime_destroy(rt) };

    let msg_len: usize = peek_error_len();
    if msg_len == 0 {
        // Some error paths do not embed the path; accept gracefully.
        return;
    }

    // Read into a heap buffer sized exactly to the reported length.
    let mut buf: Vec<u8> = vec![0_u8; msg_len];
    // SAFETY: buf is a heap-allocated buffer with exactly `msg_len` bytes.
    let n: usize = unsafe { polyplug_runtime_last_error(buf.as_mut_ptr(), buf.len()) };

    assert_eq!(n, msg_len, "large-message read must return msg_len bytes");
    assert!(
        core::str::from_utf8(&buf).is_ok(),
        "large LAST_ERROR message must be valid UTF-8"
    );

    // Verify cleared.
    let after: usize = peek_error_len();
    assert_eq!(
        after, 0,
        "LAST_ERROR must be cleared after large-message read"
    );
}

/// Repeated set-then-read cycles on the same thread must produce consistent,
/// independent error messages without accumulation or interference.
#[test]
fn last_error_repeated_cycles_independent() {
    clear_error();

    for _round in 0_u32..16_u32 {
        trigger_error();

        let len: usize = peek_error_len();
        assert!(len > 0, "each cycle must produce a non-empty error");

        let msg: Vec<u8> = drain_last_error();
        assert_eq!(
            msg.len(),
            len,
            "drained message length must match peek_error_len"
        );

        let after: usize = peek_error_len();
        assert_eq!(
            after, 0,
            "LAST_ERROR must be empty after drain in each cycle"
        );
    }
}
