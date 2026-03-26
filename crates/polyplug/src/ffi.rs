//! FFI — public `#[no_mangle]` C ABI entry points for host language bindings.
//!
//! All functions use `catch_unwind` to prevent Rust panics from unwinding across
//! the C ABI boundary. Errors are stored per-runtime in the Runtime's last_error field.

#![allow(clippy::std_instead_of_core)]

use crate::loader::BundleLoader;
use crate::reload::ReloadPhase;
use crate::runtime::Runtime;
use crate::runtime::RuntimeConfig;
use polyplug_abi::PluginHandle;

pub struct OpaqueRuntime(pub Runtime);

// ─── C-compatible types for hot-reload notification ───────────────────────────

/// Type tag for `ReloadPhaseC` variants.
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

/// C-compatible string view for passing strings across the FFI boundary.
///
/// The pointer must remain valid for the duration of the callback call.
/// This is a borrowed view — the callback must NOT free the memory.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StringViewC {
    /// Pointer to UTF-8 bytes.
    pub ptr: *const u8,
    /// Length in bytes.
    pub len: usize,
}

impl StringViewC {
    /// Create a `StringViewC` from a Rust string slice.
    fn from_str(s: &str) -> StringViewC {
        StringViewC {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }
}

/// C-compatible representation of `ReloadPhase`.
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
pub struct ReloadPhaseC {
    /// The phase type (Preparing, Reloaded, or Failed).
    pub phase_type: u32,
    /// Bundle ID (valid for all variants).
    pub bundle_id: u64,
    /// Bundle name (valid for all variants).
    pub bundle_name: StringViewC,
    /// Retry count (valid only for `Preparing` variant).
    pub retry_count: u32,
    /// Failure reason (valid only for `Failed` variant).
    pub reason: StringViewC,
}

impl ReloadPhaseC {
    /// Convert a Rust `ReloadPhase` to the C-compatible representation.
    fn from_reload_phase(phase: &ReloadPhase) -> ReloadPhaseC {
        match phase {
            ReloadPhase::Preparing {
                bundle_id,
                bundle_name,
                retry_count,
            } => ReloadPhaseC {
                phase_type: ReloadPhaseType::Preparing as u32,
                bundle_id: *bundle_id,
                bundle_name: StringViewC::from_str(bundle_name.as_str()),
                retry_count: *retry_count,
                reason: StringViewC {
                    ptr: core::ptr::null(),
                    len: 0,
                },
            },
            ReloadPhase::Reloaded {
                bundle_id,
                bundle_name,
            } => ReloadPhaseC {
                phase_type: ReloadPhaseType::Reloaded as u32,
                bundle_id: *bundle_id,
                bundle_name: StringViewC::from_str(bundle_name.as_str()),
                retry_count: 0,
                reason: StringViewC {
                    ptr: core::ptr::null(),
                    len: 0,
                },
            },
            ReloadPhase::Failed {
                bundle_id,
                bundle_name,
                reason,
            } => ReloadPhaseC {
                phase_type: ReloadPhaseType::Failed as u32,
                bundle_id: *bundle_id,
                bundle_name: StringViewC::from_str(bundle_name.as_str()),
                retry_count: 0,
                reason: StringViewC::from_str(reason.as_str()),
            },
        }
    }
}

/// C-compatible configuration for hot-reload behavior.
///
/// Pass this to `polyplug_runtime_create_with_options` to configure the runtime.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RuntimeConfigC {
    /// Maximum number of retry attempts for hot-reload operations.
    pub hot_reload_max_retries: u32,
    /// Interval between hot-reload retry attempts, in milliseconds.
    pub hot_reload_retry_interval_ms: u64,
    /// Whether to abort the runtime when max retries are exhausted.
    /// 0 = false (continue retrying), non-zero = true (abort).
    pub hot_reload_abort_on_max_retries: u8,
}

impl RuntimeConfigC {
    /// Convert to the Rust `RuntimeConfig` type.
    fn into_runtime_config(self) -> RuntimeConfig {
        RuntimeConfig {
            hot_reload_max_retries: self.hot_reload_max_retries,
            hot_reload_retry_interval: core::time::Duration::from_millis(
                self.hot_reload_retry_interval_ms,
            ),
            hot_reload_abort_on_max_retries: self.hot_reload_abort_on_max_retries != 0,
        }
    }
}

// ─── Helper functions ──────────────────────────────────────────────────────────

fn pack_handle(h: PluginHandle) -> u64 {
    if h.is_null() {
        u64::MAX
    } else {
        (h.generation as u64) << 32 | h.index as u64
    }
}

fn unpack_handle(packed: u64) -> PluginHandle {
    if packed == u64::MAX {
        PluginHandle::null()
    } else {
        PluginHandle {
            index: (packed & 0xFFFF_FFFF) as u32,
            generation: (packed >> 32) as u32,
        }
    }
}

/// Creates a new runtime instance with default configuration.
///
/// # Safety
/// Safe to call from any thread. No pointer arguments are required.
/// Returns null on allocation failure or panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_create() -> *mut OpaqueRuntime {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Runtime::builder().build() {
            Ok(rt) => Box::into_raw(Box::new(OpaqueRuntime(rt))),
            Err(_) => core::ptr::null_mut(),
        }
    }))
    .unwrap_or(core::ptr::null_mut())
}

/// Options for creating a runtime instance.
#[repr(C)]
pub struct RuntimeCreateOptions {
    /// Pointer to RuntimeConfigC, or null for default config.
    pub config: *const RuntimeConfigC,
    /// Reload callback function pointer, or null for no callback.
    pub on_reload: Option<extern "C" fn(ReloadPhaseC)>,
}

/// Creates a new runtime instance with the specified options.
///
/// # Safety
/// - If `options` is non-null, it must point to a valid `RuntimeCreateOptions` struct.
/// - If `options.config` is non-null, it must point to a valid `RuntimeConfigC` struct.
/// - Safe to call from any thread.
/// - Returns null on allocation failure or panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_create_with_options(
    options: *const RuntimeCreateOptions,
) -> *mut OpaqueRuntime {
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
                    let phase_c: ReloadPhaseC = ReloadPhaseC::from_reload_phase(&phase);
                    cb(phase_c);
                });
            }
        }

        match builder.build() {
            Ok(rt) => Box::into_raw(Box::new(OpaqueRuntime(rt))),
            Err(_) => core::ptr::null_mut(),
        }
    }))
    .unwrap_or(core::ptr::null_mut())
}

/// Destroys a runtime instance.
///
/// # Safety
/// `rt` must be a non-null pointer previously returned by `polyplug_runtime_create`.
/// Must not be called more than once for the same pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_destroy(rt: *mut OpaqueRuntime) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !rt.is_null() {
            // SAFETY: rt was allocated by polyplug_runtime_create via Box::new. Caller guarantees single call per pointer.
            drop(unsafe { Box::from_raw(rt) });
        }
    }))
    .unwrap_or(());
}

/// Loads a plugin bundle.
///
/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_create`.
/// `path` must point to `path_len` valid UTF-8 bytes for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_load_bundle(
    rt: *mut OpaqueRuntime,
    path: *const u8,
    path_len: usize,
) -> u32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return 1u32;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if path.is_null() {
            runtime
                .0
                .set_last_error("null path pointer in polyplug_runtime_load_bundle");
            return 1u32;
        }
        // SAFETY: path is non-null and points to path_len valid UTF-8 bytes per ABI contract.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
        let s: &str = match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                runtime.0.set_last_error(e.to_string());
                return 1u32;
            }
        };
        match runtime.0.load_bundle(std::path::Path::new(s)) {
            Ok(()) => 0u32,
            Err(e) => {
                runtime.0.set_last_error(e.to_string());
                1u32
            }
        }
    }))
    .unwrap_or(1u32)
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_create`.
/// `path` must point to `path_len` valid UTF-8 bytes for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_reload_bundle(
    rt: *mut OpaqueRuntime,
    path: *const u8,
    path_len: usize,
) -> u32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return 1u32;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if path.is_null() {
            runtime
                .0
                .set_last_error("null path pointer in polyplug_runtime_reload_bundle");
            return 1u32;
        }
        // SAFETY: path is non-null and points to path_len valid UTF-8 bytes per ABI contract.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
        let s: &str = match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                runtime.0.set_last_error(e.to_string());
                return 1u32;
            }
        };
        match runtime.0.reload_bundle(std::path::Path::new(s)) {
            Ok(()) => 0u32,
            Err(e) => {
                runtime.0.set_last_error(e.to_string());
                1u32
            }
        }
    }))
    .unwrap_or(1u32)
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_find_by_contract(
    rt: *const OpaqueRuntime,
    contract_id: u64,
    min_version: u32,
) -> u64 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return u64::MAX;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime.0.find_by_contract(contract_id, min_version) {
            Ok(h) => pack_handle(h),
            Err(_) => u64::MAX,
        }
    }))
    .unwrap_or(u64::MAX)
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_find_by_bundle(
    rt: *const OpaqueRuntime,
    bundle_id: u64,
    contract_id: u64,
    min_version: u32,
) -> u64 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return u64::MAX;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime
            .0
            .find_by_bundle(bundle_id, contract_id, min_version)
        {
            Ok(h) => pack_handle(h),
            Err(_) => u64::MAX,
        }
    }))
    .unwrap_or(u64::MAX)
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_create`.
/// `out` must be valid for writes of `out_cap` u64 elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_find_all_by_contract(
    rt: *const OpaqueRuntime,
    contract_id: u64,
    min_version: u32,
    out: *mut u64,
    out_cap: usize,
) -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return 0usize;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if out.is_null() && out_cap > 0 {
            runtime.0.set_last_error(
                "null output buffer with non-zero capacity in polyplug_runtime_find_all_by_contract",
            );
            return 0usize;
        }
        if out_cap == 0usize {
            return 0usize;
        }
        // SAFETY: out is valid for out_cap u64 elements per ABI contract.
        let out_slice: &mut [u64] = unsafe { core::slice::from_raw_parts_mut(out, out_cap) };
        runtime
            .0
            .find_all_by_contract_packed(contract_id, min_version, out_slice)
    }))
    .unwrap_or(0usize)
}

/// Resolve a plugin handle and return the vtable pointer directly.
///
/// # Safety
/// - `rt` must be a valid pointer returned by `polyplug_runtime_create`.
/// - The returned vtable pointer is valid as long as the runtime is alive and
///   no hot-reload occurs for this plugin.
///
/// # Returns
/// - Non-null pointer to `PluginInterface` on success
/// - Null on error (check `polyplug_runtime_last_error` for details)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_resolve_plugin(
    rt: *const OpaqueRuntime,
    packed_handle: u64,
) -> *const () {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return core::ptr::null();
        }
        const NULL_HANDLE: u64 = u64::MAX;
        if packed_handle == NULL_HANDLE {
            return core::ptr::null();
        }
        let handle: PluginHandle = unpack_handle(packed_handle);
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime.0.registry().resolve_guard(handle) {
            Ok(guard) => {
                // SAFETY: vtable pointer points to 'static plugin data that remains
                // valid for the lifetime of the loaded library.
                guard.vtable() as *const ()
            }
            Err(e) => {
                runtime.0.set_last_error(e.to_string());
                core::ptr::null()
            }
        }
    }))
    .unwrap_or(core::ptr::null())
}

/// # Safety
/// `buf` must be valid for writes of `buf_len` bytes when non-null.
/// Get the last error message from a runtime.
///
/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_create`.
/// `buf` must be valid for writes of `buf_len` bytes when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_last_error(
    rt: *const OpaqueRuntime,
    buf: *mut u8,
    buf_len: usize,
) -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            return 0;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };

        if buf.is_null() {
            let len = runtime.0.last_error_len();
            runtime.0.clear_last_error();
            return len;
        }
        if buf_len == 0 {
            runtime.0.clear_last_error();
            return 0;
        }
        // SAFETY: buf is valid for buf_len bytes per ABI contract.
        let buf_slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(buf, buf_len) };
        let len = runtime.0.get_last_error(buf_slice);
        runtime.0.clear_last_error();
        len
    }))
    .unwrap_or(0usize)
}

/// Get the length of the last error message from a runtime.
///
/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_error_message_len(rt: *const OpaqueRuntime) -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            // Return length of the null runtime error message
            return b"null runtime pointer".len();
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        runtime.0.last_error_len()
    }))
    .unwrap_or(0usize)
}

/// # Safety
/// `rt` must be a valid non-null pointer returned by `polyplug_runtime_create`.
/// `loader_ptr` must be a non-null `*mut c_void` produced by a loader cdylib's
/// `polyplug_*_loader_create()` function compiled against the same polyplug rlib.
/// This call transfers ownership — do not call `polyplug_*_loader_free()` after
/// a successful registration (return value 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_register_loader(
    rt: *mut OpaqueRuntime,
    loader_ptr: *mut std::ffi::c_void,
) -> u32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() || loader_ptr.is_null() {
            return 1u32;
        }
        // SAFETY: rt is a valid *mut OpaqueRuntime produced by polyplug_runtime_create per ABI contract.
        // loader_ptr is a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib compiled
        // against the same polyplug rlib. The double-box pattern preserves the fat pointer through
        // the c_void erasure. Reconstituting via Box::from_raw as *mut Box<dyn BundleLoader> is valid
        // because both sides agree on the layout via the shared rlib. Ownership is transferred here.
        let runtime: &mut Runtime = unsafe { &mut (*rt).0 };
        // SAFETY: loader_ptr is a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib
        // compiled against the same polyplug rlib. Reconstituting via Box::from_raw is valid.
        let loader: Box<dyn BundleLoader> =
            unsafe { *Box::from_raw(loader_ptr as *mut Box<dyn BundleLoader>) };
        match runtime.register_loader(loader) {
            Ok(()) => 0u32,
            Err(e) => {
                runtime.set_last_error(e.to_string());
                2u32
            }
        }
    }))
    .unwrap_or(1u32)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn test_runtime_new_and_free() {
        // SAFETY: polyplug_runtime_create has no pointer preconditions.
        let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
        assert!(!rt.is_null());
        // SAFETY: rt was returned by polyplug_runtime_create and is non-null.
        unsafe { polyplug_runtime_destroy(rt) };
    }

    #[test]
    fn handle_roundtrip_zero() {
        let h: PluginHandle = PluginHandle {
            index: 0u32,
            generation: 0u32,
        };
        let packed: u64 = pack_handle(h);
        let unpacked: PluginHandle = unpack_handle(packed);
        assert_eq!(unpacked.index, h.index);
        assert_eq!(unpacked.generation, h.generation);
    }

    #[test]
    fn handle_roundtrip_max_values() {
        // index = u32::MAX - 1 avoids the null sentinel (index == u32::MAX means null)
        let h: PluginHandle = PluginHandle {
            index: u32::MAX - 1,
            generation: u32::MAX,
        };
        let packed: u64 = pack_handle(h);
        let unpacked: PluginHandle = unpack_handle(packed);
        assert_eq!(unpacked.index, h.index);
        assert_eq!(unpacked.generation, h.generation);
    }

    #[test]
    fn handle_roundtrip_pack_unpack_identity() {
        // pack(unpack(x)) == x for boundary values
        let boundary_values: [u64; 4] = [
            0u64,
            1u64,
            (u32::MAX as u64) << 32 | (u32::MAX - 1) as u64,
            (1u64 << 32) | 1u64,
        ];
        for &val in &boundary_values {
            let unpacked: PluginHandle = unpack_handle(val);
            let repacked: u64 = pack_handle(unpacked);
            assert_eq!(repacked, val);
        }
    }

    #[test]
    fn handle_sentinel_null_roundtrip() {
        // u64::MAX is the sentinel for the null handle
        let packed: u64 = u64::MAX;
        let unpacked: PluginHandle = unpack_handle(packed);
        assert!(unpacked.is_null());
        let repacked: u64 = pack_handle(unpacked);
        assert_eq!(repacked, u64::MAX);
    }

    #[test]
    fn handle_null_packs_to_sentinel() {
        // The null PluginHandle (index == u32::MAX) must pack to u64::MAX
        let null_h: PluginHandle = PluginHandle::null();
        assert!(null_h.is_null());
        let packed: u64 = pack_handle(null_h);
        assert_eq!(packed, u64::MAX);
    }

    #[test]
    fn multiple_ffi_runtimes_are_isolated() {
        // Create two FFI runtimes - they should be completely independent
        let rt1: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
        let rt2: *mut OpaqueRuntime = unsafe { polyplug_runtime_create() };
        assert!(!rt1.is_null(), "first runtime must be created");
        assert!(!rt2.is_null(), "second runtime must be created");
        assert_ne!(rt1, rt2, "runtimes must be distinct pointers");
        // SAFETY: both are valid non-null pointers returned by polyplug_runtime_create
        unsafe {
            polyplug_runtime_destroy(rt1);
            polyplug_runtime_destroy(rt2);
        }
    }
}
