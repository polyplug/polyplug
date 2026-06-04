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

use polyplug_abi::HostInterface;
use polyplug_abi::runtime::RuntimeConfig;

use crate::runtime::Runtime;

// ─── FFI Entry Points (18-02: Only 2 exports) ─────────────────────────────────

/// Creates a new runtime instance.
///
/// Returns a HostInterface pointer that provides all runtime operations.
/// Callers use the HostInterface fields (load_bundle, find_guest_contract, etc.)
/// instead of separate FFI functions.
/// Pass null for `config` to use defaults.
///
/// # Safety
/// - If `config` is non-null, it must point to a valid `RuntimeConfig` struct.
/// - Safe to call from any thread.
/// - Returns null on allocation failure or panic.
///
/// # Returns
/// Pointer to HostInterface on success, null on failure.
/// The HostInterface is valid until destroyed via `polyplug_runtime_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_create(
    config: *const RuntimeConfig,
) -> *const HostInterface {
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        let mut builder = Runtime::builder();

        if !config.is_null() {
            // SAFETY: config is non-null and points to a valid RuntimeConfig per ABI contract.
            let rt_config: &RuntimeConfig = unsafe { &*config };
            builder = builder.config(rt_config.clone());

            if let Some(cb) = rt_config.on_reload {
                builder = builder.on_reload(move |phase| {
                    // SAFETY: cb is a valid extern "C" function pointer provided by the caller.
                    unsafe { cb(phase) };
                });
            }
        }

        match builder.build() {
            Ok(rt) => {
                // The HostInterface.runtime field was already patched inside build()
                // to point at the Arc's target. Hand ownership of the Arc to the caller
                // via into_raw — destroy reclaims it.
                let host_abi: &'static HostInterface = rt.host_abi();
                let runtime_ptr: *const Runtime = std::sync::Arc::into_raw(rt);

                // The HostInterface.runtime field must already equal the Arc target.
                debug_assert_eq!(
                    host_abi.runtime as *const Runtime, runtime_ptr,
                    "HostInterface.runtime must point at the Arc target"
                );

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
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        if !host.is_null() {
            // SAFETY: host is a valid HostInterface pointer returned by polyplug_runtime_create.
            // Its `runtime` field is the Arc target handed out via Arc::into_raw at creation.
            // Reconstructing the Arc and dropping it releases the Runtime and its resources.
            // The 'static HostInterface itself remains leaked, which is intentional.
            let runtime_ptr: *const core::ffi::c_void = unsafe { (*host).runtime };
            if !runtime_ptr.is_null() {
                // SAFETY: runtime_ptr was produced by Arc::into_raw in polyplug_runtime_create
                // and has not been reclaimed before (caller guarantees a single destroy).
                let _runtime: std::sync::Arc<Runtime> =
                    unsafe { std::sync::Arc::from_raw(runtime_ptr as *const Runtime) };
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
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host1: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host2: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host1.is_null());
        assert!(!host2.is_null());
        assert_ne!(host1, host2);
        // SAFETY: host1 and host2 were each returned by polyplug_runtime_create,
        // are non-null, and are destroyed exactly once here.
        unsafe {
            polyplug_runtime_destroy(host1);
            polyplug_runtime_destroy(host2);
        }
    }

    #[test]
    fn multiple_ffi_runtimes_with_config() {
        use polyplug_abi::runtime::Compatibility;

        let config1: RuntimeConfig = RuntimeConfig {
            compatibility: Compatibility::Strict,
            hot_reload_enabled: true,
            on_reload: None,
        };
        let config2: RuntimeConfig = RuntimeConfig {
            compatibility: Compatibility::Relaxed,
            hot_reload_enabled: false,
            on_reload: None,
        };

        // SAFETY: config1 points to a valid RuntimeConfig owned by this stack frame.
        let host1: *const HostInterface = unsafe { polyplug_runtime_create(&config1) };
        // SAFETY: config2 points to a valid RuntimeConfig owned by this stack frame.
        let host2: *const HostInterface = unsafe { polyplug_runtime_create(&config2) };

        assert!(!host1.is_null());
        assert!(!host2.is_null());
        assert_ne!(host1, host2);

        // SAFETY: host1 and host2 were each returned by polyplug_runtime_create,
        // are non-null, and are destroyed exactly once here.
        unsafe {
            polyplug_runtime_destroy(host1);
            polyplug_runtime_destroy(host2);
        }
    }

    #[test]
    fn host_interface_load_bundle_returns_error_on_null() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, testing null path handling
        let result = unsafe { ((*host).load_bundle)(host, core::ptr::null(), 0) };
        assert_eq!(result.code, AbiErrorCode::InvalidPointer);

        // SAFETY: host was returned by polyplug_runtime_create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_find_guest_contract_returns_null_on_empty_registry() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, testing empty registry behavior
        let handle = unsafe { ((*host).find_guest_contract)(host, 12345, 0) };
        assert!(handle.is_null());

        // SAFETY: host was returned by polyplug_runtime_create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_get_error_len_on_clean_runtime() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, no error set yet
        let len = unsafe { ((*host).get_error_len)(host) };
        assert_eq!(len, 0);

        // SAFETY: host was returned by polyplug_runtime_create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn multiple_ffi_runtimes_concurrent_operations() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::thread;

        let success_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let handles: Vec<thread::JoinHandle<()>> = (0..4)
            .map(|_| {
                let success: Arc<AtomicUsize> = Arc::clone(&success_count);
                thread::spawn(move || {
                    for _ in 0..10 {
                        // SAFETY: polyplug_runtime_create has no pointer preconditions.
                        let host: *const HostInterface =
                            unsafe { polyplug_runtime_create(core::ptr::null()) };
                        if !host.is_null() {
                            success.fetch_add(1, Ordering::SeqCst);
                            // SAFETY: host was returned by create and is destroyed once.
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
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host1: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host1.is_null());
        // SAFETY: host1 was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host1) };

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host2: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host2.is_null());
        // SAFETY: host2 was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host2) };

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host3: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host3.is_null());
        // SAFETY: host3 was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host3) };
    }

    #[test]
    fn ffi_runtime_create_with_null_options() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());
        // SAFETY: host was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn ffi_runtime_destroy_null_is_safe() {
        // SAFETY: polyplug_runtime_destroy explicitly accepts and ignores a null pointer.
        unsafe { polyplug_runtime_destroy(core::ptr::null()) };
    }

    #[test]
    fn multiple_ffi_runtimes_parallel_mixed_ops() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::thread;

        let success_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let error_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let handles: Vec<thread::JoinHandle<()>> = (0..8)
            .map(|_| {
                let success: Arc<AtomicUsize> = Arc::clone(&success_count);
                let errors: Arc<AtomicUsize> = Arc::clone(&error_count);
                thread::spawn(move || {
                    // SAFETY: polyplug_runtime_create has no pointer preconditions.
                    let host: *const HostInterface =
                        unsafe { polyplug_runtime_create(core::ptr::null()) };
                    if host.is_null() {
                        return;
                    }

                    // SAFETY: host is valid, testing load_bundle error handling
                    let result = unsafe { ((*host).load_bundle)(host, b"/bad".as_ptr(), 4) };

                    if result.code == AbiErrorCode::Ok {
                        success.fetch_add(1, Ordering::SeqCst);
                    } else {
                        errors.fetch_add(1, Ordering::SeqCst);
                    }

                    // SAFETY: host was returned by create and is destroyed once.
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
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        let null_handle = polyplug_abi::GuestContractHandle::null();
        // SAFETY: host is valid, testing null handle behavior
        let interface = unsafe { ((*host).resolve_guest_contract)(host, null_handle) };
        assert!(interface.is_null());

        // SAFETY: host was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_has_runtime_pointer() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostInterface = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, checking runtime pointer is set
        let runtime_ptr = unsafe { (*host).runtime };
        assert!(!runtime_ptr.is_null());

        // SAFETY: host was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_has_all_operation_fields() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
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

        // SAFETY: host was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }
}
