//! LoggerHandle — instance-owned copy of the host logging configuration.
//!
//! The runtime copies the `log` / `log_user_data` / `log_max_level` fields out
//! of [`RuntimeConfig`] into a small `Copy` handle that is plumbed into every
//! component that needs to emit diagnostics ([`crate::runtime::Runtime`],
//! [`crate::runtime_store::RuntimeStore`]). No statics, no thread-locals —
//! every handle is owned by its runtime instance (Rule 12).
//!
//! # Lock rule
//!
//! The host callback must NEVER be invoked while a runtime lock guard is held
//! (a callback that touches the runtime would deadlock). Poison-recovery
//! logging therefore goes through [`RecoveringGuard`], which defers the log
//! emission until after the recovered guard has been dropped.

use core::ffi::c_void;
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use core::ptr;
use std::io::{Write, stderr};
use std::sync::PoisonError;

use polyplug_abi::runtime::RuntimeConfig;
use polyplug_abi::types::{LogLevel, StringView};

/// Instance-owned copy of the host logging configuration.
///
/// Cheap to copy into components that have no back-reference to the
/// [`crate::runtime::Runtime`] (e.g. [`crate::runtime_store::RuntimeStore`]).
#[derive(Clone, Copy)]
pub struct LoggerHandle {
    /// Host-supplied logger callback, or `None` for the stderr default.
    callback: Option<unsafe extern "C" fn(*mut c_void, u32, StringView, StringView)>,
    /// Opaque host pointer forwarded as the callback's first argument.
    user_data: *mut c_void,
    /// Maximum delivered level (as `u32`); only meaningful when `callback` is set.
    max_level: u32,
}

// SAFETY: `user_data` is an opaque host pointer that the runtime never
// dereferences — it is only forwarded to the host callback. The RuntimeConfig
// contract (documented on `RuntimeConfig::log` / `log_user_data`) requires the
// host to keep the callback thread-safe and the pointee valid for the
// runtime's entire lifetime, from any thread. This mirrors how HostApi
// pointers are shared across threads under the same host contract.
unsafe impl Send for LoggerHandle {}
// SAFETY: see the Send impl above — no interior mutability and the host-supplied
// callback is required to be safe from any thread, so shared reads are sound.
unsafe impl Sync for LoggerHandle {}

impl LoggerHandle {
    /// Handle with no host callback: Error/Warn go to stderr, all else dropped.
    pub const fn default_stderr() -> LoggerHandle {
        LoggerHandle {
            callback: None,
            user_data: ptr::null_mut(),
            max_level: LogLevel::Warn as u32,
        }
    }

    /// Copy the logging fields out of a [`RuntimeConfig`].
    pub(crate) fn from_config(config: &RuntimeConfig) -> LoggerHandle {
        LoggerHandle {
            callback: config.log,
            user_data: config.log_user_data,
            max_level: config.log_max_level,
        }
    }

    /// Whether a message at `level` would be delivered.
    ///
    /// With a host callback installed this honours `log_max_level`. Without
    /// one, the stderr default is always capped at [`LogLevel::Warn`]
    /// regardless of `log_max_level` (ratified null-callback semantics — this
    /// also keeps zero-initialised configs from foreign hosts on the
    /// Error/Warn-to-stderr default).
    pub(crate) fn enabled(&self, level: LogLevel) -> bool {
        let effective_max: u32 = match self.callback {
            Some(_) => self.max_level,
            None => LogLevel::Warn as u32,
        };
        (level as u32) <= effective_max
    }

    /// Emit a log message.
    ///
    /// `message` is a lazy producer: it is only invoked when `level` passes
    /// the [`LoggerHandle::enabled`] filter, so disabled levels pay zero
    /// formatting cost.
    ///
    /// Must NEVER be called while any runtime lock guard is held — the host
    /// callback may not re-enter the runtime, but a callback blocking on host
    /// state of its own while the runtime holds a lock is still a deadlock
    /// hazard, and the documented callback contract assumes lock-free entry.
    /// Poison-recovery sites use [`RecoveringGuard`] to satisfy this.
    pub fn log<F>(&self, level: LogLevel, scope: &str, message: F)
    where
        F: FnOnce() -> String,
    {
        if !self.enabled(level) {
            return;
        }
        let msg: String = message();
        match self.callback {
            Some(callback) => {
                let scope_view = StringView {
                    ptr: scope.as_ptr(),
                    len: scope.len(),
                };
                let message_view = StringView {
                    ptr: msg.as_ptr(),
                    len: msg.len(),
                };
                // SAFETY: `callback` and `user_data` come from RuntimeConfig;
                // the host guarantees (documented field contract) that the
                // callback is thread-safe and that `user_data` stays valid for
                // the runtime's lifetime. Both StringViews point into stack-
                // owned UTF-8 data (`scope`, `msg`) that outlives the call,
                // matching the documented "valid for the duration of the call"
                // contract.
                unsafe { callback(self.user_data, level as u32, scope_view, message_view) }
            }
            None => {
                // Default sink: stderr, Error/Warn only (filtered above).
                let _ = writeln!(stderr(), "[polyplug] [{scope}] {msg}");
            }
        }
    }
}

/// Internal sink object for the Rust closure installed via
/// `RuntimeBuilder::logger`.
///
/// Every `Fn(LogLevel, &str, &str) + Send + Sync` closure implements this via
/// the blanket impl below; the trait exists so the boxed host closure has a
/// simple, nameable object type.
pub(crate) trait LogSink: Send + Sync {
    /// Deliver one log record to the host closure.
    fn emit(&self, level: LogLevel, scope: &str, message: &str);
}

impl<F> LogSink for F
where
    F: Fn(LogLevel, &str, &str) + Send + Sync,
{
    fn emit(&self, level: LogLevel, scope: &str, message: &str) {
        self(level, scope, message)
    }
}

/// Boxed Rust logger closure installed via `RuntimeBuilder::logger`.
///
/// Newtype (not a type alias — those are forbidden) naming the closure the
/// runtime keeps alive behind `RuntimeConfig::log_user_data`: the `Runtime`
/// owns a `Box<LoggerClosure>`, whose stable heap address is the thin pointer
/// the `extern "C"` trampoline dereferences.
pub(crate) struct LoggerClosure(pub(crate) Box<dyn LogSink>);

/// Lock guard wrapper that recovers from lock poisoning and defers the
/// recovery log until AFTER the inner guard has been dropped.
///
/// Rationale: poison recovery happens at lock acquisition, while the recovered
/// guard is live. Logging at that point would invoke the host callback under a
/// runtime lock, which the lock rule forbids. This wrapper records the
/// recovery and emits the Warn-level log from `Drop`, strictly after the inner
/// guard has been released.
pub struct RecoveringGuard<G> {
    /// The recovered (or clean) lock guard. `ManuallyDrop` so `Drop` can
    /// release the lock BEFORE emitting the deferred recovery log.
    guard: ManuallyDrop<G>,
    /// `Some((logger, scope))` when poison was recovered at acquisition.
    recovered: Option<(LoggerHandle, &'static str)>,
}

impl<G> RecoveringGuard<G> {
    /// Wrap a lock acquisition result, recovering from poisoning.
    ///
    /// On poison, the inner guard is extracted via `PoisonError::into_inner`
    /// and a Warn-level recovery log is scheduled for emission after the guard
    /// drops.
    pub fn new(
        result: Result<G, PoisonError<G>>,
        logger: LoggerHandle,
        scope: &'static str,
    ) -> RecoveringGuard<G> {
        match result {
            Ok(guard) => RecoveringGuard {
                guard: ManuallyDrop::new(guard),
                recovered: None,
            },
            Err(poison) => RecoveringGuard {
                guard: ManuallyDrop::new(poison.into_inner()),
                recovered: Some((logger, scope)),
            },
        }
    }
}

impl<G> Deref for RecoveringGuard<G> {
    type Target = G;

    fn deref(&self) -> &G {
        &self.guard
    }
}

impl<G> DerefMut for RecoveringGuard<G> {
    fn deref_mut(&mut self) -> &mut G {
        &mut self.guard
    }
}

impl<G> Drop for RecoveringGuard<G> {
    fn drop(&mut self) {
        // SAFETY: `self.guard` is dropped exactly once, here, and is never
        // accessed afterwards (this is the only place that drops it, and the
        // wrapper is being destroyed). Dropping it FIRST releases the lock so
        // the deferred recovery log below runs without any guard held.
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
        }
        if let Some((logger, scope)) = self.recovered.take() {
            logger.log(LogLevel::Warn, scope, || {
                String::from("lock poisoned, recovering")
            });
        }
    }
}

/// Recover a poisoned lock result into a [`RecoveringGuard`].
///
/// Suffix-form convenience for the ubiquitous
/// `lock().recover_poisoned(logger, scope)` acquisition pattern.
pub trait RecoverPoisoned<G> {
    /// See [`RecoveringGuard::new`].
    fn recover_poisoned(self, logger: LoggerHandle, scope: &'static str) -> RecoveringGuard<G>;
}

impl<G> RecoverPoisoned<G> for Result<G, PoisonError<G>> {
    fn recover_poisoned(self, logger: LoggerHandle, scope: &'static str) -> RecoveringGuard<G> {
        RecoveringGuard::new(self, logger, scope)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use core::ffi::c_void;
    use core::ptr;
    use core::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard, TryLockError};
    use std::thread;

    use polyplug_abi::types::{LogLevel, StringView};

    use super::{LoggerHandle, RecoverPoisoned, RecoveringGuard};

    /// Test callback that copies every delivered record into the
    /// `Mutex<Vec<...>>` sink behind `user_data` (no statics).
    unsafe extern "C" fn capture_log(
        user_data: *mut c_void,
        level: u32,
        scope: StringView,
        message: StringView,
    ) {
        // SAFETY: every test passes a pointer to a Mutex<Vec<...>> that
        // outlives all log calls made through the handle.
        let sink: &Mutex<Vec<(u32, String, String)>> =
            unsafe { &*(user_data as *const Mutex<Vec<(u32, String, String)>>) };
        // SAFETY: the runtime contract guarantees both views are valid UTF-8
        // for the duration of this call; we copy them immediately.
        let (scope_owned, message_owned): (String, String) =
            unsafe { (scope.as_str().to_owned(), message.as_str().to_owned()) };
        sink.lock()
            .expect("test sink lock")
            .push((level, scope_owned, message_owned));
    }

    fn capture_handle(
        sink: &Mutex<Vec<(u32, String, String)>>,
        max_level: LogLevel,
    ) -> LoggerHandle {
        LoggerHandle {
            callback: Some(capture_log),
            user_data: sink as *const Mutex<Vec<(u32, String, String)>> as *mut c_void,
            max_level: max_level as u32,
        }
    }

    #[test]
    fn callback_receives_level_scope_message() {
        let sink: Box<Mutex<Vec<(u32, String, String)>>> = Box::new(Mutex::new(Vec::new()));
        let logger: LoggerHandle = capture_handle(&sink, LogLevel::Trace);

        logger.log(LogLevel::Info, "store", || String::from("hello"));

        let records: Vec<(u32, String, String)> = sink.lock().expect("sink").clone();
        assert_eq!(
            records,
            vec![(
                LogLevel::Info as u32,
                String::from("store"),
                String::from("hello")
            )]
        );
    }

    #[test]
    fn max_level_filters_and_skips_formatting() {
        let sink: Box<Mutex<Vec<(u32, String, String)>>> = Box::new(Mutex::new(Vec::new()));
        let logger: LoggerHandle = capture_handle(&sink, LogLevel::Error);

        let formatted: AtomicBool = AtomicBool::new(false);
        logger.log(LogLevel::Warn, "store", || {
            formatted.store(true, Ordering::SeqCst);
            String::from("must not be formatted")
        });

        // Warn (2) > max Error (1): not delivered AND the message producer
        // never ran (zero formatting cost for disabled levels).
        assert!(sink.lock().expect("sink").is_empty());
        assert!(!formatted.load(Ordering::SeqCst));

        // Error still goes through.
        logger.log(LogLevel::Error, "store", || String::from("boom"));
        assert_eq!(sink.lock().expect("sink").len(), 1);
    }

    #[test]
    fn null_callback_defaults_to_warn_cap() {
        // No callback: effective max level is Warn regardless of max_level
        // (ratified null-callback semantics), so Error/Warn reach the stderr
        // fallback branch and everything else is dropped.
        let logger = LoggerHandle {
            callback: None,
            user_data: ptr::null_mut(),
            max_level: LogLevel::Trace as u32,
        };
        assert!(logger.enabled(LogLevel::Error));
        assert!(logger.enabled(LogLevel::Warn));
        assert!(!logger.enabled(LogLevel::Info));
        assert!(!logger.enabled(LogLevel::Debug));
        assert!(!logger.enabled(LogLevel::Trace));

        let default_logger: LoggerHandle = LoggerHandle::default_stderr();
        assert!(default_logger.enabled(LogLevel::Warn));
        assert!(!default_logger.enabled(LogLevel::Info));
    }

    /// Everything `recovery_log_runs_after_guard_release` needs behind one
    /// `user_data` pointer (no statics).
    struct PoisonProbe {
        /// The lock that gets poisoned and then recovered.
        lock: Mutex<u32>,
        /// Set by the callback when it observed `lock` already released
        /// (try_lock returned Poisoned rather than WouldBlock).
        saw_released_lock: AtomicBool,
        /// Set by the callback on delivery.
        delivered: AtomicBool,
    }

    unsafe extern "C" fn probe_log(
        user_data: *mut c_void,
        level: u32,
        _scope: StringView,
        _message: StringView,
    ) {
        assert_eq!(level, LogLevel::Warn as u32);
        // SAFETY: user_data points at the test-owned PoisonProbe, alive for
        // the whole test.
        let probe: &PoisonProbe = unsafe { &*(user_data as *const PoisonProbe) };
        // If the recovered guard were still held, try_lock would fail with
        // WouldBlock. Observing Poisoned proves the lock was released BEFORE
        // the deferred recovery log fired (the lock rule).
        if matches!(probe.lock.try_lock(), Err(TryLockError::Poisoned(_))) {
            probe.saw_released_lock.store(true, Ordering::SeqCst);
        }
        probe.delivered.store(true, Ordering::SeqCst);
    }

    #[test]
    fn recovery_log_runs_after_guard_release() {
        let probe: Box<PoisonProbe> = Box::new(PoisonProbe {
            lock: Mutex::new(7),
            saw_released_lock: AtomicBool::new(false),
            delivered: AtomicBool::new(false),
        });

        // Poison the lock.
        let probe_ref: &PoisonProbe = &probe;
        let _ = thread::scope(|s| {
            s.spawn(|| {
                let _guard: MutexGuard<'_, u32> = probe_ref.lock.lock().expect("not yet poisoned");
                panic!("poison the probe lock");
            })
            .join()
        });
        assert!(probe.lock.is_poisoned());

        let logger = LoggerHandle {
            callback: Some(probe_log),
            user_data: (&*probe) as *const PoisonProbe as *mut c_void,
            max_level: LogLevel::Trace as u32,
        };

        {
            let guard: RecoveringGuard<MutexGuard<'_, u32>> =
                probe.lock.lock().recover_poisoned(logger, "test");
            assert_eq!(**guard, 7);
            // While the guard is held, the recovery log has NOT fired yet.
            assert!(!probe.delivered.load(Ordering::SeqCst));
        }

        // Guard dropped: the deferred recovery log fired, and the callback
        // observed the lock already released.
        assert!(probe.delivered.load(Ordering::SeqCst));
        assert!(probe.saw_released_lock.load(Ordering::SeqCst));
    }
}
