//! FFI-safe array with caller-frees ownership model.
//!
//! Allocated via `host->alloc(self, len * sizeof(T), align)`.
//! Freed via `host->free(self, items, len * sizeof(T), align)`.
//!
//! # OWNERSHIP
//! Caller owns the memory and must free via host allocator.
//! CodeGen generates RAII wrappers in each language SDK.

use core::marker::PhantomData;

/// FFI-safe array with caller-frees ownership model.
///
/// Allocated via `host->alloc(self, len * sizeof(T), align)`.
/// Freed via `host->free(self, items, len * sizeof(T), align)`.
#[repr(C)]
pub struct Array<T: Sized> {
    /// Pointer to elements, allocated via host allocator.
    pub items: *mut T,
    /// Number of elements.
    pub len: usize,
    /// Alignment of T, for proper freeing.
    pub align: usize,
    /// Marker to track generic type.
    _marker: PhantomData<T>,
}

impl<T: Sized> Array<T> {
    /// Create an empty array.
    pub const fn empty() -> Self {
        Self {
            items: core::ptr::null_mut(),
            len: 0,
            align: core::mem::align_of::<T>(),
            _marker: PhantomData,
        }
    }

    /// Create a new array from pointer, length, and alignment.
    pub fn new(items: *mut T, len: usize) -> Self {
        Self {
            items,
            len,
            align: core::mem::align_of::<T>(),
            _marker: PhantomData,
        }
    }

    /// Check if this is an empty array.
    pub fn is_empty(&self) -> bool {
        self.items.is_null() || self.len == 0
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
    use core::mem::{align_of, offset_of, size_of};
    use super::Array;

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