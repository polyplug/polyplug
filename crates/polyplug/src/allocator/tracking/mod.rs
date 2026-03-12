#![cfg_attr(debug_assertions, allow(clippy::std_instead_of_core))]
//! Tracking allocator — wraps `polyplug_host_alloc`/`polyplug_host_free` and counts
//! allocation/deallocation calls for test-time leak detection.
//!
//! This module does NOT implement `GlobalAlloc` or `std::alloc::Allocator`.
//! It is purely a test-support shim that forwards to the ABI allocator and counts calls.

#[cfg(debug_assertions)]
use core::cell::RefCell;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
#[cfg(debug_assertions)]
use std::collections::HashSet;

use crate::allocator::polyplug_host_alloc;
use crate::allocator::polyplug_host_free;

thread_local! {
    static TLS_ALLOC_COUNT: AtomicUsize = const { AtomicUsize::new(0) };
    static TLS_FREE_COUNT: AtomicUsize = const { AtomicUsize::new(0) };
}

#[cfg(debug_assertions)]
thread_local! {
    static TLS_LIVE_ADDRS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Standalone `extern "C"` function that increments the thread-local alloc counter
/// and forwards to `polyplug_host_alloc`.
///
/// # Safety
/// `size` must be non-zero. `align` must be a power of two.
/// The returned pointer must be freed with `tracking_free` using identical `size` and `align`.
unsafe extern "C" fn tracking_alloc(size: usize, align: usize) -> *mut u8 {
    TLS_ALLOC_COUNT.with(|c| c.fetch_add(1, Ordering::SeqCst));
    // SAFETY: Caller guarantees size > 0 and align is a power of two.
    // Forwarding directly to the ABI allocator with unchanged parameters.
    let ptr: *mut u8 = polyplug_host_alloc(size, align);
    #[cfg(debug_assertions)]
    if !ptr.is_null() {
        TLS_LIVE_ADDRS.with(|s| {
            s.borrow_mut().insert(ptr as usize);
        });
    }
    ptr
}

/// Standalone `extern "C"` function that increments the thread-local free counter
/// and forwards to `polyplug_host_free`.
///
/// # Safety
/// `ptr` must have been returned by `tracking_alloc` (and thus by `polyplug_host_alloc`)
/// with the same `size` and `align`. Must not be called twice for the same pointer.
unsafe extern "C" fn tracking_free(ptr: *mut u8, size: usize, align: usize) {
    TLS_FREE_COUNT.with(|c| c.fetch_add(1, Ordering::SeqCst));
    #[cfg(debug_assertions)]
    {
        let addr: usize = ptr as usize;
        TLS_LIVE_ADDRS.with(|s| {
            if !s.borrow_mut().remove(&addr) {
                eprintln!(
                    "TrackingAllocator: double-free detected at address {:#x}",
                    addr
                );
                std::process::abort();
            }
        });
    }
    // SAFETY: ptr was allocated by polyplug_host_alloc via tracking_alloc with this layout.
    // Caller guarantees size and align match the original allocation.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// Zero-sized tracking wrapper around `polyplug_host_alloc` / `polyplug_host_free`.
///
/// Uses thread-local counters so that `alloc_fn` / `free_fn` can be `unsafe extern "C"` fn
/// pointers (which cannot capture state). Each call to `new()` resets the counters on the
/// current thread.
pub struct TrackingAllocator;

impl TrackingAllocator {
    /// Create a new `TrackingAllocator`, resetting the thread-local counters to zero.
    pub fn new() -> TrackingAllocator {
        TLS_ALLOC_COUNT.with(|c| c.store(0, Ordering::SeqCst));
        TLS_FREE_COUNT.with(|c| c.store(0, Ordering::SeqCst));
        #[cfg(debug_assertions)]
        TLS_LIVE_ADDRS.with(|s| s.borrow_mut().clear());
        TrackingAllocator
    }

    /// Returns a function pointer suitable for `HostVTable.alloc`.
    ///
    /// The returned fn increments the thread-local alloc counter on each call.
    pub fn alloc_fn(&self) -> unsafe extern "C" fn(usize, usize) -> *mut u8 {
        tracking_alloc
    }

    /// Returns a function pointer suitable for `HostVTable.free`.
    ///
    /// The returned fn increments the thread-local free counter on each call.
    pub fn free_fn(&self) -> unsafe extern "C" fn(*mut u8, usize, usize) {
        tracking_free
    }

    /// Returns the number of allocations made on this thread since the last `new()`.
    pub fn alloc_count(&self) -> usize {
        TLS_ALLOC_COUNT.with(|c| c.load(Ordering::SeqCst))
    }

    /// Returns the number of frees made on this thread since the last `new()`.
    pub fn free_count(&self) -> usize {
        TLS_FREE_COUNT.with(|c| c.load(Ordering::SeqCst))
    }

    /// Panics with a descriptive message if `alloc_count() != free_count()`.
    pub fn assert_no_leaks(&self) {
        let a: usize = self.alloc_count();
        let f: usize = self.free_count();
        if a != f {
            panic!(
                "TrackingAllocator: leak detected: {} allocs, {} frees",
                a, f
            );
        }
    }
}

impl Default for TrackingAllocator {
    fn default() -> TrackingAllocator {
        TrackingAllocator::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_allocator_counts_alloc_and_free() {
        let tracker: TrackingAllocator = TrackingAllocator::new();
        let alloc: unsafe extern "C" fn(usize, usize) -> *mut u8 = tracker.alloc_fn();
        let free: unsafe extern "C" fn(*mut u8, usize, usize) = tracker.free_fn();
        // SAFETY: alloc/free are valid function pointers wrapping polyplug_host_alloc/free.
        // size=64, align=1 produce a valid layout.
        let ptr: *mut u8 = unsafe { alloc(64, 1) };
        assert!(!ptr.is_null());
        assert_eq!(tracker.alloc_count(), 1);
        // SAFETY: ptr was just allocated with size=64, align=1 via tracking_alloc.
        unsafe { free(ptr, 64, 1) };
        assert_eq!(tracker.free_count(), 1);
        tracker.assert_no_leaks();
    }

    #[test]
    #[should_panic]
    fn assert_no_leaks_panics_on_mismatch() {
        let tracker: TrackingAllocator = TrackingAllocator::new();
        let alloc: unsafe extern "C" fn(usize, usize) -> *mut u8 = tracker.alloc_fn();
        // Allocate but do NOT free
        // SAFETY: size=64, align=1 is a valid layout. We intentionally leak for the panic test.
        let _ptr: *mut u8 = unsafe { alloc(64, 1) };
        tracker.assert_no_leaks(); // should panic
    }
}
