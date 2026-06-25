#![allow(clippy::expect_used)]

//! Concurrent `host->log` delivery into one `LoggerHandle` funnel.
//!
//! A runtime installs a single logging closure; every guest (or host-bridge)
//! call to `HostApi::log` from any thread funnels through the one `LoggerHandle`
//! the runtime built from its config. `LoggerHandle` is `Send + Sync` and holds
//! no interior mutability, so concurrent `log` calls only read shared state — but
//! the sink behind it (here a closure) is reached from every thread at once.
//!
//! This test drives many threads calling the real `HostApi::log` function pointer
//! concurrently and asserts the funnel is thread-safe end to end: every record is
//! delivered (none lost under contention), each record arrives intact (its scope
//! and message agree on the originating thread — no torn or cross-wired pair), and
//! the run completes without deadlock or panic.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

use polyplug::runtime::Runtime;
use polyplug_abi::types::LogLevel;
use polyplug_abi::{HostApi, StringView};

use crate::common::TestNativeLoader;

const THREADS: usize = 8_usize;
const LOGS_PER_THREAD: usize = 2_000_usize;

/// Parse the originating thread index a record encodes, returning `None` if the
/// record is malformed. `scope` is `"scope.<t>"`; `message` is
/// `"payload.<t>.<seq>"`. A torn or cross-wired delivery would make the two
/// disagree (or fail to parse), which the caller flags.
fn thread_index_agrees(scope: &str, message: &str) -> bool {
    let scope_idx: Option<&str> = scope.strip_prefix("scope.");
    let message_idx: Option<&str> = message
        .strip_prefix("payload.")
        .and_then(|rest: &str| rest.split('.').next());
    match (scope_idx, message_idx) {
        (Some(s), Some(m)) => !s.is_empty() && s == m,
        _ => false,
    }
}

/// Many threads call `HostApi::log` concurrently through one runtime's logger.
/// All `THREADS * LOGS_PER_THREAD` records must arrive, each well-formed.
#[test]
fn concurrent_host_log_funnel_delivers_every_record_intact() {
    let delivered: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let malformed: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let cb_delivered: Arc<AtomicUsize> = Arc::clone(&delivered);
    let cb_malformed: Arc<AtomicBool> = Arc::clone(&malformed);

    // The closure is invoked from every logging thread at once: it must be
    // `Send + Sync`, and it validates each record's integrity in place.
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(TestNativeLoader::new())
        .logger(move |level: LogLevel, scope: &str, message: &str| {
            if level != LogLevel::Info || !thread_index_agrees(scope, message) {
                cb_malformed.store(true, Ordering::Relaxed);
            }
            cb_delivered.fetch_add(1_usize, Ordering::Relaxed);
        })
        .build()
        .expect("runtime build must succeed");

    let barrier: Arc<Barrier> = Arc::new(Barrier::new(THREADS));
    let handles: Vec<thread::JoinHandle<()>> = (0_usize..THREADS)
        .map(|t| {
            let rt_clone: Arc<Runtime> = Arc::clone(&rt);
            let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
            thread::spawn(move || {
                // Resolve the host table inside the thread (no raw pointer crosses
                // a thread boundary); it stays valid for the runtime's lifetime,
                // which the Arc clone holds open.
                let host: *const HostApi = rt_clone.host_abi();
                let scope: String = format!("scope.{t}");

                barrier_clone.wait();

                for seq in 0_usize..LOGS_PER_THREAD {
                    let message: String = format!("payload.{t}.{seq}");
                    let scope_sv: StringView = StringView {
                        ptr: scope.as_bytes().as_ptr(),
                        len: scope.len(),
                    };
                    let message_sv: StringView = StringView {
                        ptr: message.as_bytes().as_ptr(),
                        len: message.len(),
                    };
                    // SAFETY: `host` is this runtime's valid HostApi (the Arc clone
                    // keeps the runtime alive across the whole loop); `log` is a
                    // function-pointer field the runtime installed; `scope`/`message`
                    // outlive the synchronous call.
                    unsafe {
                        ((*host).log)(host, LogLevel::Info as u32, scope_sv, message_sv);
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("logging thread must not panic");
    }

    assert!(
        !malformed.load(Ordering::Relaxed),
        "a record arrived torn or cross-wired — the logger funnel is not thread-safe"
    );
    assert_eq!(
        delivered.load(Ordering::Relaxed),
        THREADS * LOGS_PER_THREAD,
        "every host->log record must reach the sink exactly once under contention"
    );
}
