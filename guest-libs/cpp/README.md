# polyplug C++ Guest Library

Header-only C++ binding for writing native polyplug guest plugins.

## Usage

Add `guest-libs/cpp` to your compiler's include path and include the single convenience header:

```cpp
#include <polyplug_guest.hpp>
```

Then define your plugin entry point with the `POLYPLUG_GUEST_MAIN` macro:

```cpp
#include <polyplug_guest.hpp>

// Your function implementations ...
static AbiError my_fn(const void* args, void* out) noexcept { ... }

// Static vtable (must outlive the runtime)
static void* const MY_FNS[] = { reinterpret_cast<void*>(&my_fn) };
static PluginVTable MY_VTABLE = {
    0xMY_CONTRACT_IDULL,  // polyplug::fnv1a_contract_id("my.contract", 1)
    0u,                   // contract_version: (minor << 16 | patch)
    1u,                   // function_count
    MY_FNS
};
static const PluginDescriptor MY_DESCRIPTOR = {
    StringView{ reinterpret_cast<const uint8_t*>("my-plugin"), 9U },
    StringView{ reinterpret_cast<const uint8_t*>("my.contract"), 11U },
    1u, 0u, 0u  // major, minor, patch
};

// ABI version export (required)
extern "C" uint32_t polyplug_abi_version() { return POLYPLUG_ABI_VERSION; }

// Plugin entry point
POLYPLUG_GUEST_MAIN {
    return registrar->register_plugin(registrar, &MY_DESCRIPTOR, &MY_VTABLE);
}
```

## Build

```bash
g++ -std=c++20 -fPIC -shared -I path/to/guest-libs/cpp \
    my_plugin.cpp -o libmy_plugin.so
```

## Headers

| Header | Contents |
|---|---|
| `polyplug_guest.hpp` | Single-include entry point (pulls in all three below) |
| `polyplug/abi.hpp` | ABI structs, error codes, allocator declarations |
| `polyplug/contract.hpp` | `polyplug::Contract` abstract base class |
| `polyplug/guest.hpp` | `operator new/delete` overrides + `POLYPLUG_GUEST_MAIN` macro |

## Key Types

- **`StringView`** — Non-owning UTF-8 string (`ptr` + `len`). Never null-terminated.
- **`Buffer`** — Owning byte buffer allocated via `polyplug_host_alloc`.
- **`AbiError`** — Return type for all ABI functions (`code == ABI_OK` on success).
- **`PluginVTable`** — Function table registered for a contract.
- **`PluginDescriptor`** — Plugin metadata (name, contract, version).
- **`HostVTable`** — Host capabilities (alloc, find, resolve, extensions).

## Memory

All cross-boundary allocations go through `polyplug_host_alloc` / `polyplug_host_free`.
Including `polyplug/guest.hpp` globally overrides `operator new/delete` to route through the host allocator — this is intentional.

## Computing Contract IDs

```cpp
#include <polyplug/abi.hpp>

constexpr uint64_t MY_ID = polyplug::fnv1a_contract_id("my.contract", 1);
```

See `examples/guests/cpp/` for complete working examples.
