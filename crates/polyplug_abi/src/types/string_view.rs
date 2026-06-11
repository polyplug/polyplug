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

    /// True for the null-view sentinel (`ptr` is null).
    ///
    /// `Option<StringView>` is not FFI-safe, so ABI structs use the null view
    /// as their "absent string" convention (e.g. `ReloadPhase::reason` outside
    /// the `Failed` phase).
    pub const fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    /// Returns the StringView contents as a `&str`, assuming valid UTF-8.
    ///
    /// A null pointer or zero length yields `""` — this is the defined behaviour for
    /// [`StringView::null`] and any empty view, and never dereferences the pointer.
    ///
    /// # Safety
    /// For a non-null, non-empty view, the caller must guarantee that:
    /// - `ptr` points to `len` initialized bytes that remain live for the borrow, and
    /// - those bytes are valid UTF-8.
    ///
    /// UTF-8 validity is NOT checked here. Prefer [`StringView::try_as_str`] when the
    /// bytes originate from an untrusted source (e.g. plugin-provided data).
    pub unsafe fn as_str(&self) -> &str {
        if self.ptr.is_null() || self.len == 0 {
            return "";
        }
        // SAFETY: ptr is non-null and the caller guarantees `len` live, initialized,
        // valid-UTF-8 bytes (documented contract above).
        unsafe {
            let slice: &[u8] = core::slice::from_raw_parts(self.ptr, self.len);
            core::str::from_utf8_unchecked(slice)
        }
    }

    /// Returns the StringView contents as a `&str`, validating UTF-8.
    ///
    /// A null pointer or zero length yields `Ok("")` without dereferencing the pointer.
    /// For a non-null, non-empty view, the bytes are validated with
    /// [`core::str::from_utf8`]; invalid UTF-8 returns `Err`.
    ///
    /// # Safety
    /// For a non-null, non-empty view, the caller must guarantee that `ptr` points to
    /// `len` initialized bytes that remain live for the borrow. UTF-8 validity is
    /// checked, so this is the correct entry point for untrusted (plugin-provided) data.
    pub unsafe fn try_as_str(&self) -> Result<&str, core::str::Utf8Error> {
        if self.ptr.is_null() || self.len == 0 {
            return Ok("");
        }
        // SAFETY: ptr is non-null and the caller guarantees `len` live, initialized
        // bytes (documented contract above). UTF-8 is validated below, not assumed.
        let slice: &[u8] = unsafe { core::slice::from_raw_parts(self.ptr, self.len) };
        core::str::from_utf8(slice)
    }

    /// Copies the StringView contents into a new owned `String`.
    ///
    /// A null pointer or zero length yields an empty `String`.
    ///
    /// # Safety
    /// Same contract as [`StringView::as_str`]: for a non-null, non-empty view the
    /// caller must guarantee `len` live, initialized, valid-UTF-8 bytes.
    pub unsafe fn to_owned_string(&self) -> String {
        // SAFETY: forwards the documented as_str contract; null/empty is handled there.
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

    #[test]
    fn is_null_distinguishes_null_view_from_live_view() {
        assert!(StringView::null().is_null());
        let sv: StringView = StringView::from_static(b"x");
        assert!(!sv.is_null());
    }

    #[test]
    fn as_str_on_null_returns_empty() {
        let sv: StringView = StringView::null();
        // SAFETY: null view never dereferences the pointer; defined to yield "".
        let s: &str = unsafe { sv.as_str() };
        assert_eq!(s, "");
    }

    #[test]
    fn try_as_str_on_null_returns_empty() {
        let sv: StringView = StringView::null();
        // SAFETY: null view never dereferences the pointer; defined to yield Ok("").
        let s: Result<&str, core::str::Utf8Error> = unsafe { sv.try_as_str() };
        assert_eq!(s, Ok(""));
    }

    #[test]
    fn try_as_str_on_valid_utf8_returns_str() {
        let bytes: &'static [u8] = b"hello";
        let sv: StringView = StringView::from_static(bytes);
        // SAFETY: from_static yields a live, valid-UTF-8 view for the program lifetime.
        let s: Result<&str, core::str::Utf8Error> = unsafe { sv.try_as_str() };
        assert_eq!(s, Ok("hello"));
    }

    #[test]
    fn try_as_str_on_invalid_utf8_errors() {
        // 0xFF is never a valid UTF-8 byte.
        let bytes: &'static [u8] = &[0xFF, 0xFE];
        let sv: StringView = StringView::from_static(bytes);
        // SAFETY: from_static yields a live view; UTF-8 is checked, not assumed.
        let s: Result<&str, core::str::Utf8Error> = unsafe { sv.try_as_str() };
        assert!(s.is_err());
    }
}
