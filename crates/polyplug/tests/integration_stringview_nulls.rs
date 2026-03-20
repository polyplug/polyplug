#![allow(clippy::expect_used)]

//! Integration tests: StringView with embedded null bytes.
//! polyplug never treats StringView as null-terminated — embedded \x00 bytes must
//! be preserved through any polyplug-internal API that processes StringViews.

use polyplug_abi::StringView;

unsafe extern "C" {
    fn polyplug_host_alloc(size: usize, align: usize) -> *mut u8;
    fn polyplug_host_free(ptr: *mut u8, size: usize, align: usize);
}

#[test]
fn test_stringview_embedded_null_length() {
    // StringView with embedded null — len must be 11, not 5
    let data: &[u8] = b"hello\x00world";
    let sv: StringView = StringView {
        ptr: data.as_ptr(),
        len: data.len(),
    };
    assert_eq!(
        sv.len, 11_usize,
        "StringView len must not be truncated at null byte"
    );
}

#[test]
fn test_stringview_roundtrip_through_host_alloc() {
    // Allocate host memory for data with embedded null, write to it,
    // read it back, assert no truncation.
    let data: &[u8] = b"hello\x00world";
    let size: usize = data.len();
    // SAFETY: size > 0, align = 1 is a power of two.
    let ptr: *mut u8 = unsafe { polyplug_host_alloc(size, 1_usize) };
    assert!(!ptr.is_null(), "host_alloc must succeed for size=11");
    // SAFETY: ptr is non-null and valid for `size` bytes.
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, size) };
    // Read back the bytes
    // SAFETY: ptr valid for size bytes, just written.
    let read_back: &[u8] = unsafe { core::slice::from_raw_parts(ptr, size) };
    assert_eq!(
        read_back, data,
        "data with embedded null must round-trip unchanged"
    );
    // SAFETY: ptr was allocated with size=11, align=1.
    unsafe { polyplug_host_free(ptr, size, 1_usize) };
}

#[test]
fn test_stringview_from_static_with_embedded_null() {
    // Test that constructing a StringView pointing to static data with embedded null
    // preserves the full length.
    let data: &'static [u8] = b"poly\x00plug";
    let sv: StringView = StringView {
        ptr: data.as_ptr(),
        len: data.len(), // 9
    };
    assert_eq!(
        sv.len, 9_usize,
        "static StringView with embedded null must have full len"
    );
    // Verify raw slice reconstruction preserves null
    // SAFETY: sv.ptr is a valid pointer to sv.len bytes of static data.
    let slice: &[u8] = unsafe { core::slice::from_raw_parts(sv.ptr, sv.len) };
    assert_eq!(slice[4], 0_u8, "byte at index 4 must be null");
    assert_eq!(slice[5], b'p', "byte at index 5 must be 'p'");
}
