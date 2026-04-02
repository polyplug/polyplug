
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

/// Returns the buffer contents as a byte slice.
///
/// # Safety
/// Caller must ensure `buf.ptr` is valid for `buf.len` bytes and the memory is live.
pub unsafe fn buffer_as_slice(buf: &Buffer) -> &[u8] {
    // SAFETY: Caller guarantees ptr is non-null and valid for len bytes.
    unsafe { core::slice::from_raw_parts(buf.ptr, buf.len) }
}

/// Returns the buffer contents as a mutable byte slice.
///
/// # Safety
/// Caller must ensure `buf.ptr` is valid for `buf.cap` bytes, the memory is live, and no
/// other reference to the buffer exists.
pub unsafe fn buffer_as_mut_slice(buf: &mut Buffer) -> &mut [u8] {
    // SAFETY: Caller guarantees ptr is non-null, valid for cap bytes, and exclusively owned.
    unsafe { core::slice::from_raw_parts_mut(buf.ptr, buf.cap) }
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
}
