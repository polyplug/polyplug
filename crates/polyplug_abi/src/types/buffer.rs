/// Owning byte buffer.
///
/// OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
/// Owner calls `polyplug_host_free(ptr, cap, align)` when done.
#[repr(C)]
#[derive(Debug)]
pub struct Buffer {
    pub ptr: *mut u8,
    /// Bytes currently used.
    pub len: usize,
    /// Bytes allocated.
    pub cap: usize,
}

// SAFETY: Buffer owns its heap-allocated data through the host allocator.
// Sending between threads is safe because the host allocator is thread-safe.
unsafe impl Send for Buffer {}

impl Buffer {
    /// Returns the buffer contents as a byte slice.
    ///
    /// A null pointer or zero length yields an empty slice without dereferencing the
    /// pointer.
    ///
    /// # Safety
    /// For a non-null, non-empty buffer the caller must guarantee `ptr` is valid for
    /// `len` bytes and the memory is live for the borrow.
    pub unsafe fn as_slice(&self) -> &[u8] {
        if self.ptr.is_null() || self.len == 0 {
            return &[];
        }
        // SAFETY: ptr is non-null and the caller guarantees `len` live bytes.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Returns the buffer contents as a mutable byte slice.
    ///
    /// A null pointer or zero capacity yields an empty slice without dereferencing the
    /// pointer.
    ///
    /// # Safety
    /// For a non-null, non-empty buffer the caller must guarantee `ptr` is valid for
    /// `cap` bytes, the memory is live, and no other reference to the buffer exists.
    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        if self.ptr.is_null() || self.cap == 0 {
            return &mut [];
        }
        // SAFETY: ptr is non-null and the caller guarantees `cap` live, exclusively
        // owned bytes.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.cap) }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::types::buffer::Buffer;

    #[test]
    fn layout_buffer() {
        assert_eq!(size_of::<Buffer>(), 24);
        assert_eq!(align_of::<Buffer>(), 8);
        assert_eq!(offset_of!(Buffer, ptr), 0);
        assert_eq!(offset_of!(Buffer, len), 8);
        assert_eq!(offset_of!(Buffer, cap), 16);
    }

    #[test]
    fn as_slice_on_null_returns_empty() {
        let buf: Buffer = Buffer {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
        // SAFETY: null buffer never dereferences the pointer; defined to yield empty.
        let s: &[u8] = unsafe { buf.as_slice() };
        assert!(s.is_empty());
    }

    #[test]
    fn as_mut_slice_on_null_returns_empty() {
        let mut buf: Buffer = Buffer {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
        // SAFETY: null buffer never dereferences the pointer; defined to yield empty.
        let s: &mut [u8] = unsafe { buf.as_mut_slice() };
        assert!(s.is_empty());
    }
}
