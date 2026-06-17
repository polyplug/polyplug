# DataRecord ABI Type Reference

## Terminology Note

This document uses the following terminology (current as of the pre-1.0 ABI):
- **HostApi**: The runtime's ABI table provided to guests during `polyplug_init`
- **GuestContractInterface**: The interface struct a plugin provides for the host to call

## Overview

This document is the **canonical reference** for the `DataRecord` type used across all
example plugins in the `examples/` directory (decoder, transformer, encoder, reporter, validator). All language
bindings must mirror these layouts **inline** — plugins must NOT depend on any shared
crate or library for these definitions. The polyplug ABI **freezes at v1.0**
(currently pre-1.0; see `crates/polyplug_abi/`).

The rule: every language plugin copies these `#[repr(C)]`-compatible struct definitions
verbatim. If the definitions here and the plugin's local copy diverge, the plugin will
silently corrupt data across the boundary.

---

## Memory Layout (x86_64, all languages)

```
DataRecord — total: 40 bytes, align: 8
┌─────────────────────────────────────────┐
│ name    : StringView  [offset  0, 16B]  │
│   .ptr  : *const u8   [offset  0,  8B]  │
│   .len  : usize       [offset  8,  8B]  │
├─────────────────────────────────────────┤
│ value   : StringView  [offset 16, 16B]  │
│   .ptr  : *const u8   [offset 16,  8B]  │
│   .len  : usize       [offset 24,  8B]  │
├─────────────────────────────────────────┤
│ count   : u32         [offset 32,  4B]  │
│ _pad    : [u8; 4]     [offset 36,  4B]  │  ← implicit compiler padding
└─────────────────────────────────────────┘
```

---

## StringView Layout

`StringView` is a **non-owning, borrowed** UTF-8 string reference. Never freed by the
receiver. Valid only for the duration of the call.

```
StringView — total: 16 bytes, align: 8
┌─────────────────────────────────────────┐
│ ptr : *const u8   [offset 0, 8B]        │  ← UTF-8 bytes, NOT null-terminated
│ len : usize       [offset 8, 8B]        │  ← byte count
└─────────────────────────────────────────┘
```

---

## Rust `#[repr(C)]` Definition

Mirror these inline in each Rust plugin. Do NOT import from `polyplug`.

```rust
/// Non-owning UTF-8 string view — mirrors polyplug::abi::StringView.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StringView {
    pub ptr: *const u8,
    pub len: usize,
}

/// Example data record — passed across the ABI boundary between plugins.
/// NOT part of polyplug core ABI — defined here for all example plugins.
/// NOT part of polyplug core ABI — defined here for all showcase plugins.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DataRecord {
    pub name:  StringView,
    pub value: StringView,
    pub count: u32,
    // 4 bytes implicit padding here (compiler-inserted to align to 8 bytes)
}
```

---

## C++ Definition

```cpp
#pragma pack(push, 1)  // ensure no extra padding surprises; layout matches Rust repr(C)
struct StringView {
    const uint8_t* ptr;  // UTF-8, NOT null-terminated
    size_t         len;
};

struct DataRecord {
    StringView name;
    StringView value;
    uint32_t   count;
    uint8_t    _pad[4];  // explicit padding to match Rust repr(C)
};
#pragma pack(pop)
// static_assert(sizeof(DataRecord) == 40, "DataRecord ABI size mismatch");
// static_assert(sizeof(StringView) == 16, "StringView ABI size mismatch");
```

---

## C# `[StructLayout]` Definition

```csharp
using System.Runtime.InteropServices;

[StructLayout(LayoutKind.Sequential)]
public struct StringView {
    public IntPtr Ptr;  // UTF-8, NOT null-terminated (IntPtr = 8 bytes on 64-bit)
    public ulong  Len;  // byte count (ulong = 8 bytes, matches Rust usize on 64-bit)
}

[StructLayout(LayoutKind.Sequential)]
public struct DataRecord {
    public StringView Name;
    public StringView Value;
    public uint       Count;
    private uint      _pad;  // explicit padding — must match Rust repr(C) layout
}
// Marshal.SizeOf<DataRecord>() must equal 40
// Marshal.SizeOf<StringView>() must equal 16
```

---

## Python `ctypes` Definition

```python
import ctypes

class StringView(ctypes.Structure):
    _fields_ = [
        ("ptr", ctypes.c_char_p),  # UTF-8 pointer, NOT null-terminated
        ("len", ctypes.c_size_t),
    ]
    # sizeof(StringView) == 16 on 64-bit

class DataRecord(ctypes.Structure):
    _fields_ = [
        ("name",  StringView),
        ("value", StringView),
        ("count", ctypes.c_uint32),
        ("_pad",  ctypes.c_uint32),  # explicit padding to match repr(C)
    ]
    # assert ctypes.sizeof(DataRecord) == 40
```

---

## Lua FFI Definition

```lua
local ffi = require("ffi")

ffi.cdef[[
    typedef struct {
        const uint8_t* ptr;   /* UTF-8, NOT null-terminated */
        size_t         len;
    } StringView;             /* sizeof == 16 */

    typedef struct {
        StringView name;
        StringView value;
        uint32_t   count;
        uint8_t    _pad[4];   /* explicit padding to match repr(C) layout */
    } DataRecord;             /* sizeof == 40 */
]]
-- assert(ffi.sizeof("DataRecord") == 40)
-- assert(ffi.sizeof("StringView") == 16)
```

---

## AbiError Layout

Under the out-param ABI, `AbiError` is **written through a trailing
`*mut AbiError` out-parameter** — every ABI function that can fail returns
`void` and writes its `AbiError` through that pointer. The sole exception is
`polyplug_init`, which still returns `AbiError` by value (it is the plugin
entry point, not an `HostApi`/interface function pointer). The 24-byte layout
below is unchanged either way. Defined in
`crates/polyplug_abi/src/types/abi_error.rs`. Mirror inline in all plugins.

```
AbiError — total: 24 bytes, align: 8
┌─────────────────────────────────────────┐
│ code    : u32         [offset  0,  4B]  │  ← 0 = success
│ _pad    : [u8; 4]     [offset  4,  4B]  │  ← implicit padding
│ message : StringView  [offset  8, 16B]  │  ← NULL/empty if code == 0
└─────────────────────────────────────────┘
```

```rust
/// ABI error — mirrors polyplug_abi::types::AbiError.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AbiError {
    pub code:    u32,        // 0 = AbiErrorCode::Ok
    pub message: StringView, // static or runtime-owned; receiver must NEVER free. NULL if code==0.
}
```

`code` is a raw `u32` at the ABI boundary, **not** the `AbiErrorCode` enum.
Plugins are untrusted and write `AbiError` through the out-param across the C
ABI, so any 32-bit pattern can land here — including values that are not declared
discriminants of the frozen `AbiErrorCode` enum. Materializing such a value as
the enum would be instant undefined behaviour, so the field stays a raw `u32`.
Construct it with `AbiErrorCode::X as u32` and interpret it with
`AbiErrorCode::from_u32`, which is total and safe. The reserved values below
correspond to the `AbiErrorCode` enum; any other value (plugin-defined or
hostile) collapses to `Generic` when converted.

Reserved error codes (0–255 runtime, 256+ plugin-defined):
- `0`   — `AbiErrorCode::Ok`
- `1`   — `AbiErrorCode::Generic`
- `2`   — `AbiErrorCode::BufferTooSmall`
- `3`   — `AbiErrorCode::Panic`
- `4`   — `AbiErrorCode::NotFound`
- `5`   — `AbiErrorCode::StaleHandle`
- `6`   — `AbiErrorCode::FunctionNotAvailable`
- `7`   — `AbiErrorCode::DuplicateProvider`
- `8`   — `AbiErrorCode::InvalidPointer`
- `100` — `AbiErrorCode::HostContractNotFound`
- `101` — `AbiErrorCode::HostContractVersionMismatch`
- `102` — `AbiErrorCode::HostContractCallFailed`

---

## HostApi Note

`HostApi` is **not** part of `DataRecord`. It is passed once at plugin init via
`polyplug_init(host: *const HostApi, ctx: *const BundleInitContext)` and must
**never** be stored in a static or module-global (SDKs are static-free). The
generated `polyplug_init` hands the pointer to the author factory
(`polyplug_create_<plugin>` in Rust/C++, the registered factory in C#/Python),
which captures it in the instance; in the per-bundle Lua/JS VMs it lives as
per-VM (i.e. per-runtime-per-bundle) state. It provides `alloc`, `free`,
`find_guest_contract`, `register_guest_contract`, and other host services.

Plugins must use `host.alloc` / `host.free` for all cross-boundary memory. Never use
the system allocator for data that crosses the ABI boundary.

---

## Verification Checklist

Before releasing any example plugin, verify:
- [ ] `sizeof(DataRecord) == 40` on the target platform
- [ ] `sizeof(StringView) == 16` on the target platform
- [ ] `sizeof(AbiError)   == 24` on the target platform
- [ ] All strings passed in `DataRecord` are valid UTF-8
- [ ] `StringView.ptr` remains valid for the entire duration of the call
- [ ] `AbiError.message.ptr` is freed by the caller after reading (if non-NULL)
