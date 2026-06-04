# Polyplug ABI Architecture

## Terminology Note

This document uses the following terminology (current as of v1.1):
- **HostInterface**: The runtime's ABI table provided to guests (a `#[repr(C)]` struct of function pointers)
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
AbiError polyplug_init(const HostInterface* host, const BundleInitContext* ctx);
```
**Called by:** Host immediately after dlopen
**Parameters:**
- `host`: The `HostInterface` function table; the plugin registers by calling `host->register_contract(host, &descriptor, &interface)`
- `ctx`: Context containing bundle_id and bundle_path
**Purpose:** Plugin constructor - registers contracts with the runtime

### BundleInitContext
```c
typedef struct {
    StringView bundle_path;      // Absolute path to plugin directory
    uint32_t host_abi_version;   // Host's ABI version for negotiation (Option C)
} BundleInitContext;
```

## Host ABI (libpolyplug.so Exports)

The runtime exports functions for host applications to call.

### Runtime Lifecycle
```c
// Create a new runtime instance
OpaqueRuntime* polyplug_runtime_create(void);

// Destroy a runtime instance
void polyplug_runtime_destroy(OpaqueRuntime* rt);
```

### Plugin Loading
```c
// Load a plugin bundle
uint32_t polyplug_runtime_load_bundle(
    OpaqueRuntime* rt,
    const uint8_t* path,
    size_t path_len
);

// Reload a plugin bundle (hot-reload)
uint32_t polyplug_runtime_reload_bundle(
    OpaqueRuntime* rt,
    const uint8_t* path,
    size_t path_len
);
```

### Plugin Discovery
```c
// Find plugin by contract
uint64_t polyplug_runtime_find_by_contract(
    OpaqueRuntime* rt,
    uint64_t contract_id,
    uint32_t min_version
);

// Find plugin by bundle + contract
uint64_t polyplug_runtime_find_by_bundle(
    OpaqueRuntime* rt,
    uint64_t bundle_id,
    uint64_t contract_id,
    uint32_t min_version
);

// Find all plugins matching contract
size_t polyplug_runtime_find_all_by_contract(
    OpaqueRuntime* rt,
    uint64_t contract_id,
    uint32_t min_version,
    uint64_t* out,
    size_t out_cap
);
```

### Plugin Resolution
```c
// Resolve handle to interface
OpaquePluginGuard* polyplug_runtime_resolve_plugin(
    OpaqueRuntime* rt,
    uint64_t packed_handle
);

// Release guard
void polyplug_runtime_plugin_release(OpaquePluginGuard* guard);

// Get interface pointer
const void* polyplug_runtime_plugin_interface(OpaquePluginGuard* guard);
```

### Error Handling
```c
// Get last error message
size_t polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);

// Get error message length
size_t polyplug_runtime_error_message_len(void);
```

### Loader Registration
```c
// Register custom loader
uint32_t polyplug_runtime_register_loader(
    OpaqueRuntime* rt,
    void* loader_ptr
);
```

## Execution Flow

```
Host Application
    │
    ▼
polyplug_runtime_create() ──► Runtime Instance
    │
    ▼
polyplug_runtime_load_bundle()
    │
    ├── dlopen(plugin.so)
    ├── dlsym(polyplug_abi_version) ──► Check version
    ├── dlsym(polyplug_init)
    │
    ▼
Call: polyplug_init(host, ctx)
    │
    ├── Plugin builds interfaces
    ├── Plugin calls host->register_contract(host, &descriptor, &interface)
    └── Interfaces stored in RuntimeStore
    │
    ▼
polyplug_runtime_find_by_contract() ──► Get handle
    │
    ▼
polyplug_runtime_resolve_plugin() ──► Get interface
    │
    ▼
Call plugin functions via interface
    │
    ▼
polyplug_runtime_destroy()
```

## ABI Stability

The core ABI is frozen per §7 of AGENTS.md:
- `HostInterface` layout cannot change
- `BundleInitContext` can add fields but not remove
- `polyplug_init` signature is fixed (2 params)
- All additions go through extension system

## Future Extensions

New functionality should use:
1. Host contract interfaces resolved via `HostInterface.get_host_contract`
2. New fields in `BundleInitContext` (backward compatible)
3. New host ABI functions (doesn't break plugins)
