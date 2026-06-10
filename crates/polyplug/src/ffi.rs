//! FFI — public `#[no_mangle]` C ABI entry points for host language bindings.
//!
//! All functions use `catch_unwind` to prevent Rust panics from unwinding across
//! the C ABI boundary. Errors are stored per-runtime in the Runtime's last_error field.
//!
//! # FFI Surface (18-02)
//! Only two exports remain:
//! - `polyplug_runtime_create` — returns HostApi* for all operations
//! - `polyplug_runtime_destroy` — destroys the HostApi/runtime
//!
//! All runtime operations are now accessed through the HostApi struct fields:
//! - `load_bundle`, `reload_bundle` — bundle lifecycle
//! - `find_guest_contract`, `find_all_guest_contracts`, `resolve_guest_contract` — contract discovery
//! - `register_host_contract`, `register_loader` — registration
//! - `get_last_error`, `get_error_len` — error handling
//! - `alloc`, `free` — memory management

use polyplug_abi::HostApi;
use polyplug_abi::runtime::RuntimeConfig;

use crate::runtime::Runtime;

// ─── FFI Entry Points (18-02: Only 2 exports) ─────────────────────────────────

/// Creates a new runtime instance.
///
/// Returns a HostApi pointer that provides all runtime operations.
/// Callers use the HostApi fields (load_bundle, find_guest_contract, etc.)
/// instead of separate FFI functions.
/// Pass null for `config` to use defaults.
///
/// # Safety
/// - If `config` is non-null, it must point to a valid `RuntimeConfig` struct.
/// - Safe to call from any thread.
/// - Returns null on allocation failure or panic.
///
/// # Returns
/// Pointer to HostApi on success, null on failure.
/// The HostApi is valid until destroyed via `polyplug_runtime_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_create(config: *const RuntimeConfig) -> *const HostApi {
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        let mut builder = Runtime::builder();

        if !config.is_null() {
            // SAFETY: config is non-null and points to a valid RuntimeConfig per ABI contract.
            let rt_config: &RuntimeConfig = unsafe { &*config };
            builder = builder.config(rt_config.clone());

            if let Some(cb) = rt_config.on_reload {
                builder = builder.on_reload(move |user_data, phase| {
                    // SAFETY: cb is a valid extern "C" function pointer provided by the caller.
                    // user_data is the opaque pointer the caller stored in RuntimeConfig and
                    // is forwarded unchanged.
                    unsafe { cb(user_data, phase) };
                });
            }
        }

        match builder.build() {
            Ok(rt) => {
                // The HostApi.runtime field was already patched inside build()
                // to point at the Arc's target. Hand ownership of the Arc to the caller
                // via into_raw — destroy reclaims it.
                let host_abi: &'static HostApi = rt.host_abi();
                let runtime_ptr: *const Runtime = std::sync::Arc::into_raw(rt);

                // The HostApi.runtime field must already equal the Arc target.
                debug_assert_eq!(
                    host_abi.runtime as *const Runtime, runtime_ptr,
                    "HostApi.runtime must point at the Arc target"
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
///
/// Calling this more than once for the same pointer — including from multiple
/// threads concurrently — is safe. The `runtime` field is cleared with a single
/// atomic swap, so exactly one caller observes the non-null pointer and reclaims
/// the `Arc`; every other caller (concurrent or repeat) observes null and becomes a
/// no-op. There is no double-free or use-after-free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_destroy(host: *const HostApi) {
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        if !host.is_null() {
            // Atomically take ownership of the `runtime` field. Only the thread that
            // swaps OUT a non-null pointer is responsible for reclaiming the Arc; a
            // concurrent or repeat destroy swaps out null and does nothing. This
            // closes the read-then-null race where two threads could both observe a
            // non-null pointer and each reconstruct the Arc (double-free).
            //
            // SAFETY: `host` is a valid, properly aligned HostApi pointer returned by
            // polyplug_runtime_create. `AtomicPtr<c_void>` has the same size and
            // alignment as `*mut c_void`, and the `runtime` field is exactly a
            // `*mut c_void`, so viewing it through an `AtomicPtr` is layout-compatible.
            // The field is in-bounds for the duration of the swap. The runtime owns its
            // leaked 'static HostApi, so it outlives this access.
            let runtime_field: *mut core::ffi::c_void = unsafe {
                let field_ptr: *mut *mut core::ffi::c_void =
                    core::ptr::addr_of!((*host).runtime) as *mut *mut core::ffi::c_void;
                let atomic: &core::sync::atomic::AtomicPtr<core::ffi::c_void> =
                    core::sync::atomic::AtomicPtr::from_ptr(field_ptr);
                atomic.swap(core::ptr::null_mut(), core::sync::atomic::Ordering::AcqRel)
            };

            if !runtime_field.is_null() {
                // SAFETY: runtime_field was produced by Arc::into_raw in
                // polyplug_runtime_create. The atomic swap above guarantees exactly
                // one caller observes this non-null pointer, so the Arc is
                // reconstructed and dropped at most once.
                let _runtime: std::sync::Arc<Runtime> =
                    unsafe { std::sync::Arc::from_raw(runtime_field as *const Runtime) };
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
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());
        // SAFETY: host was returned by polyplug_runtime_create and is non-null.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn double_destroy_is_a_safe_noop() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host was returned by polyplug_runtime_create. The first destroy
        // reclaims the runtime and nulls (*host).runtime; the second observes the
        // null field and is a no-op (no double-free / UAF).
        unsafe {
            polyplug_runtime_destroy(host);
            // The runtime field must be null after the first reclaim.
            assert!((*host).runtime.is_null());
            polyplug_runtime_destroy(host);
        }
    }

    #[test]
    fn multiple_ffi_runtimes_are_isolated() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host1: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host2: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host1.is_null());
        assert!(!host2.is_null());
        assert_ne!(host1, host2);

        // Drive divergent state into runtime 1 ONLY: a load_bundle with a null path
        // fails and records a per-runtime last_error. Runtime 2 is left untouched.
        // SAFETY: host1 is a valid HostApi; passing a null path is the path
        // explicitly handled by host_load_bundle (sets last_error, returns error).
        let rc1: polyplug_abi::AbiError =
            unsafe { ((*host1).load_bundle)(host1, core::ptr::null(), 0) };
        assert_eq!(rc1.code, AbiErrorCode::InvalidPointer as u32);

        // Runtime 1 must now observe its own error; runtime 2 must observe none —
        // proving last_error state is instance-owned, not shared (Rule 12 isolation).
        // SAFETY: host1 is a valid HostApi pointer.
        let len1: usize = unsafe { ((*host1).get_error_len)(host1) };
        // SAFETY: host2 is a valid HostApi pointer.
        let len2: usize = unsafe { ((*host2).get_error_len)(host2) };
        assert!(
            len1 > 0,
            "runtime 1 must have its own last_error after a failed load"
        );
        assert_eq!(
            len2, 0,
            "runtime 2 must NOT see runtime 1's error — state must be isolated"
        );

        // The reverse: drive a different error into runtime 2 and confirm runtime 1's
        // error is unaffected (each keeps its own most-recent error independently).
        // SAFETY: host2 is valid; null path is the handled error path.
        let rc2: polyplug_abi::AbiError =
            unsafe { ((*host2).load_bundle)(host2, core::ptr::null(), 0) };
        assert_eq!(rc2.code, AbiErrorCode::InvalidPointer as u32);
        // SAFETY: both hosts valid.
        let len2_after: usize = unsafe { ((*host2).get_error_len)(host2) };
        assert!(len2_after > 0, "runtime 2 now has its own last_error");

        // SAFETY: host1 and host2 were each returned by polyplug_runtime_create,
        // are non-null, and are destroyed exactly once here.
        unsafe {
            polyplug_runtime_destroy(host1);
            polyplug_runtime_destroy(host2);
        }
    }

    #[test]
    fn multiple_ffi_runtimes_with_config() {
        use polyplug_abi::runtime::{Compatibility, UnloadMode};

        let config1: RuntimeConfig = RuntimeConfig {
            compatibility: Compatibility::Strict,
            unload_mode: UnloadMode::Retire,
            hot_reload_enabled: true,
            on_reload: None,
            on_reload_user_data: core::ptr::null_mut(),
            ..Default::default()
        };
        let config2: RuntimeConfig = RuntimeConfig {
            compatibility: Compatibility::Relaxed,
            unload_mode: UnloadMode::Retire,
            hot_reload_enabled: false,
            on_reload: None,
            on_reload_user_data: core::ptr::null_mut(),
            ..Default::default()
        };

        // SAFETY: config1 points to a valid RuntimeConfig owned by this stack frame.
        let host1: *const HostApi = unsafe { polyplug_runtime_create(&config1) };
        // SAFETY: config2 points to a valid RuntimeConfig owned by this stack frame.
        let host2: *const HostApi = unsafe { polyplug_runtime_create(&config2) };

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
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, testing null path handling
        let result = unsafe { ((*host).load_bundle)(host, core::ptr::null(), 0) };
        assert_eq!(result.code, AbiErrorCode::InvalidPointer as u32);

        // SAFETY: host was returned by polyplug_runtime_create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host) };
    }

    #[test]
    fn host_interface_find_guest_contract_returns_null_on_empty_registry() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
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
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
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
                        let host: *const HostApi =
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
        let host1: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host1.is_null());
        // SAFETY: host1 was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host1) };

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host2: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host2.is_null());
        // SAFETY: host2 was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host2) };

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host3: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host3.is_null());
        // SAFETY: host3 was returned by create and is destroyed once.
        unsafe { polyplug_runtime_destroy(host3) };
    }

    #[test]
    fn ffi_runtime_create_with_null_options() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
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
                    let host: *const HostApi =
                        unsafe { polyplug_runtime_create(core::ptr::null()) };
                    if host.is_null() {
                        return;
                    }

                    // SAFETY: host is valid, testing load_bundle error handling
                    let result = unsafe { ((*host).load_bundle)(host, b"/bad".as_ptr(), 4) };

                    if result.code == AbiErrorCode::Ok as u32 {
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
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
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
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
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
        let host: *const HostApi = unsafe { polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, verifying all fields are non-null function pointers
        let iface = unsafe { &*host };
        // Check each field is not null by casting to raw pointer
        let ptr = iface.register_guest_contract as *const ();
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
