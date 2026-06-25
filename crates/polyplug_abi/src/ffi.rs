//! Allocator — host_alloc/host_free cross-boundary memory management.
//! These functions back the `HostApi::alloc`/`HostApi::free` fields
//! and are used to allocate memory that crosses the plugin/host boundary.
//! They are NOT exported as standalone C symbols — the only exported symbols are
//! `polyplug_runtime_create` and `polyplug_runtime_destroy`.

use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::ptr;
use std::alloc::System;

/// Allocate memory via the host system allocator.
///
/// Returns null for size=0 or invalid alignment. Calling this function is itself
/// safe; the obligations below concern the returned pointer's lifecycle.
///
/// # Contract
/// To avoid leaks or undefined behaviour with the returned pointer, callers must:
/// - Free it with `polyplug_host_free` using the SAME `size` and `align`.
/// - Not use it after calling `polyplug_host_free`.
pub extern "C" fn polyplug_host_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }
    let layout: Layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return ptr::null_mut(),
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
