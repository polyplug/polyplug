use core::cell::Ref;
use core::cell::RefCell;

use crate::abi::PluginHandle;
use crate::loader::BundleLoader;
use crate::registry::PluginVTableGuard;
use crate::runtime::Runtime;

pub struct OpaqueRuntime(pub Runtime);
pub struct OpaqueGuard(pub(crate) PluginVTableGuard);

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
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

/// # Safety
/// Safe to call from any thread. No pointer arguments are required.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_new() -> *mut OpaqueRuntime {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match Runtime::builder().build() {
            Ok(rt) => Box::into_raw(Box::new(OpaqueRuntime(rt))),
            Err(e) => {
                set_last_error(e.to_string());
                core::ptr::null_mut()
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_new");
        core::ptr::null_mut()
    })
}

/// # Safety
/// `rt` must be a non-null pointer previously returned by `polyplug_runtime_new`.
/// Must not be called more than once for the same pointer.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_runtime_free(rt: *mut OpaqueRuntime) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !rt.is_null() {
            // SAFETY: rt was allocated by polyplug_runtime_new via Box::new. Caller guarantees single call per pointer.
            drop(unsafe { Box::from_raw(rt) });
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_runtime_free");
    });
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_new`.
/// `path` must point to `path_len` valid UTF-8 bytes for the duration of the call.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_load_bundle(
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
            set_last_error("null path pointer in polyplug_load_bundle");
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
        set_last_error("panic in polyplug_load_bundle");
        1u32
    })
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_new`.
/// `path` must point to `path_len` valid UTF-8 bytes for the duration of the call.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_reload_bundle(
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
            set_last_error("null path pointer in polyplug_reload_bundle");
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
        set_last_error("panic in polyplug_reload_bundle");
        1u32
    })
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_new`.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_rt_find_by_contract(
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
        set_last_error("panic in polyplug_rt_find_by_contract");
        u64::MAX
    })
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_new`.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_rt_find_by_bundle(
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
        set_last_error("panic in polyplug_rt_find_by_bundle");
        u64::MAX
    })
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_new`.
/// `out` must be valid for writes of `out_cap` u64 elements.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_rt_find_all_by_contract(
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
                "null output buffer with non-zero capacity in polyplug_rt_find_all_by_contract",
            );
            return 0usize;
        }
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        if out_cap == 0usize {
            return 0usize;
        }
        let mut handle_buf: [PluginHandle; 16] = [PluginHandle {
            index: 0u32,
            generation: 0u32,
        }; 16];
        let mut total_written: usize = 0usize;
        loop {
            let remaining: usize = out_cap - total_written;
            if remaining == 0usize {
                break;
            }
            let write_cap: usize = if remaining < handle_buf.len() {
                remaining
            } else {
                handle_buf.len()
            };
            let count: usize = runtime.0.find_all_by_contract(
                contract_id,
                min_version,
                &mut handle_buf[..write_cap],
            );
            for (offset, handle) in handle_buf[..count].iter().enumerate() {
                // SAFETY: out is valid for out_cap u64 elements per ABI contract.
                unsafe {
                    out.add(total_written + offset).write(pack_handle(*handle));
                }
            }
            total_written += count;
            if count < write_cap {
                break;
            }
        }
        total_written
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_rt_find_all_by_contract");
        0usize
    })
}

/// # Safety
/// `rt` must be a valid pointer returned by `polyplug_runtime_new`.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_rt_resolve_plugin(
    rt: *const OpaqueRuntime,
    packed_handle: u64,
) -> *mut OpaqueGuard {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if rt.is_null() {
            set_last_error("null runtime");
            return core::ptr::null_mut();
        }
        const NULL_HANDLE: u64 = u64::MAX;
        if packed_handle == NULL_HANDLE {
            // Null handle — return null guard without setting last_error.
            // Callers that receive NULL_HANDLE back from find functions use this as a sentinel.
            return core::ptr::null_mut();
        }
        let handle: PluginHandle = unpack_handle(packed_handle);
        // SAFETY: rt is non-null valid OpaqueRuntime per ABI contract.
        let runtime: &OpaqueRuntime = unsafe { &*rt };
        match runtime.0.registry().resolve_guard(handle) {
            Ok(guard) => Box::into_raw(Box::new(OpaqueGuard(guard))),
            Err(e) => {
                set_last_error(e.to_string());
                core::ptr::null_mut()
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_rt_resolve_plugin");
        core::ptr::null_mut()
    })
}

/// # Safety
/// `guard` must be a non-null pointer previously returned by `polyplug_rt_resolve_plugin`.
/// Must not be called more than once for the same pointer.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_guard_free(guard: *mut OpaqueGuard) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !guard.is_null() {
            // SAFETY: guard was allocated by polyplug_rt_resolve_plugin via Box::new. Caller guarantees single call per pointer.
            drop(unsafe { Box::from_raw(guard) });
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_guard_free");
    });
}

/// # Safety
/// `guard` must be a valid pointer returned by `polyplug_rt_resolve_plugin`.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_get_vtable(guard: *const OpaqueGuard) -> *const () {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if guard.is_null() {
            set_last_error("null guard");
            return core::ptr::null();
        }
        // SAFETY: guard is non-null valid OpaqueGuard per ABI contract.
        unsafe { (*guard).0.vtable() as *const () }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_get_vtable");
        core::ptr::null()
    })
}

/// # Safety
/// `buf` must be valid for writes of `buf_len` bytes when non-null.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_last_error(buf: *mut u8, buf_len: usize) -> usize {
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
        set_last_error("panic in polyplug_last_error");
        0usize
    })
}

/// # Safety
/// Safe to call from any thread. No pointer arguments are required.
#[allow(clippy::std_instead_of_core)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_error_message_len() -> usize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        LAST_ERROR.with(|e| e.borrow().len())
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic in polyplug_error_message_len");
        0usize
    })
}

/// # Safety
/// `rt` must be a valid non-null pointer returned by `polyplug_runtime_new`.
/// `loader_ptr` must be a non-null `*mut c_void` produced by a loader cdylib's
/// `polyplug_*_loader_create()` function compiled against the same polyplug rlib.
/// This call transfers ownership — do not call `polyplug_*_loader_free()` after
/// a successful registration (return value 0).
#[allow(clippy::std_instead_of_core)]
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
        // SAFETY: rt is a valid *mut OpaqueRuntime produced by polyplug_runtime_new per ABI contract.
        // loader_ptr is a *mut Box<dyn BundleLoader> erased to *mut c_void by a loader cdylib compiled
        // SAFETY: rt is a valid *mut OpaqueRuntime produced by polyplug_runtime_new per ABI contract.
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
    use super::*;

    #[test]
    fn test_runtime_new_and_free() {
        let rt: *mut OpaqueRuntime = unsafe { polyplug_runtime_new() };
        assert!(!rt.is_null());
        unsafe { polyplug_runtime_free(rt) };
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
