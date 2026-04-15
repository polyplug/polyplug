//! Layout calculation tests for polyplug_codegen.
//!
//! Verifies that all type layout calculations (size, alignment, offsets) are accurate
//! for ABI compatibility across all 6 languages. Wrong padding = broken FFI.
//!
//! Test Categories:
//! - Category 1: Primitive Types (6 tests)
//! - Category 2: ABI Built-in Types (5 tests)
//! - Category 3: Enum Types (5 tests)
//! - Category 4: Struct Layouts with Padding (6 tests)
//! - Category 5: Complex Multi-Param Cases (5+ tests)

#![allow(clippy::expect_used)]

use std::mem::{align_of, offset_of, size_of};

// ─── Category 1: Primitive Types (6 tests) ─────────────────────────────────────

/// u8: size=1, align=1
#[test]
fn layout_u8_size_align() {
    assert_eq!(size_of::<u8>(), 1, "u8 size must be 1 byte");
    assert_eq!(align_of::<u8>(), 1, "u8 alignment must be 1 byte");
}

/// u16: size=2, align=2
#[test]
fn layout_u16_size_align() {
    assert_eq!(size_of::<u16>(), 2, "u16 size must be 2 bytes");
    assert_eq!(align_of::<u16>(), 2, "u16 alignment must be 2 bytes");
}

/// u32: size=4, align=4
#[test]
fn layout_u32_size_align() {
    assert_eq!(size_of::<u32>(), 4, "u32 size must be 4 bytes");
    assert_eq!(align_of::<u32>(), 4, "u32 alignment must be 4 bytes");
}

/// u64: size=8, align=8
#[test]
fn layout_u64_size_align() {
    assert_eq!(size_of::<u64>(), 8, "u64 size must be 8 bytes");
    assert_eq!(align_of::<u64>(), 8, "u64 alignment must be 8 bytes");
}

/// usize: size=8, align=8 (x86_64)
#[test]
#[cfg(target_pointer_width = "64")]
fn layout_usize_size_align() {
    assert_eq!(
        size_of::<usize>(),
        8,
        "usize size must be 8 bytes on x86_64"
    );
    assert_eq!(
        align_of::<usize>(),
        8,
        "usize alignment must be 8 bytes on x86_64"
    );
}

/// bool: size=1, align=1
#[test]
fn layout_bool_size_align() {
    assert_eq!(size_of::<bool>(), 1, "bool size must be 1 byte");
    assert_eq!(align_of::<bool>(), 1, "bool alignment must be 1 byte");
}

// ─── Category 2: ABI Built-in Types (5 tests) ───────────────────────────────────

use polyplug_abi::{AbiError, Buffer, GuestContractHandle, StringView};

/// StringView: size=16, align=8, ptr@0, len@8
#[test]
fn layout_stringview_fields_and_size() {
    assert_eq!(
        size_of::<StringView>(),
        16,
        "StringView size must be 16 bytes (ptr + len)"
    );
    assert_eq!(
        align_of::<StringView>(),
        8,
        "StringView alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(StringView, ptr),
        0,
        "StringView.ptr must be at offset 0"
    );
    assert_eq!(
        offset_of!(StringView, len),
        8,
        "StringView.len must be at offset 8"
    );
}

/// Buffer: size=24, align=8, ptr@0, len@8, cap@16
#[test]
fn layout_buffer_fields_and_size() {
    assert_eq!(
        size_of::<Buffer>(),
        24,
        "Buffer size must be 24 bytes (ptr + len + cap)"
    );
    assert_eq!(align_of::<Buffer>(), 8, "Buffer alignment must be 8 bytes");
    assert_eq!(offset_of!(Buffer, ptr), 0, "Buffer.ptr must be at offset 0");
    assert_eq!(offset_of!(Buffer, len), 8, "Buffer.len must be at offset 8");
    assert_eq!(
        offset_of!(Buffer, cap),
        16,
        "Buffer.cap must be at offset 16"
    );
}

/// AbiError: size=24, align=8, code@0, message@8
#[test]
fn layout_abierror_fields_and_size() {
    assert_eq!(
        size_of::<AbiError>(),
        24,
        "AbiError size must be 24 bytes (code + message)"
    );
    assert_eq!(
        align_of::<AbiError>(),
        8,
        "AbiError alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(AbiError, code),
        0,
        "AbiError.code must be at offset 0"
    );
    assert_eq!(
        offset_of!(AbiError, message),
        8,
        "AbiError.message must be at offset 8"
    );
}

/// GuestContractHandle: size=4, align=4, index@0 (opaque handle)
#[test]
fn layout_plugin_handle_fields_and_size() {
    assert_eq!(
        size_of::<GuestContractHandle>(),
        4,
        "GuestContractHandle size must be 4 bytes (opaque index handle)"
    );
    assert_eq!(
        align_of::<GuestContractHandle>(),
        4,
        "GuestContractHandle alignment must be 4 bytes"
    );
    assert_eq!(
        offset_of!(GuestContractHandle, index),
        0,
        "GuestContractHandle.index must be at offset 0"
    );
}

// ─── Category 3: Enum Types (5 tests) ───────────────────────────────────────────

/// #[repr(u8)] enum: size=1, align=1
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
enum EnumU8 {
    A = 0,
    B = 1,
}

#[test]
fn layout_enum_u8_size_align() {
    assert_eq!(
        size_of::<EnumU8>(),
        1,
        "#[repr(u8)] enum size must be 1 byte"
    );
    assert_eq!(
        align_of::<EnumU8>(),
        1,
        "#[repr(u8)] enum alignment must be 1 byte"
    );
}

/// #[repr(u16)] enum: size=2, align=2
#[repr(u16)]
#[derive(Debug, Clone, Copy)]
enum EnumU16 {
    A = 0,
    B = 1,
}

#[test]
fn layout_enum_u16_size_align() {
    assert_eq!(
        size_of::<EnumU16>(),
        2,
        "#[repr(u16)] enum size must be 2 bytes"
    );
    assert_eq!(
        align_of::<EnumU16>(),
        2,
        "#[repr(u16)] enum alignment must be 2 bytes"
    );
}

/// #[repr(u32)] enum: size=4, align=4
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
enum EnumU32 {
    A = 0,
    B = 1,
}

#[test]
fn layout_enum_u32_size_align() {
    assert_eq!(
        size_of::<EnumU32>(),
        4,
        "#[repr(u32)] enum size must be 4 bytes"
    );
    assert_eq!(
        align_of::<EnumU32>(),
        4,
        "#[repr(u32)] enum alignment must be 4 bytes"
    );
}

/// LogLevel (repr=u32): size=4, align=4
/// This matches the LogLevel enum used in host contracts.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

#[test]
fn layout_loglevel_size_align() {
    assert_eq!(
        size_of::<LogLevel>(),
        4,
        "LogLevel (#[repr(u32)]) size must be 4 bytes"
    );
    assert_eq!(
        align_of::<LogLevel>(),
        4,
        "LogLevel (#[repr(u32)]) alignment must be 4 bytes"
    );
}

/// Single variant enum: correct size
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
enum SingleVariant {
    OnlyOne = 42,
}

#[test]
fn layout_enum_single_variant() {
    assert_eq!(
        size_of::<SingleVariant>(),
        4,
        "Single variant #[repr(u32)] enum size must be 4 bytes"
    );
    assert_eq!(
        align_of::<SingleVariant>(),
        4,
        "Single variant #[repr(u32)] enum alignment must be 4 bytes"
    );
}

// ─── Category 4: Struct Layouts with Padding (6 tests) ───────────────────────────

/// All fields naturally aligned, no padding
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SimpleStructNoPadding {
    a: u32,
    b: u32,
    c: u64,
}

#[test]
fn layout_simple_struct_no_padding() {
    // a@0 (4 bytes), b@4 (4 bytes), c@8 (8 bytes) = 16 bytes total
    assert_eq!(
        size_of::<SimpleStructNoPadding>(),
        16,
        "SimpleStructNoPadding size must be 16 bytes"
    );
    assert_eq!(
        align_of::<SimpleStructNoPadding>(),
        8,
        "SimpleStructNoPadding alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(SimpleStructNoPadding, a),
        0,
        "a must be at offset 0"
    );
    assert_eq!(
        offset_of!(SimpleStructNoPadding, b),
        4,
        "b must be at offset 4"
    );
    assert_eq!(
        offset_of!(SimpleStructNoPadding, c),
        8,
        "c must be at offset 8"
    );
}

/// Small field before large field needs padding
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct StructWithInternalPadding {
    a: u8, // 1 byte @ 0
    // 7 bytes padding
    b: u64, // 8 bytes @ 8
    c: u32, // 4 bytes @ 16
            // 4 bytes trailing padding to align to 8
}

#[test]
fn layout_struct_with_internal_padding() {
    // Total: 1 + 7(pad) + 8 + 4 + 4(pad) = 24 bytes
    assert_eq!(
        size_of::<StructWithInternalPadding>(),
        24,
        "StructWithInternalPadding size must be 24 bytes"
    );
    assert_eq!(
        align_of::<StructWithInternalPadding>(),
        8,
        "StructWithInternalPadding alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(StructWithInternalPadding, a),
        0,
        "a must be at offset 0"
    );
    assert_eq!(
        offset_of!(StructWithInternalPadding, b),
        8,
        "b must be at offset 8 (after 7 bytes padding)"
    );
    assert_eq!(
        offset_of!(StructWithInternalPadding, c),
        16,
        "c must be at offset 16"
    );

    // Verify padding between a and b
    let padding_before_b: usize = offset_of!(StructWithInternalPadding, b)
        - (offset_of!(StructWithInternalPadding, a) + size_of::<u8>());
    assert_eq!(
        padding_before_b, 7,
        "Expected 7 bytes padding between a and b"
    );
}

/// LogWithLevelArgs: size=24, level@0, message@8, pad@4
/// This matches the LogWithLevelArgs struct used in host contracts.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct LogWithLevelArgs {
    level: LogLevel, // 4 bytes @ 0
    // 4 bytes padding
    message: StringView, // 16 bytes @ 8
}

#[test]
fn layout_logwithlevelargs_layout() {
    // level(4) + pad(4) + message(16) = 24 bytes
    assert_eq!(
        size_of::<LogWithLevelArgs>(),
        24,
        "LogWithLevelArgs size must be 24 bytes"
    );
    assert_eq!(
        align_of::<LogWithLevelArgs>(),
        8,
        "LogWithLevelArgs alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(LogWithLevelArgs, level),
        0,
        "level must be at offset 0"
    );
    assert_eq!(
        offset_of!(LogWithLevelArgs, message),
        8,
        "message must be at offset 8 (after 4 bytes padding)"
    );

    // Verify padding between level and message
    let padding: usize = offset_of!(LogWithLevelArgs, message)
        - (offset_of!(LogWithLevelArgs, level) + size_of::<LogLevel>());
    assert_eq!(
        padding, 4,
        "Expected 4 bytes padding between level and message"
    );
}

/// Final size rounded to alignment
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct StructTrailingPadding {
    a: u64, // 8 bytes @ 0
    b: u8,  // 1 byte @ 8
            // 7 bytes trailing padding to align to 8
}

#[test]
fn layout_struct_trailing_padding() {
    // Total: 8 + 1 + 7(pad) = 16 bytes
    assert_eq!(
        size_of::<StructTrailingPadding>(),
        16,
        "StructTrailingPadding size must be 16 bytes"
    );
    assert_eq!(
        align_of::<StructTrailingPadding>(),
        8,
        "StructTrailingPadding alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(StructTrailingPadding, a),
        0,
        "a must be at offset 0"
    );
    assert_eq!(
        offset_of!(StructTrailingPadding, b),
        8,
        "b must be at offset 8"
    );

    // Verify trailing padding
    let trailing_padding: usize = size_of::<StructTrailingPadding>()
        - (offset_of!(StructTrailingPadding, b) + size_of::<u8>());
    assert_eq!(trailing_padding, 7, "Expected 7 bytes trailing padding");
}

/// Struct containing other structs
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct InnerStruct {
    x: u32,
    y: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct NestedStruct {
    inner: InnerStruct, // 8 bytes @ 0
    z: u64,             // 8 bytes @ 8
}

#[test]
fn layout_nested_struct() {
    // inner(8) + z(8) = 16 bytes
    assert_eq!(
        size_of::<NestedStruct>(),
        16,
        "NestedStruct size must be 16 bytes"
    );
    assert_eq!(
        align_of::<NestedStruct>(),
        8,
        "NestedStruct alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(NestedStruct, inner),
        0,
        "inner must be at offset 0"
    );
    assert_eq!(offset_of!(NestedStruct, z), 8, "z must be at offset 8");

    // Verify InnerStruct layout
    assert_eq!(
        size_of::<InnerStruct>(),
        8,
        "InnerStruct size must be 8 bytes"
    );
    assert_eq!(
        offset_of!(InnerStruct, x),
        0,
        "InnerStruct.x must be at offset 0"
    );
    assert_eq!(
        offset_of!(InnerStruct, y),
        4,
        "InnerStruct.y must be at offset 4"
    );
}

/// Struct containing enum field (LogLevel)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct StructWithEnumField {
    status: LogLevel, // 4 bytes @ 0
    value: u64,       // 8 bytes @ 8 (after 4 bytes padding)
}

#[test]
fn layout_struct_with_enum_field() {
    // status(4) + pad(4) + value(8) = 16 bytes
    assert_eq!(
        size_of::<StructWithEnumField>(),
        16,
        "StructWithEnumField size must be 16 bytes"
    );
    assert_eq!(
        align_of::<StructWithEnumField>(),
        8,
        "StructWithEnumField alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(StructWithEnumField, status),
        0,
        "status must be at offset 0"
    );
    assert_eq!(
        offset_of!(StructWithEnumField, value),
        8,
        "value must be at offset 8"
    );

    // Verify padding between status and value
    let padding: usize = offset_of!(StructWithEnumField, value)
        - (offset_of!(StructWithEnumField, status) + size_of::<LogLevel>());
    assert_eq!(
        padding, 4,
        "Expected 4 bytes padding between status and value"
    );
}

// ─── Category 5: Complex Multi-Param Cases (5+ tests) ───────────────────────────

/// (u32, u32): size=8, align=4
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct TwoPrimitivesNoPadding {
    a: u32,
    b: u32,
}

#[test]
fn layout_two_primitives_no_padding() {
    // a(4) + b(4) = 8 bytes
    assert_eq!(
        size_of::<TwoPrimitivesNoPadding>(),
        8,
        "TwoPrimitivesNoPadding size must be 8 bytes"
    );
    assert_eq!(
        align_of::<TwoPrimitivesNoPadding>(),
        4,
        "TwoPrimitivesNoPadding alignment must be 4 bytes"
    );
    assert_eq!(
        offset_of!(TwoPrimitivesNoPadding, a),
        0,
        "a must be at offset 0"
    );
    assert_eq!(
        offset_of!(TwoPrimitivesNoPadding, b),
        4,
        "b must be at offset 4"
    );
}

/// (u32, u64): size=16, align=8, second@8
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct TwoPrimitivesWithPadding {
    a: u32, // 4 bytes @ 0
    // 4 bytes padding
    b: u64, // 8 bytes @ 8
}

#[test]
fn layout_two_primitives_with_padding() {
    // a(4) + pad(4) + b(8) = 16 bytes
    assert_eq!(
        size_of::<TwoPrimitivesWithPadding>(),
        16,
        "TwoPrimitivesWithPadding size must be 16 bytes"
    );
    assert_eq!(
        align_of::<TwoPrimitivesWithPadding>(),
        8,
        "TwoPrimitivesWithPadding alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(TwoPrimitivesWithPadding, a),
        0,
        "a must be at offset 0"
    );
    assert_eq!(
        offset_of!(TwoPrimitivesWithPadding, b),
        8,
        "b must be at offset 8"
    );

    // Verify padding between a and b
    let padding: usize = offset_of!(TwoPrimitivesWithPadding, b)
        - (offset_of!(TwoPrimitivesWithPadding, a) + size_of::<u32>());
    assert_eq!(padding, 4, "Expected 4 bytes padding between a and b");
}

/// (u8, StringView, u32): size=32, align=8
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ThreeParamsMixed {
    a: u8, // 1 byte @ 0
    // 7 bytes padding
    b: StringView, // 16 bytes @ 8
    c: u32,        // 4 bytes @ 24
                   // 4 bytes trailing padding
}

#[test]
fn layout_three_params_mixed() {
    // a(1) + pad(7) + b(16) + c(4) + pad(4) = 32 bytes
    assert_eq!(
        size_of::<ThreeParamsMixed>(),
        32,
        "ThreeParamsMixed size must be 32 bytes"
    );
    assert_eq!(
        align_of::<ThreeParamsMixed>(),
        8,
        "ThreeParamsMixed alignment must be 8 bytes"
    );
    assert_eq!(offset_of!(ThreeParamsMixed, a), 0, "a must be at offset 0");
    assert_eq!(offset_of!(ThreeParamsMixed, b), 8, "b must be at offset 8");
    assert_eq!(
        offset_of!(ThreeParamsMixed, c),
        24,
        "c must be at offset 24"
    );

    // Verify padding between a and b
    let padding_ab: usize =
        offset_of!(ThreeParamsMixed, b) - (offset_of!(ThreeParamsMixed, a) + size_of::<u8>());
    assert_eq!(padding_ab, 7, "Expected 7 bytes padding between a and b");
}

/// (LogLevel, StringView): size=24, align=8
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct EnumThenStringView {
    level: LogLevel, // 4 bytes @ 0
    // 4 bytes padding
    message: StringView, // 16 bytes @ 8
}

#[test]
fn layout_enum_then_stringview() {
    // level(4) + pad(4) + message(16) = 24 bytes
    assert_eq!(
        size_of::<EnumThenStringView>(),
        24,
        "EnumThenStringView size must be 24 bytes"
    );
    assert_eq!(
        align_of::<EnumThenStringView>(),
        8,
        "EnumThenStringView alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(EnumThenStringView, level),
        0,
        "level must be at offset 0"
    );
    assert_eq!(
        offset_of!(EnumThenStringView, message),
        8,
        "message must be at offset 8"
    );
}

/// (StringView, LogLevel): size=24, align=8, different offsets
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct StringViewThenEnum {
    message: StringView, // 16 bytes @ 0
    level: LogLevel,     // 4 bytes @ 16
                         // 4 bytes trailing padding
}

#[test]
fn layout_stringview_then_enum() {
    // message(16) + level(4) + pad(4) = 24 bytes
    assert_eq!(
        size_of::<StringViewThenEnum>(),
        24,
        "StringViewThenEnum size must be 24 bytes"
    );
    assert_eq!(
        align_of::<StringViewThenEnum>(),
        8,
        "StringViewThenEnum alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(StringViewThenEnum, message),
        0,
        "message must be at offset 0"
    );
    assert_eq!(
        offset_of!(StringViewThenEnum, level),
        16,
        "level must be at offset 16"
    );

    // Verify trailing padding
    let trailing_padding: usize = size_of::<StringViewThenEnum>()
        - (offset_of!(StringViewThenEnum, level) + size_of::<LogLevel>());
    assert_eq!(trailing_padding, 4, "Expected 4 bytes trailing padding");

    // Compare with EnumThenStringView - same size but different field order
    assert_eq!(
        size_of::<EnumThenStringView>(),
        size_of::<StringViewThenEnum>(),
        "Both structs should have same total size"
    );
    // But offsets are different
    assert_ne!(
        offset_of!(EnumThenStringView, level),
        offset_of!(StringViewThenEnum, level),
        "level offset differs based on field order"
    );
}

// ─── Additional Edge Cases ─────────────────────────────────────────────────────

/// Empty struct: size=0, align=1 (Rust-specific)
#[repr(C)]
struct EmptyStruct {}

#[test]
fn layout_empty_struct() {
    // In Rust, empty structs have size 0 and alignment 1
    assert_eq!(
        size_of::<EmptyStruct>(),
        0,
        "EmptyStruct size must be 0 bytes"
    );
    assert_eq!(
        align_of::<EmptyStruct>(),
        1,
        "EmptyStruct alignment must be 1 byte"
    );
}

/// Struct with only u8 fields: packed tightly
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct AllU8Fields {
    a: u8,
    b: u8,
    c: u8,
    d: u8,
}

#[test]
fn layout_all_u8_fields() {
    // 4 u8s packed tightly = 4 bytes
    assert_eq!(
        size_of::<AllU8Fields>(),
        4,
        "AllU8Fields size must be 4 bytes"
    );
    assert_eq!(
        align_of::<AllU8Fields>(),
        1,
        "AllU8Fields alignment must be 1 byte"
    );
    assert_eq!(offset_of!(AllU8Fields, a), 0, "a must be at offset 0");
    assert_eq!(offset_of!(AllU8Fields, b), 1, "b must be at offset 1");
    assert_eq!(offset_of!(AllU8Fields, c), 2, "c must be at offset 2");
    assert_eq!(offset_of!(AllU8Fields, d), 3, "d must be at offset 3");
}

/// Struct with mixed alignment requiring multiple padding regions
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MultiplePaddingRegions {
    a: u8, // 1 byte @ 0
    // 1 byte padding
    b: u16, // 2 bytes @ 2
    c: u8,  // 1 byte @ 4
    // 3 bytes padding
    d: u32, // 4 bytes @ 8
    e: u8,  // 1 byte @ 12
            // 3 bytes trailing padding (alignment is 4, not 8)
}

#[test]
fn layout_multiple_padding_regions() {
    // a(1) + pad(1) + b(2) + c(1) + pad(3) + d(4) + e(1) + pad(3) = 16 bytes
    // Alignment is 4 (max of u8=1, u16=2, u32=4)
    assert_eq!(
        size_of::<MultiplePaddingRegions>(),
        16,
        "MultiplePaddingRegions size must be 16 bytes"
    );
    assert_eq!(
        align_of::<MultiplePaddingRegions>(),
        4,
        "MultiplePaddingRegions alignment must be 4 bytes"
    );
    assert_eq!(
        offset_of!(MultiplePaddingRegions, a),
        0,
        "a must be at offset 0"
    );
    assert_eq!(
        offset_of!(MultiplePaddingRegions, b),
        2,
        "b must be at offset 2"
    );
    assert_eq!(
        offset_of!(MultiplePaddingRegions, c),
        4,
        "c must be at offset 4"
    );
    assert_eq!(
        offset_of!(MultiplePaddingRegions, d),
        8,
        "d must be at offset 8"
    );
    assert_eq!(
        offset_of!(MultiplePaddingRegions, e),
        12,
        "e must be at offset 12"
    );
}

/// Buffer followed by u32: verifies Buffer alignment
#[repr(C)]
#[derive(Debug)]
struct BufferFollowedByU32 {
    buf: Buffer, // 24 bytes @ 0
    extra: u32,  // 4 bytes @ 24
                 // 4 bytes trailing padding
}

#[test]
fn layout_buffer_followed_by_u32() {
    // buf(24) + extra(4) + pad(4) = 32 bytes
    assert_eq!(
        size_of::<BufferFollowedByU32>(),
        32,
        "BufferFollowedByU32 size must be 32 bytes"
    );
    assert_eq!(
        align_of::<BufferFollowedByU32>(),
        8,
        "BufferFollowedByU32 alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(BufferFollowedByU32, buf),
        0,
        "buf must be at offset 0"
    );
    assert_eq!(
        offset_of!(BufferFollowedByU32, extra),
        24,
        "extra must be at offset 24"
    );
}

/// GuestContractHandle followed by u64: verifies GuestContractHandle alignment
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct HandleFollowedByU64 {
    handle: GuestContractHandle, // 8 bytes @ 0
    value: u64,                  // 8 bytes @ 8
}

#[test]
fn layout_handle_followed_by_u64() {
    // handle(8) + value(8) = 16 bytes
    assert_eq!(
        size_of::<HandleFollowedByU64>(),
        16,
        "HandleFollowedByU64 size must be 16 bytes"
    );
    assert_eq!(
        align_of::<HandleFollowedByU64>(),
        8,
        "HandleFollowedByU64 alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(HandleFollowedByU64, handle),
        0,
        "handle must be at offset 0"
    );
    assert_eq!(
        offset_of!(HandleFollowedByU64, value),
        8,
        "value must be at offset 8"
    );
}

/// #[repr(u64)] enum: size=8, align=8
#[repr(u64)]
#[derive(Debug, Clone, Copy)]
enum EnumU64 {
    A = 0,
    B = 1,
}

#[test]
fn layout_enum_u64_size_align() {
    assert_eq!(
        size_of::<EnumU64>(),
        8,
        "#[repr(u64)] enum size must be 8 bytes"
    );
    assert_eq!(
        align_of::<EnumU64>(),
        8,
        "#[repr(u64)] enum alignment must be 8 bytes"
    );
}

/// i32, i64, f32, f64 primitive type layouts
#[test]
fn layout_signed_and_float_primitives() {
    assert_eq!(size_of::<i8>(), 1, "i8 size must be 1 byte");
    assert_eq!(align_of::<i8>(), 1, "i8 alignment must be 1 byte");

    assert_eq!(size_of::<i16>(), 2, "i16 size must be 2 bytes");
    assert_eq!(align_of::<i16>(), 2, "i16 alignment must be 2 bytes");

    assert_eq!(size_of::<i32>(), 4, "i32 size must be 4 bytes");
    assert_eq!(align_of::<i32>(), 4, "i32 alignment must be 4 bytes");

    assert_eq!(size_of::<i64>(), 8, "i64 size must be 8 bytes");
    assert_eq!(align_of::<i64>(), 8, "i64 alignment must be 8 bytes");

    assert_eq!(size_of::<f32>(), 4, "f32 size must be 4 bytes");
    assert_eq!(align_of::<f32>(), 4, "f32 alignment must be 4 bytes");

    assert_eq!(size_of::<f64>(), 8, "f64 size must be 8 bytes");
    assert_eq!(align_of::<f64>(), 8, "f64 alignment must be 8 bytes");
}

/// Complex struct with all ABI types
#[repr(C)]
#[derive(Debug)]
struct AllAbiTypesStruct {
    sv: StringView,              // 16 bytes @ 0
    buf: Buffer,                 // 24 bytes @ 16
    err: AbiError,               // 24 bytes @ 40
    handle: GuestContractHandle, // 4 bytes @ 64
}

#[test]
fn layout_all_abi_types_struct() {
    // sv(16) + buf(24) + err(24) + handle(4) = 68 bytes, aligned to 8 = 72 bytes
    assert_eq!(
        size_of::<AllAbiTypesStruct>(),
        72,
        "AllAbiTypesStruct size must be 72 bytes"
    );
    assert_eq!(
        align_of::<AllAbiTypesStruct>(),
        8,
        "AllAbiTypesStruct alignment must be 8 bytes"
    );
    assert_eq!(
        offset_of!(AllAbiTypesStruct, sv),
        0,
        "sv must be at offset 0"
    );
    assert_eq!(
        offset_of!(AllAbiTypesStruct, buf),
        16,
        "buf must be at offset 16"
    );
    assert_eq!(
        offset_of!(AllAbiTypesStruct, err),
        40,
        "err must be at offset 40"
    );
    assert_eq!(
        offset_of!(AllAbiTypesStruct, handle),
        64,
        "handle must be at offset 64"
    );
}
