# polyplug-guest — Rust Guest Library

The `polyplug_guest` crate provides the types, constants, and allocator helpers that
**plugin authors** need to implement a [polyplug](https://github.com/your-org/polyplug)
contract in Rust.

> **Who is this for?**
> You are writing a shared library (`.so` / `.dylib` / `.dll`) that will be loaded by a
> polyplug host at runtime. You are **not** writing a host application — see
> `crates/polyplug/` for that.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Cargo.toml Setup](#cargotoml-setup)
- [Available Types](#available-types)
- [ABI Constants](#abi-constants)
- [Hash Utilities](#hash-utilities)
- [Allocator API](#allocator-api)
- [Implementing a Contract — Step by Step](#implementing-a-contract--step-by-step)
- [Full Example](#full-example)
- [manifest.toml — Declaring Your Bundle](#manifesttoml--declaring-your-bundle)
- [Building](#building)
- [Error Handling](#error-handling)
- [Memory Rules](#memory-rules)
- [More Examples](#more-examples)

---

## Quick Start

Add `polyplug_guest` to your plugin's `Cargo.toml`, set `crate-type = ["cdylib"]`, then
export two symbols: `polyplug_abi_version` and `polyplug_init`.

```toml
# Cargo.toml
[package]
name    = "my_plugin"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
polyplug_guest = { workspace = true }
```

```rust
// src/lib.rs
use polyplug_guest::*;

const MY_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B; // FNV-1a of "my.contract@1"

extern "C" fn my_fn(args: *const (), out: *mut ()) -> AbiError {
    // ... read from args, write result to out
    AbiError::ok()
}

static MY_FNS: [FnPtr; 1] = [FnPtr(my_fn as *const ())];

static MY_VTABLE: GuestContractInterface = GuestContractInterface {
    contract_id: MY_CONTRACT_ID,
    contract_version: Version { major: 1, minor: 0, patch: 0 },
    dispatch_type: DispatchType::Native,
    create_instance: my_create_instance,
    destroy_instance: my_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 1,
            functions: MY_FNS.as_ptr(),
        },
    },
};

static MY_DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    name:          StringView { ptr: b"my_plugin".as_ptr(), len: 9 },
    contract_name: StringView { ptr: b"my.contract".as_ptr(), len: 11 },
    version:       Version { major: 1, minor: 0, patch: 0 },
};

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    POLYPLUG_ABI_VERSION
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_init(host: *const HostApi) -> AbiError {
    if host.is_null() {
        return AbiError { code: AbiErrorCode::Generic, message: StringView::null() };
    }
    // SAFETY: host is non-null and provided by the host per ABI contract.
    let iface: &HostApi = unsafe { &*host };
    // SAFETY: register_guest_contract is a valid function pointer set by the host.
    unsafe { (iface.register_guest_contract)(host, &MY_DESCRIPTOR, &MY_VTABLE) }
}
```

---

## Cargo.toml Setup

Your plugin **must** be a `cdylib`:

```toml
[lib]
crate-type = ["cdylib"]
```

**Important:** A `cdylib` cannot have a runtime dependency on the workspace `polyplug`
crate because that would introduce a circular dependency — the host already embeds
`polyplug`. Instead, **mirror the ABI types inline** in your plugin (as shown in the full
example below and in `examples/guests/native/src/lib.rs`). The `polyplug_guest` crate is
a convenience for use during development when circular dependency is not a concern; for
standalone distribution, mirror the types directly.

---

## Available Types

All types are re-exported from `polyplug::abi`. Import them with `use polyplug_guest::*;`
or selectively.

### `StringView`

Non-owning, non-null-terminated UTF-8 string reference.

```rust
#[repr(C)]
pub struct StringView {
    pub ptr: *const u8,
    pub len: usize,
}
```

**Ownership:** Borrowed. `ptr` must remain valid for the duration of the call. The
receiver **must not free** it.

```rust
// Create from a Rust string slice (valid for the call's lifetime):
let s: &str = "hello";
let sv = StringView { ptr: s.as_ptr(), len: s.len() };

// Create a null view (empty / absent):
let null_sv: StringView = StringView::null();
```

---

### `Buffer`

Owning byte buffer allocated via the host allocator.

```rust
#[repr(C)]
pub struct Buffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}
```

**Ownership:** The holder must free with `host.free(host, ptr, cap, 1)` when done.
Never place `Buffer` data on the Rust allocator — always allocate through the stored
`HostApi` (`host.alloc`, e.g. via `alloc_string`).

---

### `AbiError`

Returned by value from all ABI calls.

```rust
#[repr(C)]
pub struct AbiError {
    pub code: u32,
    pub message: StringView,
}
```

- `code == 0` → success (`AbiErrorCode::Ok`).
- `code != 0` → failure; `message` provides a human-readable description.
- **Ownership of `message.ptr`:** always a static or runtime-owned string. The
  receiver **never** frees it (a bare `StringView` carries no allocation provenance).
  For rich, allocated error detail, use `get_last_error` instead.

Convenience constructors:

```rust
AbiError::ok()   // AbiError { code: 0, message: StringView::null() }
```

---

### `GuestContractInterface`

One per contract your plugin implements. Must be `'static`.

```rust
#[repr(C)]
pub struct GuestContractInterface {
    pub contract_id:      u64,
    pub contract_version: Version,     // { major: u32, minor: u32, patch: u32 }
    pub dispatch_type:    DispatchType, // Native (0) or VirtualMachine (1)
    pub create_instance:  unsafe extern "C" fn(
        host: *const HostApi,
        args: *const (),
    ) -> GuestContractInstance,
    pub destroy_instance: unsafe extern "C" fn(
        host: *const HostApi,
        instance: GuestContractInstance,
    ),
    pub dispatch:         DispatchMechanisms, // union of NativeDispatch or VmDispatch
}
```

- `contract_id` is the FNV-1a 64-bit hash of `"name@major_version"`.
- `dispatch_type` determines how to access the `dispatch` union:
  - `Native` — use `dispatch.native.functions[fn_id]` for direct function pointer calls.
  - `VirtualMachine` — use `dispatch.vm.call(loader_data, instance, fn_id, args, out)`.
- `create_instance` / `destroy_instance` manage instance lifecycle (required for hot-reload).

---

### `PluginDescriptor`

Metadata about your plugin. Must be `'static`.

```rust
#[repr(C)]
pub struct PluginDescriptor {
    pub name:          StringView,   // plugin instance name
    pub contract_name: StringView,   // e.g. "pipeline.transformer"
    pub version:       Version,      // { major: u32, minor: u32, patch: u32 }
}
```

---

### `HostApi`

Passed by the host to your `polyplug_init`. Use it **only during init** — do not store it.

```rust
#[repr(C)]
pub struct HostApi {
    pub runtime: *mut c_void,
    pub register_guest_contract: unsafe extern "C" fn(
        this:        *const HostApi,
        descriptor:  *const PluginDescriptor,
        interface:   *const GuestContractInterface,
    ) -> AbiError,
    pub alloc: unsafe extern "C" fn(this: *const HostApi, size: usize, align: usize) -> *mut u8,
    pub free: unsafe extern "C" fn(this: *const HostApi, ptr: *mut u8, size: usize, align: usize),
    pub find_guest_contract: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> GuestContractHandle,
    pub find_all_guest_contracts: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> Array<GuestContractHandle>,
    pub resolve_guest_contract: unsafe extern "C" fn(
        this: *const HostApi,
        handle: GuestContractHandle,
    ) -> *const GuestContractInterface,
    pub get_host_contract: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> HostContractInstance,
    pub resolve_host_contract_interface: unsafe extern "C" fn(
        this: *const HostApi,
        contract_id: u64,
        min_version: u32,
    ) -> *const HostContractInterface,
    pub list_bundles: unsafe extern "C" fn(this: *const HostApi) -> Array<BundleId>,
    pub get_dependencies: unsafe extern "C" fn(this: *const HostApi) -> Array<DependencyInfo>,
    pub load_bundle: unsafe extern "C" fn(
        this: *const HostApi, path: *const u8, path_len: usize,
    ) -> AbiError,
    pub reload_bundle: unsafe extern "C" fn(
        this: *const HostApi, path: *const u8, path_len: usize,
    ) -> AbiError,
    pub register_host_contract: unsafe extern "C" fn(
        this: *const HostApi,
        interface: *const HostContractInterface,
    ) -> AbiError,
    pub register_loader: unsafe extern "C" fn(
        this: *const HostApi, runtime_name: StringView, loader_ptr: *mut c_void,
    ) -> AbiError,
    pub get_last_error: unsafe extern "C" fn(
        this: *const HostApi, buf: *mut u8, buf_len: usize,
    ) -> usize,
    pub get_error_len: unsafe extern "C" fn(this: *const HostApi) -> usize,
    pub call_guest_method: unsafe extern "C" fn(
        this: *const HostApi,
        instance: GuestContractInstance,
        fn_id: u32,
        args: *const c_void,
        out: *mut c_void,
        arena: *mut CallArena,
    ) -> AbiError,
    pub get_extension: unsafe extern "C" fn(
        this: *const HostApi, extension_id: u32,
    ) -> *const (),
}
```

All functions use the self-passing pattern — the first parameter is always the
`HostApi` pointer itself. Call `(iface.register_guest_contract)(host, &MY_DESCRIPTOR, &MY_VTABLE)` for each contract
your plugin implements.

---

### `GuestContractHandle`

Opaque handle to another loaded plugin. Validated by the host on every use.

```rust
#[repr(C)]
pub struct GuestContractHandle {
    pub index:      u32,
    pub generation: u32,
}
```

A zeroed `GuestContractHandle` (`{ index: 0, generation: 0 }`) is the sentinel "not found" value.

---

### `BundleInitContext`

Passed to `polyplug_init`. Contains `bundle_path` — the absolute path to the bundle
directory on disk.

> **Do not store the raw pointer.** Copy the string value if you need it after init.

---

### `FnPtr`

Wrapper around `*const ()` that implements `Sync` and `Send`, enabling function pointers
in `static` arrays.

```rust
// Build your vtable function array:
static MY_FNS: [FnPtr; 2] = [
    FnPtr(my_first_fn  as *const ()),
    FnPtr(my_second_fn as *const ()),
];
```

---

### `GuestError`

High-level error type for use in generated trait implementations.

```rust
pub struct GuestError {
    pub code:    AbiErrorCode,
    pub message: String,
}
```

Implements `std::error::Error` and `Display`. Returned from generated wrapper code when
an ABI call returns a non-zero code.

---

## ABI Error Codes

The `AbiErrorCode` enum defines all possible return codes:

```rust
pub enum AbiErrorCode: u32 {
    Ok = 0,                    // Success
    Generic = 1,               // Unclassified error
    BufferTooSmall = 2,        // Output buffer too small
    Panic = 3,                 // Plugin panicked
    NotFound = 4,              // Plugin or contract not found
    StaleHandle = 5,           // Handle refers to unloaded plugin
    FunctionNotAvailable = 6,  // Function index out of range
    DuplicateProvider = 7,     // Contract already registered
    InvalidPointer = 8,        // Null or invalid pointer
    // Host contract errors (100+):
    HostContractNotFound = 100,
    HostContractVersionMismatch = 101,
    HostContractCallFailed = 102,
}
```

Use `AbiErrorCode::Ok` for success, `AbiErrorCode::Generic` for errors.

---

## Hash Utilities

Contract IDs are FNV-1a 64-bit hashes of `"name@major_version"`.
`polyplugc` generates these for you; use `contract_id()` at runtime to verify.

```rust
use polyplug_guest::contract_id;

// Verify the compile-time constant matches the runtime hash:
assert_eq!(contract_id("my.contract", 1), MY_CONTRACT_ID);
```

| Function | Description |
|---|---|
| `contract_id(name: &str, major: u64) -> u64` | FNV-1a of `"name@major"` |
| `bundle_id(name: &str) -> u64` | FNV-1a of a bundle name |
| `extension_id(name: &str) -> u32` | FNV-1a (32-bit) of an extension name |

---

## Allocator API

All memory that **crosses the plugin/host boundary** must use the host allocator,
reached through the `alloc` / `free` function-pointer fields of the `HostApi`
the host stored during `polyplug_init`. There are no `polyplug_host_alloc` /
`polyplug_host_free` C exports — allocation always flows through the stored interface.
Never return heap-allocated data from your Rust allocator — the host cannot free it.

For strings, prefer the `alloc_string` helper, which allocates through `host.alloc`:

```rust
use polyplug_guest::{alloc_string, StringView};

// Allocate a host-owned StringView from a &str:
let sv: StringView = alloc_string("hello")?;
// sv.ptr points to host-allocated memory; the host frees it via
// host.free(host, sv.ptr, sv.len, 1) when done.
```

For raw buffers, call the stored interface directly:

```rust
use polyplug_guest::get_host_vtable;

let host = get_host_vtable();
// SAFETY: host was stored during polyplug_init and is non-null in plugin code.
let ptr: *mut u8 = unsafe { ((*host).alloc)(host, 64, 8) };
if ptr.is_null() {
    // allocation failed
}
// Free it through the same interface:
unsafe { ((*host).free)(host, ptr, 64, 8) };
```

**Rules:**
- A plugin must **never free** memory it did not allocate.
- Never place cross-boundary data on the Rust managed heap (`Box`, `Vec`, `String`).
- For string outputs, allocate via `alloc_string` (or `host.alloc`), copy bytes in,
  return a `StringView` pointing to that allocation.

---

## Implementing a Contract — Step by Step

### Step 1: Compute the contract ID

`polyplugc` generates the constant for you. For hand-written plugins:

```rust
// FNV-1a-64 of "pipeline.transformer@1"
const TRANSFORMER_CONTRACT_ID: u64 = 0x0E3044133E12EB05;
```

Verify at startup with `contract_id("pipeline.transformer", 1)`.

### Step 2: Define your ABI types

Declare `#[repr(C)]` structs matching the contract's argument and return types exactly.

```rust
#[repr(C)]
pub struct DataRecord {
    pub name:  StringView,
    pub value: StringView,
    pub count: u32,
}
```

### Step 3: Implement your function with the generic dispatch signature

Every function in the vtable has the signature `extern "C" fn(*const (), *mut ()) -> AbiError`.
Cast the opaque pointers to your concrete types inside.

```rust
extern "C" fn plugin_transform(args: *const (), out: *mut ()) -> AbiError {
    // SAFETY: host guarantees args → DataRecord and out → DataRecord per ABI contract.
    unsafe { do_transform(args as *const DataRecord, out as *mut DataRecord) }
}
```

### Step 4: Declare static vtable and descriptor

```rust
static TRANSFORM_FNS: [FnPtr; 1] = [FnPtr(plugin_transform as *const ())];

static TRANSFORM_VTABLE: GuestContractInterface = GuestContractInterface {
    contract_id:      TRANSFORMER_CONTRACT_ID,
    contract_version: Version { major: 1, minor: 0, patch: 0 },
    dispatch_type:    DispatchType::Native,
    create_instance:  my_create_instance,
    destroy_instance: my_destroy_instance,
    dispatch:         DispatchMechanisms {
        native: NativeDispatch {
            function_count: 1,
            functions:      TRANSFORM_FNS.as_ptr(),
        },
    },
};

static TRANSFORM_DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    name:          StringView { ptr: b"my_transformer".as_ptr(), len: 14 },
    contract_name: StringView { ptr: b"pipeline.transformer".as_ptr(), len: 20 },
    version:       Version { major: 1, minor: 0, patch: 0 },
};
```

### Step 5: Export the two mandatory ABI symbols

```rust
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    POLYPLUG_ABI_VERSION
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_init(host: *const HostApi) -> AbiError {
    if host.is_null() {
        return AbiError { code: AbiErrorCode::Generic, message: StringView::null() };
    }
    // SAFETY: host is non-null and provided by the host per ABI contract.
    let iface: &HostApi = unsafe { &*host };
    // SAFETY: register_guest_contract is a valid function pointer. TRANSFORM_DESCRIPTOR and
    // TRANSFORM_VTABLE are 'static.
    unsafe {
        (iface.register_guest_contract)(host, &TRANSFORM_DESCRIPTOR, &TRANSFORM_VTABLE)
    }
}
```

---

## Full Example

The canonical hand-written native Rust plugin lives at
[`examples/guests/native/src/lib.rs`](../../examples/guests/native/src/lib.rs).

It implements the `pipeline.transformer` contract — uppercasing the `name` field and
appending `" (transformed)"` to the `value` field of a `DataRecord`.

Key points demonstrated:

- **Mirroring ABI types inline** (required for a standalone `cdylib` with no
  `polyplug` dependency)
- **Safe UTF-8 decoding** of `StringView` data
- **Proper error returns** instead of panics
- **Static vtable construction** with `FnPtr`
- **`// SAFETY:` comments** on every `unsafe` block
- **`polyplug_init`** null-checking the `host` pointer before dereferencing

```
examples/guests/native/
├── Cargo.toml        # crate-type = ["cdylib"], no polyplug dep
├── manifest.toml     # bundle metadata
└── src/
    └── lib.rs        # full hand-written plugin implementation
```

---

## manifest.toml — Declaring Your Bundle

Every plugin bundle needs a `manifest.toml` alongside the compiled `.so`:

```toml
bundle_name = "my_plugin"
runtime     = "native"
version     = "1.0"
file        = "libmy_plugin.so"
provides    = ["pipeline.transformer"]

[function_count]
"pipeline.transformer@1" = 1
```

| Field | Description |
|---|---|
| `bundle_name` | Unique name for this bundle |
| `runtime` | Must be `"native"` for Rust/C/C++ plugins |
| `version` | Bundle version string |
| `file` | Filename of the compiled shared library |
| `provides` | List of contract names this bundle implements |
| `[function_count]` | Number of functions per contract (at `"name@major"`) |

---

## Building

```bash
# Debug build
cargo build

# Release build (recommended for distribution)
cargo build --release

# Copy to your bundle directory
cp target/release/libmy_plugin.so /path/to/bundle/
```

The output is a native shared library. File names by platform:

| Platform | Output file |
|---|---|
| Linux | `libmy_plugin.so` |
| macOS | `libmy_plugin.dylib` |
| Windows | `my_plugin.dll` |

Place the compiled library alongside its `manifest.toml` in your bundle directory.

---

## Bundle layout

Assemble the bundle directory yourself:

```
dist/my-plugin/
├── manifest.toml          # emitted by `generate` (carries the precomputed bundle_id)
└── libmy_plugin.so        # the cdylib you compiled (.dylib on macOS, .dll on Windows; runtime = "native")
```

Validate the assembled directory before shipping:

```bash
polyplugc validate --bundle-dir dist/my-plugin/
```

---

## Error Handling

**Never use `.unwrap()` in plugin code.** Return `AbiError` with a non-zero code instead.

```rust
// FORBIDDEN in plugin code
let s = std::str::from_utf8(bytes).unwrap();

// CORRECT — return an AbiError on failure
let s: &str = match std::str::from_utf8(bytes) {
    Ok(s) => s,
    Err(_) => {
        return AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView {
                ptr: b"invalid UTF-8".as_ptr(),
                len: 13,
            },
        };
    }
};
```

`AbiError.message` is **always** a static or runtime-owned string. The receiver
**never** frees it — a bare `StringView` carries no allocation provenance, so the
host cannot and does not free `message.ptr`. Point it at
a `'static` byte string, as above. If you need rich, allocated error detail,
expose it through `get_last_error` rather than through `AbiError.message`.

`AbiError.code` is a raw `u32`, not the `AbiErrorCode` enum: plugins are
untrusted and may return any 32-bit value. Construct it with
`AbiErrorCode::Variant as u32` and interpret a received code with
`AbiErrorCode::from_u32`.

---

## Memory Rules

| Rule | Detail |
|---|---|
| Cross-boundary allocations | Must use the stored `HostApi` `alloc` / `free` fields |
| Plugin-side allocations | May use Rust allocator, but must NOT cross the boundary |
| Returned strings / buffers | Must be allocated via `alloc_string` / `host.alloc` |
| `StringView` (input) | Borrowed — do NOT free; valid only for the duration of the call |
| `Buffer` (output) | Owned — caller frees with `host.free(host, ptr, cap, align)` |
| `AbiError.message` (output) | Static or runtime-owned; receiver NEVER frees it |

---

## More Examples

- **`examples/guests/native/`** — Hand-written Rust native plugin (`pipeline.transformer`)
- **`examples/guests/`** — Guest plugins in C++, C#, Python, Lua, and JavaScript
- **`examples/hosts/`** — Host runtimes that load polyplug bundles
- **`examples/api.toml`** — API definition used by `polyplugc` for codegen
- **`examples/contract_ids.txt`** — Registry of contract IDs for the example contracts
