/// Non-owning UTF-8 string view.
///
/// OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
/// of the call. Never freed by the receiver.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StringView {
    /// UTF-8 bytes, NOT null-terminated.
    pub ptr: *const u8,
    /// Byte count.
    pub len: usize,
}

// SAFETY: StringView is a read-only view into externally-owned data.
// The data pointed to is either 'static or valid for the lifetime of the call.
// Using StringView from multiple threads concurrently only reads the pointer —
// no mutation occurs. The caller guarantees the pointed-to data remains valid.
unsafe impl Send for StringView {}

// SAFETY: Same reasoning as Send — concurrent reads are safe.
unsafe impl Sync for StringView {}

impl StringView {
    /// Construct a StringView from a static byte slice.
    pub const fn from_static(bytes: &'static [u8]) -> StringView {
        StringView {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// The null/empty StringView (ptr=null, len=0).
    pub const fn null() -> StringView {
        StringView {
            ptr: core::ptr::null(),
            len: 0,
        }
    }

    /// Returns the StringView contents as a `&str`.
    ///
    /// # Safety
    /// Caller must ensure `ptr` is valid UTF-8 for `len` bytes and the memory is live.
    pub unsafe fn as_str(&self) -> &str {
        // SAFETY: string_view_as_str is only called with host-owned StringViews created
        // via string_view_from_static — guarantees valid UTF-8.
        // Plugin-provided StringViews must never be passed to this function.
        unsafe {
            let slice: &[u8] = core::slice::from_raw_parts(self.ptr, self.len);
            core::str::from_utf8_unchecked(slice) // SAFETY: see comment above
        }
    }

    /// Copies the StringView contents into a new owned `String`.
    ///
    /// # Safety
    /// Caller must ensure `ptr` is valid UTF-8 for `len` bytes and the memory is live.
    pub unsafe fn to_owned_string(&self) -> String {
        // SAFETY: Caller guarantees ptr is valid, non-null, UTF-8, and live.
        unsafe { self.as_str().to_owned() }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::types::string_view::StringView;

    #[test]
    fn layout_string_view() {
        assert_eq!(size_of::<StringView>(), 16);
        assert_eq!(align_of::<StringView>(), 8);
        assert_eq!(offset_of!(StringView, ptr), 0);
        assert_eq!(offset_of!(StringView, len), 8);
    }
}