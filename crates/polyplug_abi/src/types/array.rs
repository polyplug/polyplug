//! FFI-safe array with caller-frees ownership model.
//!
//! This module defines `Array<T>`, the FFI-safe array type used for
//! returning collections from runtime functions.
//!
//! # Memory Management
//! Allocated via `host->alloc(self, len * sizeof(T), align)`.
//! Freed via `host->free(self, items, len * sizeof(T), align)`.
//!
//! # Ownership
//! Caller owns the memory and must free via host allocator.
//! CodeGen generates RAII wrappers in each language SDK:
//! - Rust: `Drop` impl calls `host->free`
//! - Python: `__del__` calls free
//! - C#: `IDisposable.Dispose` calls free
//!
//! # Safety
//! The `align` field is required for proper freeing. Generic code must
//! track alignment of `T` to free correctly.

use core::fmt;
use core::marker::PhantomData;
use core::mem;
use core::ptr;

/// FFI-safe array with caller-frees ownership model.
///
/// # Memory Management
/// - Allocated via `host->alloc(self, len * sizeof(T), align)`
/// - Freed via `host->free(self, items, len * sizeof(T), align)`
///
/// # Ownership
/// Caller owns the memory and must free via host allocator.
/// CodeGen generates RAII wrappers in each language SDK:
/// - Rust: `Drop` impl calls `host->free`
/// - Python: `__del__` calls free
/// - C#: `IDisposable.Dispose` calls free
///
/// # Safety
/// The `align` field is required for proper freeing. Generic code must
/// track alignment of `T` to free correctly.
///
/// # Thread Safety
/// Safe to read from multiple threads if underlying data is immutable.
/// Send/Sync implemented for T: Send/Sync.
#[repr(C)]
pub struct Array<T: Sized> {
    /// Pointer to elements, allocated via host allocator.
    ///
    /// # Ownership
    /// Caller owns. Must be freed via `host->free` with same size/align.
    pub items: *mut T,
    /// Number of elements.
    ///
    /// Used to calculate total size for freeing: `len * sizeof(T)`.
    pub len: usize,
    /// Alignment of T, for proper freeing.
    ///
    /// # Purpose
    /// Required because generic code may not know T's alignment at runtime.
    /// Must match `align_of::<T>()` used during allocation.
    pub align: usize,
    /// Marker to track generic type.
    _marker: PhantomData<T>,
}

impl<T: Sized> Array<T> {
    /// Create an empty array.
    pub const fn empty() -> Self {
        Self {
            items: ptr::null_mut(),
            len: 0,
            align: mem::align_of::<T>(),
            _marker: PhantomData,
        }
    }

    /// Create a new array from pointer, length, and alignment.
    pub fn new(items: *mut T, len: usize) -> Self {
        Self {
            items,
            len,
            align: mem::align_of::<T>(),
            _marker: PhantomData,
        }
    }

    /// Check if this is an empty array.
    pub fn is_empty(&self) -> bool {
        self.items.is_null() || self.len == 0
    }
}

// `Array<T>` is bitwise-copyable for ANY `T`: every field — the `*mut T`
// pointer, the two `usize` size fields, and the zero-sized `PhantomData<T>` — is
// `Copy` regardless of whether `T` itself is `Copy`. The manual impls below
// therefore carry NO `T: Copy` bound (the derive would have added one). Copying
// an `Array<T>` duplicates the pointer/len/align triple only; it does NOT
// duplicate the pointed-to buffer, so the caller-frees ownership contract is
// unchanged (exactly one logical owner must free, as before).
impl<T: Sized> Clone for Array<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Sized> Copy for Array<T> {}

// Manual `Debug` (no `T: Debug` bound): the array does not own a borrow it may
// safely dereference here, so it prints only the FFI fields (pointer, length,
// alignment) — never the pointed-to elements.
impl<T: Sized> fmt::Debug for Array<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Array")
            .field("items", &self.items)
            .field("len", &self.len)
            .field("align", &self.align)
            .finish()
    }
}

// SAFETY: Array<T> contains a raw pointer and size fields.
// The pointer ownership is documented (caller-frees).
unsafe impl<T: Sized + Send> Send for Array<T> {}

// SAFETY: Array<T> contains a raw pointer and size fields.
// Concurrent reads are safe if the underlying data is immutable.
unsafe impl<T: Sized + Sync> Sync for Array<T> {}

#[cfg(test)]
mod tests {
    use super::Array;
    use core::mem::{align_of, offset_of, size_of};

    #[test]
    fn layout_array() {
        // Array<T>: pointer (8) + len (8) + align (8) + PhantomData (0) = 24 bytes
        assert_eq!(size_of::<Array<u64>>(), 24);
        assert_eq!(align_of::<Array<u64>>(), 8);
        assert_eq!(offset_of!(Array<u64>, items), 0);
        assert_eq!(offset_of!(Array<u64>, len), 8);
        assert_eq!(offset_of!(Array<u64>, align), 16);
    }

    #[test]
    fn empty_array() {
        let arr: Array<u64> = Array::empty();
        assert!(arr.is_empty());
        assert_eq!(arr.len, 0);
        assert!(arr.items.is_null());
    }
}
