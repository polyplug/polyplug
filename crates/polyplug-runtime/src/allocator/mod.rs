//! Allocator — host_alloc / host_free cross-boundary memory management.
//!
//! These functions are exported with C linkage and used by all plugins
//! to allocate memory that crosses the plugin/host boundary.

use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use std::alloc::System;

/// Allocate memory via the host system allocator.
///
/// Returns null for size=0 or invalid alignment.
///
/// # Safety
/// Callers must:
/// - Free the returned pointer with `polyplug_host_free` using the SAME `size` and `align`.
/// - Not use the returned pointer after calling `polyplug_host_free`.
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_host_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    let layout: Layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };
    // SAFETY: layout is non-zero size and power-of-two alignment, validated above.
    // Caller is generated code that always passes correct alignment for the type.
    // System allocator is thread-safe on all supported platforms.
    unsafe { System.alloc(layout) }
}

/// Free memory previously allocated by `polyplug_host_alloc`.
///
/// Passing null or size=0 is a safe no-op.
///
/// # Safety
/// Callers must:
/// - Pass a pointer returned by `polyplug_host_alloc`.
/// - Pass the SAME `size` and `align` used in the original allocation.
/// - Not use the pointer after this call.
/// - Not call this twice with the same pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_host_free(ptr: *mut u8, size: usize, align: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    let layout: Layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => {
            // Invalid layout — cannot safely free. Intentional leak to avoid UB.
            return;
        }
    };
    // SAFETY: ptr was allocated by polyplug_host_alloc with this exact layout.
    // The caller guarantees size and align match the original allocation.
    // System allocator is thread-safe on all supported platforms.
    unsafe { System.dealloc(ptr, layout) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_free_basic() {
        let ptr: *mut u8 = polyplug_host_alloc(64, 8);
        assert!(!ptr.is_null());
        // SAFETY: ptr is a valid 64-byte, align-8 allocation just returned by polyplug_host_alloc.
        unsafe {
            std::ptr::write(ptr, 0xAB_u8);
            assert_eq!(std::ptr::read(ptr), 0xAB_u8);
            polyplug_host_free(ptr, 64, 8);
        }
    }

    #[test]
    fn alloc_zero_size_returns_null() {
        let ptr: *mut u8 = polyplug_host_alloc(0, 8);
        assert!(ptr.is_null());
    }

    #[test]
    fn free_null_is_noop() {
        // Must not panic or crash
        // SAFETY: null is a documented no-op in polyplug_host_free.
        unsafe {
            polyplug_host_free(core::ptr::null_mut(), 0, 1);
        }
    }

    #[test]
    fn alloc_invalid_align_returns_null() {
        // align=3 is not a power of two — Layout::from_size_align will reject it
        let ptr: *mut u8 = polyplug_host_alloc(64, 3);
        assert!(ptr.is_null());
    }
}
