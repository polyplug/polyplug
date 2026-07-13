//! FFI — public `#[no_mangle]` C ABI entry points for host language bindings.
//!
//! The two exports below (`polyplug_runtime_create` / `polyplug_runtime_destroy`)
//! catch their own panics. This is the **embedder guarantee**: a create failure
//! returns null; destroy failures before raw `Arc` consumption return `false`; and
//! teardown panics after consumption return `true`. None unwind across the C ABI and
//! abort the embedding host process. These two are the *only* runtime-side panic guards — the
//! `HostApi` field operations (`load_bundle`, `find_guest_contract`, …) are
//! intentionally NOT guarded: a bug in the runtime there fails fast. Foreign-plugin
//! failures are the plugin's own responsibility — each language's generated glue
//! converts them to an `AbiError` before the boundary (see docs/TRUST_MODEL.md);
//! the runtime does not absorb a panic/exception that escapes a plugin's glue.
//! Per-runtime errors are stored in the Runtime's `last_error` field.
//!
//! # FFI Surface (18-02)
//! Only two exports remain:
//! - `polyplug_runtime_create` — returns HostApi* for all operations
//! - `polyplug_runtime_destroy` — destroys the HostApi/runtime
//!
//! All runtime operations are now accessed through the HostApi struct fields:
//! - `load_bundle`, `reload_bundle`, `unload_bundle` — bundle lifecycle
//! - `begin_internal_plugin`, `commit_internal_plugin`, `abort_internal_plugin` — internal-plugin registration
//! - `find_guest_contract`, `find_all_guest_contracts`, `resolve_guest_contract` — contract discovery
//! - `register_host_contract`, `register_loader` — registration
//! - `alloc`, `free` — memory management

use core::ffi::c_void;
use core::panic::AssertUnwindSafe;
use core::{ptr, slice, str};
use std::panic::catch_unwind;
use std::sync::Arc;

use polyplug_abi::runtime::{ReloadPhase, RuntimeConfig};
use polyplug_abi::{
    AbiError, AbiErrorCode, GuestContractHandle, HostApi, StringView, SupportedLanguage,
};
use polyplug_common::ManifestData;
use polyplug_utils::BundleId;

use crate::runtime::Runtime;
use crate::runtime::current_os_thread_id;

/// Callback used by a native internal-plugin adapter to release its opaque resident.
pub type InternalPluginResidentRelease = unsafe extern "C" fn(*mut c_void);

// ─── FFI Entry Points ─────────────────────────────────────────────────────────

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
/// Attempt `polyplug_runtime_destroy` until it returns `true`: a `false` result means
/// destruction failed before consuming the runtime reference, leaving the pointer valid
/// for an owner-thread retry. After the single `true` result, the non-null pointer is
/// consumed and must never be used or passed to destroy again, including when teardown
/// caught a panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_create(config: *const RuntimeConfig) -> *const HostApi {
    catch_unwind(AssertUnwindSafe(|| {
        let mut builder = Runtime::builder();

        if !config.is_null() {
            // SAFETY: config is non-null and points to a valid RuntimeConfig per ABI contract.
            let rt_config: &RuntimeConfig = unsafe { &*config };
            builder = builder.config(rt_config.clone());

            if let Some(cb) = rt_config.on_reload {
                builder = builder.on_reload(move |user_data, phase| {
                    // SAFETY: cb is a valid extern "C" function pointer provided by the caller.
                    // user_data is the opaque pointer the caller stored in RuntimeConfig and
                    // is forwarded unchanged. `&phase` is a non-null, properly aligned
                    // pointer to a ReloadPhase that lives on this stack frame for the
                    // whole call — the ABI contract only requires the pointee to be valid
                    // for the duration of the callback.
                    unsafe { cb(user_data, &phase as *const ReloadPhase) };
                });
            }
        }

        match builder.build() {
            Ok(rt) => {
                // The HostApi.runtime field was already patched inside build()
                // to point at the Arc's target. Hand ownership of the Arc to the caller
                // via into_raw — destroy reclaims it. The HostApi is owned by the
                // Runtime (its `host_abi` box), so this pointer stays valid until
                // `polyplug_runtime_destroy` drops the Arc and, with it, the Runtime.
                let host_abi: *const HostApi = rt.host_abi();
                let runtime_ptr: *const Runtime = Arc::into_raw(rt);

                // The HostApi.runtime field must already equal the Arc target.
                // SAFETY: `host_abi` is the runtime-owned HostApi pointer; the Arc
                // (and thus the Runtime owning the box) is still alive here, so the
                // read of its `runtime` field is in-bounds and valid.
                let stored_runtime: *const Runtime =
                    unsafe { (*host_abi).runtime as *const Runtime };
                debug_assert_eq!(
                    stored_runtime, runtime_ptr,
                    "HostApi.runtime must point at the Arc target"
                );

                host_abi
            }
            Err(_) => ptr::null(),
        }
    }))
    .unwrap_or(ptr::null())
}

/// Destroys a runtime instance.
///
/// Returns `true` after consuming a valid runtime reference, and for a null `host`.
/// An owner-affinity rejection or caught panic before consumption returns `false` and
/// leaves a non-null `host` live for its owner to retry. Once raw `Arc` consumption
/// begins, this function returns `true` even when teardown catches a panic; that
/// non-null `host` has been consumed and must not be used again.
///
/// # Safety
/// Must be called once with a `host` pointer previously returned by
/// `polyplug_runtime_create`, unless a previous call returned `false`. Calling it more
/// than once after it returns `true`, or concurrently with itself on the same handle,
/// is undefined behavior.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_destroy(host: *const HostApi) -> bool {
    if host.is_null() {
        return true;
    }

    // A foreign-thread final drop cannot safely release native residents. Keep all
    // checks that precede raw Arc reconstruction in this guard so their panic leaves
    // the host's Arc reference intact and retryable.
    let runtime_ptr: *const Runtime = match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `(*host).runtime` was produced by `Arc::into_raw` in
        // `polyplug_runtime_create` and `host` is a valid, properly aligned pointer
        // returned by it. The raw Arc remains owned by this HostApi during this check.
        let runtime_ptr: *const Runtime = unsafe { (*host).runtime as *const Runtime };
        if !runtime_ptr.is_null() {
            // SAFETY: the raw Arc remains owned by this HostApi during this check.
            let runtime: &Runtime = unsafe { &*runtime_ptr };
            if !runtime.can_destroy_on_current_thread() {
                return None;
            }
        }
        Some(runtime_ptr)
    })) {
        Ok(Some(runtime_ptr)) => runtime_ptr,
        Ok(None) | Err(_) => return false,
    };

    if runtime_ptr.is_null() {
        return true;
    }

    // From this point ownership is terminal: Arc::from_raw balances the reference
    // transferred by create before any teardown can panic.
    // SAFETY: this balances the Arc::into_raw from create exactly once.
    let runtime: Arc<Runtime> = unsafe { Arc::from_raw(runtime_ptr) };
    let _ = catch_unwind(AssertUnwindSafe(|| drop(runtime)));
    true
}

/// Begin an internal-plugin registration transaction using canonical manifest TOML.
///
/// Call `HostApi::register_guest_contract` for every provider before committing. The
/// manifest bytes are copied while parsing; no registration envelope crosses the ABI.
///
/// # Safety
///
/// `host` must be a live runtime `HostApi`. Non-null output pointers must be
/// writable, and `manifest_bytes` must reference `manifest_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_begin_internal_plugin(
    host: *const HostApi,
    manifest_bytes: *const u8,
    manifest_len: usize,
    language: u32,
    out_bundle_id: *mut u64,
    out_error: *mut AbiError,
) {
    if !out_bundle_id.is_null() {
        // SAFETY: caller provided writable result storage.
        unsafe { out_bundle_id.write(0) };
    }
    let result: Result<u64, String> = catch_unwind(AssertUnwindSafe(|| {
        if host.is_null()
            || out_bundle_id.is_null()
            || (manifest_bytes.is_null() && manifest_len != 0)
        {
            return Err("invalid internal-plugin registration pointer".to_owned());
        }
        let language: SupportedLanguage = match language {
            0 => SupportedLanguage::Rust,
            1 => SupportedLanguage::Cpp,
            2 => SupportedLanguage::Dotnet,
            3 => SupportedLanguage::Python,
            4 => SupportedLanguage::Lua,
            5 => SupportedLanguage::JavaScript,
            _ => return Err("invalid internal-plugin language".to_owned()),
        };
        let manifest_bytes: &[u8] = if manifest_len == 0 {
            &[]
        } else {
            // SAFETY: non-nullness and byte length are validated above.
            unsafe { slice::from_raw_parts(manifest_bytes, manifest_len) }
        };
        let manifest_text: &str =
            str::from_utf8(manifest_bytes).map_err(|_| "manifest TOML is not UTF-8".to_owned())?;
        let manifest: ManifestData =
            ManifestData::parse_from_str(manifest_text).map_err(|error| error.to_string())?;
        // SAFETY: host is a live HostApi for the whole registration transaction.
        let runtime_ptr: *const Runtime = unsafe { (*host).runtime.cast() };
        if runtime_ptr.is_null() {
            return Err("runtime pointer is null".to_owned());
        }
        // SAFETY: HostApi.runtime is owned by this live runtime.
        let runtime: &Runtime = unsafe { &*runtime_ptr };
        runtime
            .begin_internal_plugin(manifest, language)
            .map(|bundle_id| bundle_id.id())
            .map_err(|error| error.to_string())
    }))
    .unwrap_or_else(|_| Err("internal-plugin registration panicked".to_owned()));
    match result {
        Ok(bundle_id) => {
            // SAFETY: out_bundle_id is non-null on the success path.
            unsafe { out_bundle_id.write(bundle_id) };
            if !out_error.is_null() {
                // SAFETY: caller provided writable result storage.
                unsafe { out_error.write(AbiError::ok()) };
            }
        }
        Err(error) => write_internal_plugin_error(host, out_error, error),
    }
}

/// Return the caller's OS thread identity for native resident ownership.
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_current_os_thread_id() -> u64 {
    current_os_thread_id()
}

/// Attach an opaque native resident to a staged internal-plugin transaction.
///
/// The adapter must call this from `owner_thread_id` after begin and before commit
/// or abort. A successful call transfers the resident to core; failure leaves it
/// owned by the adapter.
///
/// # Safety
///
/// `host` must be a live runtime `HostApi`. `resident` and `release` must be
/// non-null, and `out_error` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_attach_internal_plugin_resident(
    host: *const HostApi,
    bundle_id: u64,
    resident: *mut c_void,
    owner_thread_id: u64,
    release: Option<InternalPluginResidentRelease>,
    out_error: *mut AbiError,
) -> bool {
    let result: Result<(), String> = catch_unwind(AssertUnwindSafe(|| {
        if host.is_null() {
            return Err("HostApi pointer is null".to_owned());
        }
        if resident.is_null() {
            return Err("native internal-plugin resident must be non-null".to_owned());
        }
        let release: InternalPluginResidentRelease = release
            .ok_or_else(|| "native internal-plugin release callback must be non-null".to_owned())?;
        if owner_thread_id == 0 {
            return Err("native internal-plugin owner thread ID must be nonzero".to_owned());
        }
        // SAFETY: host is non-null and belongs to a live runtime.
        let runtime_ptr: *const Runtime = unsafe { (*host).runtime.cast() };
        if runtime_ptr.is_null() {
            return Err("runtime pointer is null".to_owned());
        }
        // SAFETY: host is live for this registration call.
        unsafe { &*runtime_ptr }
            .attach_internal_plugin_resident(
                BundleId::from_u64(bundle_id),
                resident,
                owner_thread_id,
                release,
            )
            .map_err(|error| error.to_string())
    }))
    .unwrap_or_else(|_| Err("native internal-plugin resident attachment panicked".to_owned()));
    match result {
        Ok(()) => {
            if !out_error.is_null() {
                // SAFETY: caller provided writable result storage.
                unsafe { out_error.write(AbiError::ok()) };
            }
            true
        }
        Err(error) => {
            write_internal_plugin_error(host, out_error, error);
            false
        }
    }
}

/// Commit an internal-plugin registration transaction after all guest contracts staged.
///
/// # Safety
///
/// `host` must be the live runtime that began `bundle_id`, and a non-null
/// `out_error` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_commit_internal_plugin(
    host: *const HostApi,
    bundle_id: u64,
    out_error: *mut AbiError,
) {
    let result: Result<(), String> = catch_unwind(AssertUnwindSafe(|| {
        if host.is_null() {
            return Err("HostApi pointer is null".to_owned());
        }
        // SAFETY: host is non-null and belongs to a live runtime.
        let runtime_ptr: *const Runtime = unsafe { (*host).runtime.cast() };
        if runtime_ptr.is_null() {
            return Err("runtime pointer is null".to_owned());
        }
        // SAFETY: HostApi.runtime is owned by this live runtime.
        unsafe { &*runtime_ptr }
            .commit_internal_plugin(BundleId::from_u64(bundle_id))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }))
    .unwrap_or_else(|_| Err("internal-plugin registration panicked".to_owned()));
    match result {
        Ok(()) if !out_error.is_null() => {
            // SAFETY: caller provided writable result storage.
            unsafe { out_error.write(AbiError::ok()) };
        }
        Ok(()) => {}
        Err(error) => write_internal_plugin_error(host, out_error, error),
    }
}

/// Commit an internal-plugin registration transaction and return its exact staged handles.
///
/// The caller supplies a buffer sized to the generated provider count. Capacity is
/// checked before publication, and success writes handles in the same order that
/// `HostApi::register_guest_contract` staged the providers.
///
/// # Safety
///
/// `host` must be the live runtime that began `bundle_id`; `out_handles` must be
/// writable for `handle_capacity` entries when that capacity is nonzero; and
/// `out_handle_count` and `out_error` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_commit_internal_plugin_with_handles(
    host: *const HostApi,
    bundle_id: u64,
    out_handles: *mut GuestContractHandle,
    handle_capacity: usize,
    out_handle_count: *mut usize,
    out_error: *mut AbiError,
) {
    let result: Result<usize, String> = catch_unwind(AssertUnwindSafe(|| {
        if host.is_null() {
            return Err("HostApi pointer is null".to_owned());
        }
        if out_handle_count.is_null() {
            return Err("out_handle_count pointer is null".to_owned());
        }
        if handle_capacity > 0 && out_handles.is_null() {
            return Err("out_handles pointer is null for nonzero capacity".to_owned());
        }
        // SAFETY: host is non-null and belongs to a live runtime.
        let runtime_ptr: *const Runtime = unsafe { (*host).runtime.cast() };
        if runtime_ptr.is_null() {
            return Err("runtime pointer is null".to_owned());
        }
        let handles: &mut [GuestContractHandle] = if handle_capacity == 0 {
            &mut []
        } else {
            // SAFETY: non-nullness and entry capacity are validated above.
            unsafe { slice::from_raw_parts_mut(out_handles, handle_capacity) }
        };
        // SAFETY: HostApi.runtime is owned by this live runtime.
        unsafe { &*runtime_ptr }
            .commit_internal_plugin_into_handles(BundleId::from_u64(bundle_id), handles)
            .map_err(|error| error.to_string())
    }))
    .unwrap_or_else(|_| Err("internal-plugin registration panicked".to_owned()));
    match result {
        Ok(handle_count) => {
            // SAFETY: non-nullness was checked before commit.
            unsafe { out_handle_count.write(handle_count) };
            if !out_error.is_null() {
                // SAFETY: caller provided writable result storage.
                unsafe { out_error.write(AbiError::ok()) };
            }
        }
        Err(error) => write_internal_plugin_error(host, out_error, error),
    }
}

/// Abort an uncommitted internal-plugin registration transaction and release staged data.
///
/// # Safety
///
/// `host` must be the live runtime that began `bundle_id`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_abort_internal_plugin(host: *const HostApi, bundle_id: u64) {
    if host.is_null() {
        return;
    }
    // SAFETY: a non-null host belongs to a live runtime for this call.
    let runtime_ptr: *const Runtime = unsafe { (*host).runtime.cast() };
    if !runtime_ptr.is_null() {
        // SAFETY: HostApi.runtime is owned by this live runtime.
        unsafe { &*runtime_ptr }.abort_internal_plugin(BundleId::from_u64(bundle_id));
    }
}

fn write_internal_plugin_error(host: *const HostApi, out_error: *mut AbiError, error: String) {
    if !host.is_null() {
        // SAFETY: non-null HostApi belongs to a live runtime for this callback.
        let runtime_ptr: *const Runtime = unsafe { (*host).runtime.cast() };
        if !runtime_ptr.is_null() {
            // SAFETY: HostApi.runtime is owned by this live runtime.
            unsafe { &*runtime_ptr }.set_last_error(error);
        }
    }
    if !out_error.is_null() {
        // SAFETY: caller provided writable result storage.
        unsafe {
            out_error.write(AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use core::ffi::c_void;
    use core::ptr;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    use polyplug_abi::runtime::Compatibility;
    use polyplug_abi::{AbiError, AbiErrorCode, GuestContractHandle};
    use polyplug_utils::BundleId;

    use crate::runtime_store::InternalPluginResident;

    use super::*;

    #[test]
    fn test_runtime_new_and_free() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());
        // SAFETY: host was returned by polyplug_runtime_create and is non-null.
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }

    #[test]
    fn runtime_destroy_is_terminal_after_runtime_owned_root_teardown_panics() {
        struct PanicOnDrop {
            drops: Arc<AtomicUsize>,
        }

        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
                panic!("runtime-owned root teardown panicked");
            }
        }

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());
        let drops: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        // SAFETY: host is live and points to its runtime for the duration of this test.
        let runtime: &Runtime = unsafe { &*((*host).runtime as *const Runtime) };
        runtime
            .internal_plugin_roots
            .lock()
            .expect("internal plugin roots mutex must not be poisoned")
            .insert(
                BundleId::from_u64(0xA173_3A09_4E02_0005),
                Box::new(PanicOnDrop {
                    drops: Arc::clone(&drops),
                }),
            );

        // SAFETY: `host` is the live, uniquely owned pointer returned above; destroy
        // terminally consumes that ownership exactly once, even when root teardown panics.
        assert!(unsafe { polyplug_runtime_destroy(host) });
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn off_owner_runtime_destroy_preserves_native_resident_for_owner_release() {
        unsafe extern "C" fn release_resident(context: *mut c_void) {
            // SAFETY: this callback owns the allocation transferred into the resident.
            let releases: Box<Arc<AtomicUsize>> =
                unsafe { Box::from_raw(context.cast::<Arc<AtomicUsize>>()) };
            releases.fetch_add(1, Ordering::SeqCst);
        }

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());
        let releases: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let context: *mut c_void = Box::into_raw(Box::new(Arc::clone(&releases))).cast();
        // SAFETY: host is live and points to its runtime for the duration of this test.
        let runtime: &Runtime = unsafe { &*((*host).runtime as *const Runtime) };
        runtime.registry.lock_internal_plugin_residents().insert(
            BundleId::from_u64(0xA173_3A09_4E02_0004),
            InternalPluginResident::new(context, current_os_thread_id(), release_resident),
        );

        let host_address: usize = host as usize;
        let destroyed = thread::spawn(move || {
            // SAFETY: the off-owner call must leave the valid host handle unconsumed.
            unsafe { polyplug_runtime_destroy(host_address as *const HostApi) }
        })
        .join()
        .expect("off-owner destroy must return");
        assert!(
            !destroyed,
            "off-owner destruction must report a retryable failure"
        );
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        assert!(
            runtime
                .registry
                .lock_internal_plugin_residents()
                .contains_key(&BundleId::from_u64(0xA173_3A09_4E02_0004)),
            "off-owner destruction must leave the resident attached for its owner"
        );

        // SAFETY: the owner-thread retry consumes the original runtime handle once.
        assert!(unsafe { polyplug_runtime_destroy(host) });
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn multiple_ffi_runtimes_are_isolated() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host1: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host2: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host1.is_null());
        assert!(!host2.is_null());
        assert_ne!(host1, host2);

        // Drive divergent state into runtime 1 ONLY: a load_bundle with a null path
        // fails and records a per-runtime last_error. Runtime 2 is left untouched.
        // SAFETY: host1 is a valid HostApi; passing a null path is the path
        // explicitly handled by host_load_bundle (sets last_error, returns error).
        let mut rc1: AbiError = AbiError::ok();
        // SAFETY: rc1 is a valid, writable out-param for the load_bundle result.
        unsafe { ((*host1).load_bundle)(host1, ptr::null(), 0, &mut rc1) };
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
        let mut rc2: AbiError = AbiError::ok();
        // SAFETY: rc2 is a valid, writable out-param for the load_bundle result.
        unsafe { ((*host2).load_bundle)(host2, ptr::null(), 0, &mut rc2) };
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
        let config1: RuntimeConfig = RuntimeConfig {
            compatibility: Compatibility::Strict,
            hot_reload_enabled: true,
            on_reload: None,
            on_reload_user_data: ptr::null_mut(),
            ..Default::default()
        };
        let config2: RuntimeConfig = RuntimeConfig {
            compatibility: Compatibility::Relaxed,
            hot_reload_enabled: false,
            on_reload: None,
            on_reload_user_data: ptr::null_mut(),
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
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());

        let mut result: AbiError = AbiError::ok();
        // SAFETY: host is valid, testing null path handling; result is a valid out-param.
        unsafe { ((*host).load_bundle)(host, ptr::null(), 0, &mut result) };
        assert_eq!(result.code, AbiErrorCode::InvalidPointer as u32);

        // SAFETY: host was returned by polyplug_runtime_create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }

    #[test]
    fn host_interface_find_guest_contract_returns_null_on_empty_registry() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, testing empty registry behavior
        let handle = unsafe { ((*host).find_guest_contract)(host, 12345, 0) };
        assert!(handle.is_null());

        // SAFETY: host was returned by polyplug_runtime_create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }

    #[test]
    fn host_interface_get_error_len_on_clean_runtime() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, no error set yet
        let len = unsafe { ((*host).get_error_len)(host) };
        assert_eq!(len, 0);

        // SAFETY: host was returned by polyplug_runtime_create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }

    #[test]
    fn multiple_ffi_runtimes_concurrent_operations() {
        let success_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let handles: Vec<thread::JoinHandle<()>> = (0..4)
            .map(|_| {
                let success: Arc<AtomicUsize> = Arc::clone(&success_count);
                thread::spawn(move || {
                    for _ in 0..10 {
                        // SAFETY: polyplug_runtime_create has no pointer preconditions.
                        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
                        if !host.is_null() {
                            success.fetch_add(1, Ordering::SeqCst);
                            // SAFETY: host was returned by create and is destroyed once.
                            assert!(unsafe { polyplug_runtime_destroy(host) });
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
        let host1: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host1.is_null());
        // SAFETY: host1 was returned by create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host1) });

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host2: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host2.is_null());
        // SAFETY: host2 was returned by create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host2) });

        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host3: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host3.is_null());
        // SAFETY: host3 was returned by create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host3) });
    }

    #[test]
    fn ffi_runtime_create_with_null_options() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());
        // SAFETY: host was returned by create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }

    #[test]
    fn ffi_runtime_destroy_null_is_safe() {
        // SAFETY: polyplug_runtime_destroy explicitly accepts and ignores a null pointer.
        assert!(unsafe { polyplug_runtime_destroy(ptr::null()) });
    }

    #[test]
    fn multiple_ffi_runtimes_parallel_mixed_ops() {
        let success_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let error_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let handles: Vec<thread::JoinHandle<()>> = (0..8)
            .map(|_| {
                let success: Arc<AtomicUsize> = Arc::clone(&success_count);
                let errors: Arc<AtomicUsize> = Arc::clone(&error_count);
                thread::spawn(move || {
                    // SAFETY: polyplug_runtime_create has no pointer preconditions.
                    let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
                    if host.is_null() {
                        return;
                    }

                    let mut result: AbiError = AbiError::ok();
                    // SAFETY: host is valid, testing load_bundle error handling;
                    // result is a valid, writable out-param.
                    unsafe { ((*host).load_bundle)(host, b"/bad".as_ptr(), 4, &mut result) };

                    if result.code == AbiErrorCode::Ok as u32 {
                        success.fetch_add(1, Ordering::SeqCst);
                    } else {
                        errors.fetch_add(1, Ordering::SeqCst);
                    }

                    // SAFETY: host was returned by create and is destroyed once.
                    assert!(unsafe { polyplug_runtime_destroy(host) });
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
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());

        let null_handle = GuestContractHandle::null();
        // SAFETY: host is valid, testing null handle behavior
        let interface = unsafe { ((*host).resolve_guest_contract)(host, null_handle) };
        assert!(interface.is_null());

        // SAFETY: host was returned by create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }

    #[test]
    fn host_interface_has_runtime_pointer() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
        assert!(!host.is_null());

        // SAFETY: host is valid, checking runtime pointer is set
        let runtime_ptr = unsafe { (*host).runtime };
        assert!(!runtime_ptr.is_null());

        // SAFETY: host was returned by create and is destroyed once.
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }

    #[test]
    fn host_interface_has_all_operation_fields() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let host: *const HostApi = unsafe { polyplug_runtime_create(ptr::null()) };
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
        assert!(unsafe { polyplug_runtime_destroy(host) });
    }
}
