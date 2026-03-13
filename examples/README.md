# polyplug Examples

This directory contains the canonical examples for the **polyplug** plugin runtime. These examples demonstrate how to build hosts and guest plugins in various supported languages.

## Directory Structure

- **`hosts/`**: Host runtimes that load and execute polyplug bundles.
- **`guests/`**: Guest plugins implementing specific contracts.
- **`abi_types.md`**: Canonical reference for the `DataRecord` ABI type used by these examples.
- **`build_guests.sh`**: Master build script for all guest plugins.
- **`contract_ids.txt`**: Registry of contract IDs used across these examples.
- **`api.toml`**: The API definition used by `polyplugc` to generate bindings.

## Supported Languages

We provide examples for 6 major languages, totaling 6 hosts and 12 guest plugins.

### Hosts
Available in `examples/hosts/`:
- **Rust**: The reference host implementation.
- **C++**: High-performance native host.
- **C#**: .NET integration.
- **Python**: Scripting integration via `ctypes`.
- **Lua**: Fast scripting via LuaJIT FFI.
- **JavaScript**: Deno and QuickJS support.

### Guests
Available in `examples/guests/`:
- **Rust**: `decoder`, `encoder`
- **C++**: `transformer`, `validator`
- **C#**: `reporter`, `logger`
- **Python**: `analyzer`, `filter`
- **Lua**: `processor`, `mapper`
- **JavaScript**: `fetcher`, `parser`

## JavaScript Support

polyplug provides **two distinct JavaScript integrations** that serve different roles. Understanding the difference is essential before using either.

### js_quickjs — For Writing Guest Plugins

`js_quickjs` uses the [QuickJS](https://bellard.org/quickjs/) engine (a pure-C, embeddable JavaScript VM) to execute guest plugins written in JavaScript.

- **Use case:** Write a guest plugin in JavaScript that can be loaded by *any* polyplug host (Rust, C++, C#, Python, Lua, or Deno).
- **Shared library:** `libpolyplug_js.so`
- **Loader function:** `register_js_loader` / `polyplug_js_loader_create`
- **No TLS issues:** QuickJS is pure C with no thread-local storage requirements. It links cleanly as a `cdylib` and loads into any host process.

**When to use:** You have a JavaScript plugin and want it to be loadable by all host languages.

```
guests/js/fetcher/     ← QuickJS guest (loaded by any host via libpolyplug_js.so)
guests/js/parser/      ← QuickJS guest (loaded by any host via libpolyplug_js.so)
```

---

### js_deno — For Writing Hosts in Deno

`js_deno` uses [Deno](https://deno.land/) (V8-based) to write the *host* runtime itself in TypeScript/JavaScript. The Deno host can load guest plugins written in all supported languages (Rust, C++, C#, Python, Lua, and QuickJS JavaScript).

- **Use case:** Write a polyplug host application in Deno/TypeScript.
- **Host library:** `host-libs/js/polyplug.ts`
- **Can load guests:** Rust, C++, C#, Python, Lua, QuickJS JavaScript
- **Cannot load:** Deno/V8 guests (see limitation below)

**When to use:** You are building a host application and want to write it in TypeScript/Deno.

```
hosts/js/              ← Deno host (TypeScript, loads QuickJS guests among others)
```

---

### The V8 TLS Limitation — Why js_deno Guests Don't Exist

> **TL;DR:** V8 cannot be loaded as a shared library on Linux. There is no `libpolyplug_js_deno.so` and no `registerDenoLoader`. This is a hard platform limitation, not a design choice.

**The technical reason:**

V8 (the JavaScript engine used by Deno and Chrome) makes heavy use of **thread-local storage (TLS)** — a mechanism that stores per-thread data at a fixed offset from a thread-control register. When V8 is compiled, it reserves specific TLS slots for its internal state.

On Linux, when you load a shared library (`cdylib` / `.so`) via `dlopen` at runtime, the dynamic linker **cannot** honour pre-allocated TLS slots for that library. TLS for dynamically loaded libraries must be allocated lazily, and V8 does not support this mode. The result is a linker error or a runtime crash when the TLS slots overflow.

In contrast, **QuickJS uses no TLS**. It allocates all state on the heap and passes it explicitly, making it trivially loadable as a shared library.

**Consequence for polyplug:**

| Scenario | Works? | Why |
|----------|--------|-----|
| QuickJS guest loaded by any host | ✅ Yes | QuickJS has no TLS requirements |
| Deno host loading QuickJS guests | ✅ Yes | V8 runs in the main executable, not a `.so` |
| Deno host loading Rust/C/C# guests | ✅ Yes | Native loaders link cleanly as `.so` |
| Deno/V8 guest loaded by any host | ❌ No | V8 cannot be a `cdylib` on Linux |

**If you need JavaScript guest plugins:** Use QuickJS (`libpolyplug_js.so`). It is ECMAScript 2023-compatible and handles the vast majority of use cases. For performance-sensitive code, consider a Rust or C guest instead.

---

## Building the Examples

### Guest Plugins
To build all guest plugins across all languages, run the master build script from the repository root:

```bash
./examples/build_guests.sh
```

You can also build specific languages:

```bash
./examples/build_guests.sh rust cpp
```

Individual language build scripts are located at `examples/guests/<lang>/build.sh`.

### Host Runtimes
Host runtimes are typically built using their respective language's standard build tools (e.g., `cargo build` for Rust, `cmake` for C++). See the README within each host directory for specific instructions.

## ABI Reference
All examples in this directory use a shared `DataRecord` structure for data exchange. For detailed memory layouts and language-specific struct definitions, see [abi_types.md](./abi_types.md).

---

## Loader FFI

polyplug's runtime supports multiple guest languages through a pluggable **loader** system. Every loader — including the native loader for Rust, C, and C++ guests — must be **explicitly registered** before loading bundles. No loader is built into the runtime automatically.

> **Migration note (pre-1.0):** Earlier versions of polyplug loaded native (Rust/C/C++) guests automatically without any loader registration. This implicit behaviour has been removed. You must now call `register_native_loader` explicitly, just like any other loader.

### Available Loaders

| Loader | Shared Library | Guest Language |
|--------|---------------|----------------|
| `native` | `libpolyplug_native.so` | Rust / C / C++ (compiled native code) |
| `dotnet` | `libpolyplug_dotnet.so` | C# / .NET (any CLR-compatible language) |
| `python` | `libpolyplug_python.so` | Python 3.x (via CPython) |
| `lua` | `libpolyplug_lua.so` | Lua (via LuaJIT) |
| `js` | `libpolyplug_js.so` | JavaScript (via QuickJS) |

> **Note:** A `js_deno` (V8-based) loader is **not available** as a linkable shared library. V8's thread-local storage requirements make it impossible to build `libpolyplug_js_deno.so` as a `cdylib` on Linux. JavaScript guest support is provided exclusively through QuickJS (`libpolyplug_js.so`). The Deno host library (`host-libs/js/`) is for writing *hosts* in Deno/TypeScript, not for loading Deno guest plugins.

---

### How Loader Registration Works

The loader FFI follows a two-step pattern:

1. **Create** a loader by calling `polyplug_<lang>_loader_create(&config)` from the corresponding shared library. This returns an opaque `void*` pointer.
2. **Register** the loader into the runtime by calling `polyplug_runtime_register_loader(rt, loader_ptr)`. This transfers ownership of the loader to the runtime — do **not** free it afterward.

Once registered, the runtime will use the loader automatically when loading bundles that contain guests in that language.

---

### C++

Include `<polyplug/loaders.hpp>` and link against the desired loader libraries.

```cpp
#include <polyplug/runtime.hpp>
#include <polyplug/loaders.hpp>

// Build: -lpolyplug -lpolyplug_native -lpolyplug_dotnet -lpolyplug_python -lpolyplug_lua -lpolyplug_js

auto rt = polyplug::Runtime::builder()
    .plugin_dir("/usr/lib/myplugins")
    .build();

// Register native loader for Rust/C/C++ guest plugins (no longer implicit — must be explicit)
polyplug::register_native_loader(rt.handle());

// Register other guest loaders as needed
polyplug::register_dotnet_loader(rt.handle(), "10.0");  // min .NET version
polyplug::register_python_loader(rt.handle(), "3.11");  // min Python version
polyplug::register_lua_loader(rt.handle());
polyplug::register_js_loader(rt.handle());

// Now load bundles containing any supported guest language
rt.load_bundle("/path/to/my/plugin_bundle");
```

Each function throws `std::runtime_error` on failure.

---

### C\#

Call the `Register*Loader()` methods on the `Runtime` instance after construction. The C# host library links against `polyplug_native`, `polyplug_dotnet`, `polyplug_python`, `polyplug_lua`, and `polyplug_js` via P/Invoke.

```csharp
using Polyplug;

var rt = Runtime.Builder()
    .PluginDir("/usr/lib/myplugins")
    .Init();

// Register native loader for Rust/C/C++ guest plugins (no longer implicit — must be explicit)
rt.RegisterNativeLoader();

// Register other guest loaders as needed
rt.RegisterDotnetLoader("10.0");  // optional: min .NET framework version
rt.RegisterPythonLoader("3.11"); // optional: min Python version
rt.RegisterLuaLoader();
rt.RegisterJsLoader();

// Now load bundles
rt.LoadBundle("/path/to/my/plugin_bundle");
```

Each method throws `InvalidOperationException` on failure.

---

### Python

Import the loader helpers from `polyplug.loaders` and call them after creating a `Runtime`.

```python
from polyplug import Runtime
from polyplug.loaders import (
    register_native_loader,
    register_dotnet_loader,
    register_python_loader,
    register_lua_loader,
    register_js_loader,
)

rt = Runtime()

# Register native loader for Rust/C/C++ guest plugins (no longer implicit — must be explicit)
register_native_loader(rt)

# Register other guest loaders as needed
register_dotnet_loader(rt, min_framework="10.0")
register_python_loader(rt, min_version="3.11")
register_lua_loader(rt)
register_js_loader(rt)

# Now load bundles
rt.load_bundle("/path/to/my/plugin_bundle")
```

Each function raises `RuntimeError` on failure.

---

### Lua (LuaJIT)

Call the `register_*_loader` functions on the `polyplug` module. Pass the raw `OpaqueRuntime*` cdata pointer obtained from `M._lib.polyplug_runtime_new()` (i.e., the `_ptr` field of a `Runtime` table).

```lua
local polyplug = require("polyplug")
polyplug.load_lib("/usr/local/lib/libpolyplug.so")

local rt = polyplug.Runtime.new()

-- Register native loader for Rust/C/C++ guest plugins (no longer implicit — must be explicit)
-- Pass the internal OpaqueRuntime* cdata pointer
polyplug.register_native_loader(rt._ptr)

-- Register other guest loaders as needed
polyplug.register_dotnet_loader(rt._ptr, { min_framework = "10.0" })
polyplug.register_python_loader(rt._ptr, { min_version = "3.11" })
polyplug.register_lua_loader(rt._ptr)
polyplug.register_js_loader(rt._ptr)

-- Now load bundles
rt:load_bundle("/path/to/my/plugin_bundle")
```

Each function calls `error()` on failure. The loader libraries (`libpolyplug_native.so`, `libpolyplug_dotnet.so`, etc.) are loaded lazily on first use.

---

### JavaScript / Deno (host writing Deno, loading QuickJS guests)

The Deno host library (`host-libs/js/polyplug.ts`) includes exported functions for registering loaders. Pass the library handle and the raw runtime pointer.

```typescript
import {
  openPolyplug,
  runtimeNew,
  registerNativeLoader,
  registerDotnetLoader,
  registerPythonLoader,
  registerLuaLoader,
  registerJsLoader,
} from "./polyplug.ts";

const lib = openPolyplug("/usr/local/lib/libpolyplug.so");
const rt = runtimeNew(lib);

// Register native loader for Rust/C/C++ guest plugins (no longer implicit — must be explicit)
// Note: pass lib and the rt.#ptr (internal pointer) — see Deno host docs
registerNativeLoader(lib, rt_ptr);

// Register other guest loaders as needed
registerDotnetLoader(lib, rt_ptr, "10.0");
registerPythonLoader(lib, rt_ptr, "3.11");
registerLuaLoader(lib, rt_ptr);
registerJsLoader(lib, rt_ptr);   // loads QuickJS guests, NOT Deno guests

// Now load bundles
rt.loadBundle("/path/to/my/plugin_bundle");
```

> **V8 / js\_deno limitation:** There is no `registerDenoLoader` function and no `libpolyplug_js_deno.so`. V8 uses thread-local storage in a way that is incompatible with loading as a `cdylib` on Linux. If you need JavaScript guest plugins, use the QuickJS loader (`registerJsLoader`) instead.

---

### Raw C ABI

If you are integrating from a language not listed above, the raw C ABI for loader registration is:

```c
// From <polyplug/loaders.hpp> or polyplug.h:

// Step 1: create the loader (links against libpolyplug_<lang>.so)
void* polyplug_native_loader_create(const PolyplugNativeConfig* config);
void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig* config);
void* polyplug_python_loader_create(const PolyplugPythonConfig* config);
void* polyplug_lua_loader_create(const PolyplugLuaConfig* config);
void* polyplug_js_loader_create(const PolyplugJsConfig* config);

// Step 2: register with runtime (from libpolyplug.so)
uint32_t polyplug_runtime_register_loader(OpaqueRuntime* rt, void* loader_ptr);
```

Config structs:

```c
// Native loader: no configuration required
struct PolyplugNativeConfig { uint8_t _reserved; };  // set _reserved = 0

// .NET loader: specify minimum framework version
struct PolyplugDotnetConfig {
    const uint8_t* min_framework_ptr;  // UTF-8 string
    size_t         min_framework_len;
};

// Python loader: specify minimum Python version
struct PolyplugPythonConfig {
    const uint8_t* min_version_ptr;    // UTF-8 string
    size_t         min_version_len;
};

// Lua and JS loaders: no configuration required
struct PolyplugLuaConfig { uint8_t _reserved; };  // set _reserved = 0
struct PolyplugJsConfig  { uint8_t _reserved; };  // set _reserved = 0
```

`polyplug_runtime_register_loader` returns `0` on success, non-zero on error. Call `polyplug_last_error` to retrieve the error message.
