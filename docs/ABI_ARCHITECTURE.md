# Polyplug ABI Architecture

## Terminology Note

This document uses the following terminology (current as of v1.1):
- **HostApi**: The runtime's ABI table provided to guests (a `#[repr(C)]` struct of function pointers)
- **GuestContractInterface**: The interface struct a plugin provides for the host to call
- **Host Contract**: A contract provided by the host to plugins
- **Guest Contract**: A contract implemented by plugins

## Overview

Polyplug uses a dual-ABI system where both the **host** (runtime) and **guest** (plugins) export C functions across the FFI boundary.

## Plugin ABI (Guest Exports)

Plugins are dynamic libraries (`.so`, `.dll`, `.dylib`) that export two functions:

### `polyplug_abi_version`
```c
uint32_t polyplug_abi_version(void);
```
**Called by:** Host during plugin loading
**Returns:** ABI version (currently `1`)
**Purpose:** Version sentinel to ensure compatibility

### `polyplug_init`
```c
AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx);
```
**Called by:** Host immediately after dlopen
**Parameters:**
- `host`: The `HostApi` function table; the plugin registers by calling `host->register_guest_contract(host, &descriptor, &interface)`
- `ctx`: Context containing bundle_id and bundle_path
**Purpose:** Plugin constructor - registers contracts with the runtime

### BundleInitContext
```c
typedef struct {
    uint64_t   bundle_id;    // Bundle ID for dependency enforcement during init
    StringView bundle_path;  // Absolute canonical path to the bundle directory
} BundleInitContext;
```
24 bytes total: `bundle_id` (8) + `bundle_path` (16).

## Host ABI (libpolyplug Exports)

The runtime exports exactly **two** `#[no_mangle]` C symbols, both runtime
lifecycle entry points. Every other operation — including cross-boundary
allocation (load/reload, discovery, resolution, registration, error handling,
and `alloc` / `free`) — is reached through the function-pointer fields of the
`HostApi` returned by `polyplug_runtime_create`, not through additional
C exports.

### Runtime Lifecycle
```c
// Create a new runtime instance. Pass NULL for config to use defaults.
// Returns a HostApi* that exposes all runtime operations.
const HostApi* polyplug_runtime_create(const void* config);

// Destroy a runtime instance. Must be called exactly once per handle returned
// by polyplug_runtime_create. Calling it more than once, or concurrently with
// itself on the same handle, is undefined behavior — the handle is freed, same
// as C free(); the HostApi pointer is dangling afterwards and must not be used.
void polyplug_runtime_destroy(const HostApi* host);
```

`config` points at a `RuntimeConfig` (`#[repr(C)]`, **48 bytes, align 8**):
`compatibility: Compatibility` (u32, offset 0), `hot_reload_enabled: bool`
(offset 4), `on_reload` callback (offset 8), `on_reload_user_data` (offset 16),
`log` callback (offset 24), `log_user_data` (offset 32), and `log_max_level`
(u32, offset 40). The `log` callback —
`fn(user_data, level: u32, scope: StringView, message: StringView)` — receives
every runtime diagnostic at or below `log_max_level` (`LogLevel { Error = 1,
Warn = 2, Info = 3, Debug = 4, Trace = 5 }`); when null, Error/Warn messages go
to stderr and `log_max_level` is ignored. The callback may run on any thread,
must not re-enter the runtime, and the `StringView`s are only valid for the
duration of the call. The by-value `StringView` parameters are deliberate (hot
path, no copies); LuaJIT FFI callbacks cannot receive structs by value, so the
Lua host SDK installs the `polyplug_lua_log_trampoline` exported by the
polyplug_lua loader cdylib as `log` and carries a scalar-callback
`PolyplugLuaLogBridge` in `log_user_data` (see
`crates/polyplug_lua/src/ffi.rs`). The `on_reload` callback —
`fn(user_data, phase: *const ReloadPhase)` — receives a **const pointer** to a
`ReloadPhase` whose `ReloadPhaseType` is one of `Preparing = 0`, `Reloaded = 1`,
`Failed = 2`, or `Unloading = 3` (fired before a bundle is invalidated on unload).
The pointer is always non-null; the pointee (and the `StringView`s inside it) is
valid only for the duration of the call — copy to retain. `reason` is the null
view unless `phase_type == Failed`.

### Cross-Boundary Allocator (via HostApi fields)
```c
// Allocate memory that crosses the plugin/host boundary.
// Returns NULL for size == 0 or invalid alignment.
uint8_t* host->alloc(const HostApi* host, size_t size, size_t align);

// Free memory previously allocated by host->alloc.
// Must pass the SAME size and align used for the allocation.
void host->free(const HostApi* host, uint8_t* ptr, size_t size, size_t align);
```

### All Other Operations (via HostApi fields)

`polyplug_runtime_create` returns a pointer to `HostApi`, a `184`-byte
`#[repr(C)]` struct: one opaque runtime pointer plus 21 function-pointer fields
(`call_guest_method` at offset 136, `unload_bundle` at offset 144, `log` at
offset 152, `create_guest_instance` at offset 160, `destroy_guest_instance` at
offset 168) followed by a trailing `reserved: *const c_void` data pointer at
offset 176 (producers set null; consumers must not read it). Host applications
and plugins call these fields using the self-passing pattern, e.g.
`host->load_bundle(host, path, path_len)`.
The fields cover bundle lifecycle (`load_bundle`, `reload_bundle`, `unload_bundle`),
contract discovery (`find_guest_contract`, `find_all_guest_contracts`,
`resolve_guest_contract`), instance lifecycle (`create_guest_instance`,
`destroy_guest_instance`), runtime-mediated dispatch (`call_guest_method`),
registration (`register_guest_contract`, `register_host_contract`,
`register_loader`), and error handling (`get_last_error`, `get_error_len`),
among others.

## Execution Flow

```
Host Application
    │
    ▼
polyplug_runtime_create() ──► HostApi* (Runtime Instance)
    │
    ▼
host->load_bundle(host, path, len)
    │
    ├── dlopen(plugin.so)
    ├── dlsym(polyplug_abi_version) ──► Check version
    ├── dlsym(polyplug_init)
    │
    ▼
Call: polyplug_init(host, ctx)
    │
    ├── Plugin builds interfaces
    ├── Plugin calls host->register_guest_contract(host, &descriptor, &interface)
    └── Interfaces stored in RuntimeStore
    │
    ▼
host->find_guest_contract(host, contract_id, ver) ──► Get handle
    │
    ▼
host->resolve_guest_contract(host, handle) ──► Get interface
    │
    ▼
Call plugin functions via interface
    │
    ▼
polyplug_runtime_destroy(host)
```

## ABI Stability

The core ABI freezes at v1.0 per §7 of CLAUDE.md. The project is currently pre-1.0
(no public release yet), so ABI-visible changes are still permitted with explicit
owner approval. At and after v1.0:
- `HostApi` layout cannot change
- `BundleInitContext` layout cannot change (no field additions or removals)
- `polyplug_init` signature is fixed (2 params)
- All additions go through the host/guest contract model

## Forward Compatibility

New functionality should use host contract interfaces resolved via
`HostApi.get_host_contract`. The trailing `reserved: *const c_void` pointer
(offset 160) is the only sanctioned post-freeze expansion slot; producers set
it to null, consumers must not read it.
