//! FFI — public `#[no_mangle]` C ABI entry points for host language bindings.
//!
//! All functions use `catch_unwind` to prevent Rust panics from unwinding across
//! the C ABI boundary. Errors are stored in a thread-local `LAST_ERROR` string.

#![allow(clippy::std_instead_of_core)]

use core::cell::Ref;
use core::cell::RefCell;
use core::sync::atomic::AtomicPtr;
use core::sync::atomic::Ordering;
use std::sync::OnceLock;

use crate::loader::BundleLoader;
use crate::reload::ReloadPhase;
use crate::runtime::Runtime;
use crate::runtime::RuntimeConfig;
use polyplug_abi::PluginHandle;

pub struct OpaqueRuntime(pub Runtime);

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

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
/// This struct is passed to `polyplug_runtime_set_config` to configure
/// the runtime before creation.
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

// ─── Global storage for pre-build configuration ───────────────────────────────

/// Type alias for the C reload callback function pointer.
type ReloadCallbackC = extern "C" fn(ReloadPhaseC);

/// Global storage for the reload callback set via `polyplug_runtime_on_reload`.
///
/// Uses `AtomicPtr` to store the function pointer. A null pointer indicates
/// no callback has been registered.
static GLOBAL_RELOAD_CB: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Global storage for the runtime config set via `polyplug_runtime_set_config`.
static GLOBAL_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

/// Wrapper that converts the C callback to a Rust callback.
///
/// This function is called by the runtime when a reload phase changes.
/// It reads the stored C callback and invokes it with the C-compatible struct.
fn invoke_reload_callback(phase: ReloadPhase) {
    let cb_ptr: *mut () = GLOBAL_RELOAD_CB.load(Ordering::Relaxed);
    if cb_ptr.is_null() {
        return;
    }
    // SAFETY: cb_ptr was stored by polyplug_runtime_on_reload from a valid
    // extern "C" function pointer. The pointer remains valid for the process
    // lifetime (function pointers are 'static). The transmute is safe because
    // we only store function pointers of the correct type.
    let cb: ReloadCallbackC = unsafe { core::mem::transmute(cb_ptr) };
    let phase_c: ReloadPhaseC = ReloadPhaseC::from_reload_phase(&phase);
    cb(phase_c);
}

pub fn set_last_error_pub(msg: &str) {
    set_last_error(msg);
}

fn set_last_error(msg: impl Into<String>) {
    LAST_ERROR.with(|e| *e.borrow_mut() = msg.into());
}

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

/// Creates a new runtime instance.
///
/// Applies any configuration set via `polyplug_runtime_set_config` and any
/// reload callback registered via `polyplug_runtime_on_reload`.
///
/// # Safety
/// Safe to call from any thread. No pointer arguments are required.
/// Returns null on allocation failure or panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_create() -> *mut OpaqueRuntime {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut builder = Runtime::builder();

        if let Some(config) = GLOBAL_CONFIG.get() {
            builder = builder.config(config.clone());
        }

        let cb_ptr: *mut () = GLOBAL_RELOAD_CB.load(Ordering::Relaxed);
        if !cb_ptr.is_null() {
            builder = builder.on_reload(invoke_reload_callback);
        }

        match builder.build() {
            Ok(rt) => Box::into_raw(Box::new(OpaqueRuntime(rt))),
            Err(e) => {
                set_last_error(e.to_string());
                core::ptr::null_mut()
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_create");
        core::ptr::null_mut()
    })
}

/// Register a callback to be invoked during hot-reload operations.
///
/// The callback is invoked at each phase of a hot-reload:
/// - `Preparing`: Before vtable swap, includes retry count
/// - `Reloaded`: After successful vtable swap
/// - `Failed`: When reload fails, includes reason string
///
/// The callback is applied to all subsequently created runtimes.
/// Call before `polyplug_runtime_create`.
///
/// # Safety
/// `callback` must be a valid function pointer with C calling convention.
/// The callback must not panic. The callback receives borrowed string
/// pointers that are valid only for the duration of the call.
///
/// # Returns
/// 0 on success, non-zero on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_on_reload(callback: extern "C" fn(ReloadPhaseC)) -> u32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ptr: *mut () = callback as *mut ();
        GLOBAL_RELOAD_CB.store(ptr, Ordering::Relaxed);
        0u32
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_on_reload");
        1u32
    })
}

/// Set runtime configuration for subsequently created runtimes.
///
/// Must be called before `polyplug_runtime_create`. The configuration
/// is applied to all subsequently created runtimes.
///
/// # Safety
/// `config` must be a valid pointer to a `RuntimeConfigC` struct.
///
/// # Returns
/// 0 on success, non-zero on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_set_config(config: *const RuntimeConfigC) -> u32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if config.is_null() {
            set_last_error("null config pointer in polyplug_runtime_set_config");
            return 1u32;
        }
        // SAFETY: config is non-null and points to a valid RuntimeConfigC per ABI contract.
        let config_c: RuntimeConfigC = unsafe { *config };
        let runtime_config: RuntimeConfig = config_c.into_runtime_config();
        // OnceLock::set returns Err when already set — we allow overwriting by ignoring the result.
        // This matches the pattern used for GLOBAL_WARNING_CB in runtime.rs.
        let _: Result<(), RuntimeConfig> = GLOBAL_CONFIG.set(runtime_config);
        0u32
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_set_config");
        1u32
    })
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
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_destroy");
    });
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
            set_last_error("null runtime");
            return 1u32;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if path.is_null() {
            set_last_error("null path pointer in polyplug_runtime_load_bundle");
            return 1u32;
        }
        // SAFETY: path is non-null and points to path_len valid UTF-8 bytes per ABI contract.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
        let s: &str = match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(e.to_string());
                return 1u32;
            }
        };
        match runtime.0.load_bundle(std::path::Path::new(s)) {
            Ok(()) => 0u32,
            Err(e) => {
                set_last_error(e.to_string());
                1u32
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_load_bundle");
        1u32
    })
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
            set_last_error("null runtime");
            return 1u32;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if path.is_null() {
            set_last_error("null path pointer in polyplug_runtime_reload_bundle");
            return 1u32;
        }
        // SAFETY: path is non-null and points to path_len valid UTF-8 bytes per ABI contract.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(path, path_len) };
        let s: &str = match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(e.to_string());
                return 1u32;
            }
        };
        match runtime.0.reload_bundle(std::path::Path::new(s)) {
            Ok(()) => 0u32,
            Err(e) => {
                set_last_error(e.to_string());
                1u32
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_reload_bundle");
        1u32
    })
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
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_find_by_contract");
        u64::MAX
    })
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
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_find_by_bundle");
        u64::MAX
    })
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
        if out.is_null() && out_cap > 0 {
            set_last_error(
                "null output buffer with non-zero capacity in polyplug_runtime_find_all_by_contract",
            );
            return 0usize;
        }
        if out_cap == 0usize {
            return 0usize;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        // SAFETY: out is valid for out_cap u64 elements per ABI contract.
        let out_slice: &mut [u64] = unsafe { core::slice::from_raw_parts_mut(out, out_cap) };
        runtime
            .0
            .find_all_by_contract_packed(contract_id, min_version, out_slice)
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_find_all_by_contract");
        0usize
    })
}

/// Resolve a plugin handle and return the vtable pointer directly.
///
/// # Safety
/// - `rt` must be a valid pointer returned by `polyplug_runtime_create`.
/// - The returned vtable pointer is valid as long as the runtime is alive and
///   no hot-reload occurs for this plugin.
///
/// # Returns
/// - Non-null pointer to `PluginVTable` on success
/// - Null on error (check `polyplug_runtime_last_error` for details)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_resolve_plugin(
    rt: *const OpaqueRuntime,
    packed_handle: u64,
) -> *const () {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            set_last_error("null runtime");
            return core::ptr::null();
        }
        const NULL_HANDLE: u64 = u64::MAX;
        if packed_handle == NULL_HANDLE {
            // Null handle — return null without setting last_error.
            return core::ptr::null();
        }
        let handle: PluginHandle = unpack_handle(packed_handle);
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime.0.registry().resolve_guard(handle) {
            Ok(guard) => {
                // Get the vtable pointer from the guard.
                // The guard is dropped here, but the vtable pointer remains valid
                // because it points to 'static data in the loaded library.
                // SAFETY: vtable pointer points to 'static plugin data that remains
                // valid for the lifetime of the loaded library.
                guard.vtable() as *const ()
            }
            Err(e) => {
                set_last_error(e.to_string());
                core::ptr::null()
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_resolve_plugin");
        core::ptr::null()
    })
}

/// # Safety
/// `buf` must be valid for writes of `buf_len` bytes when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_last_error(buf: *mut u8, buf_len: usize) -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let len: usize = LAST_ERROR.with(|e| {
            let msg: Ref<'_, String> = e.borrow();
            let bytes: &[u8] = msg.as_bytes();
            let write_n: usize = bytes.len().min(buf_len);
            if !buf.is_null() && write_n > 0 {
                // SAFETY: buf is valid for buf_len bytes per ABI contract.
                unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, write_n) };
            }
            write_n
        });
        LAST_ERROR.with(|e| e.borrow_mut().clear());
        len
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_last_error");
        0usize
    })
}

/// # Safety
/// Safe to call from any thread. No pointer arguments are required.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_error_message_len() -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        LAST_ERROR.with(|e| e.borrow().len())
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_error_message_len");
        0usize
    })
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
            set_last_error("null argument in polyplug_runtime_register_loader");
            return 1u32;
        }
        // SAFETY: rt is a valid *mut OpaqueRuntime produced by polyplug_runtime_create per ABI contract.
        // loader_ptr is a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib compiled
        // SAFETY: rt is a valid *mut OpaqueRuntime produced by polyplug_runtime_create per ABI contract.
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
                set_last_error(e.to_string());
                2u32
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_register_loader");
        1u32
    })
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
}
