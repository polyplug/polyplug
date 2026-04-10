//! FFI — public `#[no_mangle]` C ABI entry points for host language bindings.
//!
//! All functions use `catch_unwind` to prevent Rust panics from unwinding across
//! the C ABI boundary. Errors are stored per-runtime in the Runtime's last_error field.
//!
//! # FFI Surface (18-02)
//! Only two exports remain:
//! - `polyplug_runtime_create` — returns HostInterface* for all operations
//! - `polyplug_runtime_destroy` — destroys the HostInterface/runtime
//!
//! All runtime operations are now accessed through the HostInterface struct fields:
//! - `load_bundle`, `reload_bundle` — bundle lifecycle
//! - `find_guest_contract`, `find_all_guest_contracts`, `resolve_guest_contract` — contract discovery
//! - `register_host_contract`, `register_loader` — registration
//! - `get_last_error`, `get_error_len` — error handling
//! - `alloc`, `free` — memory management

use polyplug_abi::{HostInterface, types::StringView};

use crate::reload::ReloadPhase;
use crate::runtime::Runtime;
use crate::RuntimeConfig;

/// Helper to create a StringView from a Rust string slice.
fn string_view_from_str(s: &str) -> StringView {
    StringView {
        ptr: s.as_ptr(),
        len: s.len(),
    }
}

// ─── C-compatible types for hot-reload notification ───────────────────────────

/// Type tag for `ReloadPhaseFfi` variants.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadPhaseType {
    /// `Preparing` variant.
    Preparing = 0,
    /// `Reloaded` variant.
    Reloaded = 1,
    /// `Failed` variant.
    Failed = 2,
}

/// FFI-safe representation of `ReloadPhase` (not a 'C suffix' type, but an FFI variant).
///
/// This is a tagged union style struct. The `phase_type` field indicates
/// which variant is active, and the corresponding fields are populated.
///
/// # Memory Safety
///
/// All string pointers (`bundle_name`, `reason`) are borrowed from the
/// runtime's internal state and are valid only for the duration of the
/// callback invocation. The callback must NOT store these pointers or
/// free the memory.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ReloadPhaseFfi {
    /// The phase type (Preparing, Reloaded, or Failed).
    pub phase_type: u32,
    /// Bundle ID (valid for all variants).
    pub bundle_id: u64,
    /// Bundle name (valid for all variants).
    pub bundle_name: StringView,
    /// Retry count (valid only for `Preparing` variant).
    pub retry_count: u32,
    /// Failure reason (valid only for `Failed` variant).
    pub reason: StringView,
}

impl ReloadPhaseFfi {
    /// Convert a Rust `ReloadPhase` to the FFI-safe representation.
    fn from_reload_phase(phase: &ReloadPhase) -> ReloadPhaseFfi {
        match phase {
            ReloadPhase::Preparing {
                bundle_id,
                bundle_name,
                retry_count,
            } => ReloadPhaseFfi {
                phase_type: ReloadPhaseType::Preparing as u32,
                bundle_id: bundle_id.id(),
                bundle_name: string_view_from_str(bundle_name.as_str()),
                retry_count: *retry_count,
                reason: StringView::null(),
            },
            ReloadPhase::Reloaded {
                bundle_id,
                bundle_name,
            } => ReloadPhaseFfi {
                phase_type: ReloadPhaseType::Reloaded as u32,
                bundle_id: bundle_id.id(),
                bundle_name: string_view_from_str(bundle_name.as_str()),
                retry_count: 0,
                reason: StringView::null(),
            },
            ReloadPhase::Failed {
                bundle_id,
                bundle_name,
                reason,
            } => ReloadPhaseFfi {
                phase_type: ReloadPhaseType::Failed as u32,
                bundle_id: bundle_id.id(),
                bundle_name: string_view_from_str(bundle_name.as_str()),
                retry_count: 0,
                reason: string_view_from_str(reason.as_str()),
            },
        }
    }
}

// ─── C-compatible runtime configuration ───────────────────────────────────────

/// C-compatible runtime configuration.
///
/// This is a C ABI compatible version of RuntimeConfig, using integers for booleans
/// and omitting the compatibility field (uses default Strict mode).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RuntimeConfigC {
    /// Whether hot-reload is enabled (0 = false, non-zero = true).
    pub hot_reload_enabled: u32,
    /// Maximum retry attempts for hot-reload.
    pub hot_reload_max_retries: u32,
    /// Interval between retries in milliseconds.
    pub hot_reload_retry_interval_ms: u64,
    /// Abort runtime when max retries exhausted (0 = false, non-zero = true).
    pub hot_reload_abort_on_max_retries: u32,
}

impl RuntimeConfigC {
    /// Convert to the Rust RuntimeConfig type.
    fn into_runtime_config(self) -> crate::RuntimeConfig {
        crate::RuntimeConfig {
            hot_reload_enabled: self.hot_reload_enabled != 0,
            hot_reload_max_retries: self.hot_reload_max_retries,
            hot_reload_retry_interval_ms: self.hot_reload_retry_interval_ms,
            hot_reload_abort_on_max_retries: self.hot_reload_abort_on_max_retries != 0,
            compatibility: polyplug_abi::Compatibility::Strict,
        }
    }
}

/// Options for creating a runtime instance.
#[repr(C)]
pub struct RuntimeCreateOptions {
    /// Pointer to RuntimeConfigC, or null for default config.
    pub config: *const RuntimeConfigC,
    /// Reload callback function pointer, or null for no callback.
    pub on_reload: Option<extern "C" fn(ReloadPhaseFfi)>,
}

// ─── FFI Entry Points (18-02: Only 2 exports) ─────────────────────────────────

/// Creates a new runtime instance.
///
/// Returns a HostInterface pointer that provides all runtime operations.
/// Callers use the HostInterface fields (load_bundle, find_guest_contract, etc.)
/// instead of separate FFI functions.
/// Pass null for options to use defaults.
///
/// # Safety
/// - If `options` is non-null, it must point to a valid `RuntimeCreateOptions` struct.
/// - If `options.config` is non-null, it must point to a valid `RuntimeConfigC` struct.
/// - Safe to call from any thread.
/// - Returns null on allocation failure or panic.
///
/// # Returns
/// Pointer to HostInterface on success, null on failure.
/// The HostInterface is valid until destroyed via `polyplug_runtime_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_create(
    options: *const RuntimeCreateOptions,
) -> *const HostInterface {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut builder = Runtime::builder();

        if !options.is_null() {
            // SAFETY: options is non-null and points to a valid RuntimeCreateOptions per ABI contract.
            let opts: &RuntimeCreateOptions = unsafe { &*options };

            if !opts.config.is_null() {
                // SAFETY: opts.config is non-null and points to a valid RuntimeConfigC per ABI contract.
                let config_c: RuntimeConfigC = unsafe { *opts.config };
                let runtime_config: RuntimeConfig = config_c.into_runtime_config();
                builder = builder.config(runtime_config);
            }

            if let Some(cb) = opts.on_reload {
                builder = builder.on_reload(move |phase: ReloadPhase| {
                    let phase_ffi: ReloadPhaseFfi = ReloadPhaseFfi::from_reload_phase(&phase);
                    cb(phase_ffi);
                });
            }
        }

        match builder.build() {
            Ok(rt) => {
                // Box the Runtime so it can be stored and reclaimed later
                let runtime_box: Box<Runtime> = Box::new(rt);
                let runtime_ptr: *mut Runtime = Box::into_raw(runtime_box);

                // Get the HostInterface from the Runtime
                // SAFETY: runtime_ptr is valid and points to a properly constructed Runtime
                let host_abi: &'static HostInterface = unsafe { (*runtime_ptr).host_abi() };

                // Store the Runtime pointer in the HostInterface so destroy can reclaim it
                // SAFETY: host_abi.runtime is a *mut c_void that we can write to,
                // even though host_abi itself is 'static
                unsafe {
                    (*(host_abi as *const HostInterface as *mut HostInterface)).runtime =
                        runtime_ptr as *mut core::ffi::c_void;
                }

                host_abi
            }
            Err(_) => core::ptr::null(),
        }
    }))
    .unwrap_or(core::ptr::null())
}

/// Destroys a runtime instance.
///
/// # Safety
/// `host` must be a non-null pointer previously returned by `polyplug_runtime_create`.
/// Must not be called more than once for the same pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_destroy(host: *const HostInterface) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !host.is_null() {
            // SAFETY: host is a valid HostInterface pointer returned by polyplug_runtime_create.
            // The HostInterface was created by Box::leak in RuntimeBuilder.
            // To destroy, we need to:
            // 1. Get the Runtime pointer from the HostInterface
            // 2. Drop the Runtime (which will clean up all resources)
            let runtime_ptr: *mut core::ffi::c_void = unsafe { (*host).runtime };
            if !runtime_ptr.is_null() {
                // SAFETY: runtime_ptr is a valid *mut Runtime that was stored during creation.
                // Converting back to Box<Runtime> and dropping will clean up resources.
                let _runtime: Box<Runtime> = unsafe {
                    Box::from_raw(runtime_ptr as *mut Runtime)
                };
                // Note: The HostInterface itself was leaked and will remain in memory.
                // This is intentional because the HostInterface is 'static.
                // The Runtime and all its resources are properly cleaned up.
            }
        }
    }))
    .unwrap_or(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use polyplug_abi::AbiErrorCode;

    #[test]
    fn test_runtime_new_and_free() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());
        // SAFETY: host was returned by polyplug_runtime_create and is non-null.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn multiple_ffi_runtimes_are_isolated() {
        let host1: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        let host2: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host1.is_null());
        assert!(!host2.is_null());
        assert_ne!(host1, host2);
        unsafe {
            polyplug_runtime_destroy(host1);
            polyplug_runtime_destroy(host2);
        }
    }

    #[test]
    fn multiple_ffi_runtimes_with_config() {
        let config1: RuntimeConfigC = RuntimeConfigC {
            hot_reload_enabled: 1,
            hot_reload_max_retries: 5,
            hot_reload_retry_interval_ms: 1000,
            hot_reload_abort_on_max_retries: 1,
        };
        let config2: RuntimeConfigC = RuntimeConfigC {
            hot_reload_enabled: 0,
            hot_reload_max_retries: 10,
            hot_reload_retry_interval_ms: 2000,
            hot_reload_abort_on_max_retries: 0,
        };

        let opts1: RuntimeCreateOptions = RuntimeCreateOptions {
            config: &config1,
            on_reload: None,
        };
        let opts2: RuntimeCreateOptions = RuntimeCreateOptions {
            config: &config2,
            on_reload: None,
        };

        let host1: *const HostInterface = unsafe { polyplug_runtime_create(&opts1) };
        let host2: *const HostInterface = unsafe { polyplug_runtime_create(&opts2) };

        assert!(!host1.is_null());
        assert!(!host2.is_null());
        assert_ne!(host1, host2);

        unsafe {
            polyplug_runtime_destroy(host1);
            polyplug_runtime_destroy(host2);
        }
    }

    #[test]
    fn host_interface_load_bundle_returns_error_on_null() {
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, testing null path handling
        let result = unsafe {
            ((*host).load_bundle)(host, core::ptr::null(), 0)
        };
        assert_eq!(result.code, AbiErrorCode::InvalidPointer);

        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_find_guest_contract_returns_null_on_empty_registry() {
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, testing empty registry behavior
        let handle = unsafe {
            ((*host).find_guest_contract)(host, 12345, 0)
        };
        assert!(handle.is_null());

        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_get_error_len_on_clean_runtime() {
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, no error set yet
        let len = unsafe {
            ((*host).get_error_len)(host)
        };
        assert_eq!(len, 0);

        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn multiple_ffi_runtimes_concurrent_operations() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::thread;

        let success_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let handles: Vec<thread::JoinHandle<()>> = (0..4)
            .map(|_| {
                let success: Arc<AtomicUsize> = Arc::clone(&success_count);
                thread::spawn(move || {
                    for _ in 0..10 {
                        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
                        if !host.is_null() {
                            success.fetch_add(1, Ordering::SeqCst);
                            unsafe { polyplug_runtime_destroy(host) };
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread should not panic");
        }

        assert_eq!(success_count.load(Ordering::SeqCst), 40);
    }

    #[test]
    fn multiple_ffi_runtimes_lifecycle_interleaved() {
        let host1: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host1.is_null());
        unsafe { polyplug_runtime_destroy(host1) };

        let host2: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host2.is_null());
        unsafe { polyplug_runtime_destroy(host2) };

        let host3: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host3.is_null());
        unsafe { polyplug_runtime_destroy(host3) };
    }

    #[test]
    fn ffi_runtime_create_with_null_options() {
        let host: *const HostInterface =
            unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn ffi_runtime_destroy_null_is_safe() {
        unsafe { polyplug_runtime_destroy(core::ptr::null()) };
    }

    #[test]
    fn multiple_ffi_runtimes_parallel_mixed_ops() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::thread;

        let success_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let error_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let handles: Vec<thread::JoinHandle<()>> = (0..8)
            .map(|_| {
                let success: Arc<AtomicUsize> = Arc::clone(&success_count);
                let errors: Arc<AtomicUsize> = Arc::clone(&error_count);
                thread::spawn(move || {
                    let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
                    if host.is_null() {
                        return;
                    }

                    // SAFETY: host is valid, testing load_bundle error handling
                    let result = unsafe {
                        ((*host).load_bundle)(host, b"/bad".as_ptr(), 4)
                    };

                    if result.code == AbiErrorCode::Ok {
                        success.fetch_add(1, Ordering::SeqCst);
                    } else {
                        errors.fetch_add(1, Ordering::SeqCst);
                    }

                    unsafe { polyplug_runtime_destroy(host) };
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread should not panic");
        }

        assert_eq!(
            success_count.load(Ordering::SeqCst) + error_count.load(Ordering::SeqCst),
            8
        );
    }

    #[test]
    fn host_interface_resolve_guest_contract_returns_null_on_null_handle() {
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, testing null handle behavior
        let null_handle = polyplug_abi::GuestContractHandle::null();
        let interface = unsafe {
            ((*host).resolve_guest_contract)(host, null_handle)
        };
        assert!(interface.is_null());

        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_has_runtime_pointer() {
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, checking runtime pointer is set
        let runtime_ptr = unsafe { (*host).runtime };
        assert!(!runtime_ptr.is_null());

        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_has_all_operation_fields() {
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, verifying all fields are non-null function pointers
        let iface = unsafe { &*host };
        // Check each field is not null by casting to raw pointer
        let ptr = iface.register_contract as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.alloc as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.free as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.find_guest_contract as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.find_all_guest_contracts as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.resolve_guest_contract as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.call_guest_method as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.get_host_contract as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.resolve_host_contract_interface as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.list_bundles as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.get_dependencies as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.load_bundle as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.reload_bundle as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.register_host_contract as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.register_loader as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.get_last_error as *const ();
        assert!(!ptr.is_null());
        let ptr = iface.get_error_len as *const ();
        assert!(!ptr.is_null());

        unsafe { polyplug_runtime_destroy(host) };
    }
}