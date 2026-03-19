# polyplug C++ Host Library

Header-only C++17 binding for the polyplug plugin runtime. Provides RAII wrappers, type-safe plugin resolution, and loader registration for multiple language runtimes.

## Prerequisites

- **C++17** or later — required for `std::string_view` and move semantics
- **libpolyplug.so** — the core polyplug shared library (compiled separately)
- A C++ compiler with C++17 support (GCC 8+, Clang 7+, MSVC 2019+)

## Finding the Native Library

The C++ host library requires the native `libpolyplug` shared library at runtime. There are three ways to provide it:

### Option 1: Set POLYPLUG_LIB Environment Variable

Set the `POLYPLUG_LIB` environment variable to the path containing the library:

```bash
# Linux/macOS
export POLYPLUG_LIB=/path/to/libpolyplug.so

# Windows (PowerShell)
$env:POLYPLUG_LIB = "C:\path\to\polyplug.dll"
```

### Option 2: Download Automatically with CMake

Enable automatic download from GitHub Releases by setting `POLYPLUG_DOWNLOAD`:

```cmake
# In your CMakeLists.txt before find_package(polyplug)
set(POLYPLUG_DOWNLOAD ON)
set(POLYPLUG_VERSION "0.1.0")  # Optional: specify version

find_package(polyplug REQUIRED)
```

The library will be downloaded to `_native/{platform}-{arch}/` in your project directory.

### Option 3: Manual Download Script

Use the provided download script for CI builds or manual installation:

```bash
# Download latest version (0.1.0)
./download-native.sh

# Download specific version
./download-native.sh 0.1.0
```

The script detects your platform and downloads the appropriate library to `_native/{platform}-{arch}/`.

**Supported platforms:**
- `linux-x64` → `libpolyplug.so`
- `darwin-x64` → `libpolyplug.dylib`
- `darwin-arm64` → `libpolyplug.dylib`
- `win32-x64` → `polyplug-windows-x64.dll`

### Option 4: System Installation

Install the library to a system path (`/usr/lib`, `/usr/local/lib`, etc.) and CMake will find it automatically.

## Quick Start

```cpp
#include <polyplug.hpp>
#include <polyplug/runtime.hpp>

int main() {
    // 1. Create a runtime using the fluent builder
    auto rt = polyplug::Runtime::builder()
        .plugin_dir("/path/to/plugins")
        .build();

    // 2. Load a plugin bundle
    rt.load_bundle("/path/to/my_plugin_bundle");

    // 3. Find a plugin by contract ID
    constexpr uint64_t CONTRACT_ID = 0xCC4232FAB0410D2BULL;
    uint64_t handle = rt.find(CONTRACT_ID, 1);  // min_version = 1

    if (handle == UINT64_MAX) {
        // Plugin not found — handle error
        return 1;
    }

    // 4. Resolve to a PluginGuard with cached vtable
    polyplug::PluginGuard guard = rt.resolve_plugin(handle);
    if (!guard) {
        // Resolution failed
        return 1;
    }

    // 5. Get the cached vtable and call plugin functions
    const auto* vtable = guard.vtable();
    // Cast to your contract-specific vtable type and dispatch
}
```

## Runtime API

### Builder Pattern

The `Runtime` class uses a fluent builder for construction:

```cpp
auto rt = polyplug::Runtime::builder()
    .plugin_dir("/path/to/plugins")      // Optional: set plugin search directory
    .compatibility(0)                    // Optional: compatibility mode flags
    .build();                            // Throws std::runtime_error on failure
```

**Methods:**

- `plugin_dir(std::string_view path)` — Add a directory to the plugin search path (call multiple times for multiple directories)
- `compatibility(uint32_t mode)` — Set compatibility mode flags (default: 0)
- `build()` — Construct the Runtime instance (throws on failure)

### Core Runtime Methods

```cpp
class Runtime {
public:
    // Find a plugin by contract ID and minimum version
    // Returns UINT64_MAX if not found
    uint64_t find(uint64_t contract_id, uint32_t min_version) const noexcept;

    // Resolve a packed handle to a PluginGuard with cached vtable
    PluginGuard resolve_plugin(uint64_t packed_handle) const noexcept;

    // Load a plugin bundle from disk
    // Throws std::runtime_error on failure
    void load_bundle(std::string_view path);

    // Get the underlying runtime handle (for FFI)
    RuntimeHandle handle() const noexcept;
};
```

## PluginGuard API

The `PluginGuard` class provides RAII management of resolved plugins with **zero-overhead vtable access** via caching.

### Key Features

- **Vtable caching** — The vtable pointer is cached at construction time (no FFI call on access)
- **RAII cleanup** — Plugin is automatically released when the guard goes out of scope
- **Move-only** — Guards can be moved but not copied (prevents double-free)
- **Null safety** — Failed resolution creates a null guard (no exceptions)

### Usage Pattern

```cpp
// Resolve plugin and get cached vtable
polyplug::PluginGuard guard = rt.resolve_plugin(handle);

// Check if resolution succeeded
if (!guard) {
    // Handle null guard (resolution failed)
    return;
}

// Get cached vtable — zero FFI overhead
const auto* vtable = guard.vtable();

// Cast to your contract-specific type
struct MyContractVTable {
    int32_t (*add)(int32_t a, int32_t b);
    void (*destroy)();
};

const auto* contract = static_cast<const MyContractVTable*>(vtable);

// Call plugin functions
int32_t result = contract->add(1, 2);
```

### PluginGuard Methods

```cpp
class PluginGuard {
public:
    PluginGuard() noexcept;                              // Null guard
    PluginGuard(RuntimeHandle, uint64_t) noexcept;       // Resolve and cache
    ~PluginGuard() noexcept;                             // RAII release

    const PluginVTable* vtable() const noexcept;         // Cached vtable (no FFI)
    bool is_null() const noexcept;                       // Check if null
    explicit operator bool() const noexcept;             // Boolean conversion

    // Move-only
    PluginGuard(PluginGuard&&) noexcept;
    PluginGuard& operator=(PluginGuard&&) noexcept;

    // Copy disabled
    PluginGuard(const PluginGuard&) = delete;
    PluginGuard& operator=(const PluginGuard&) = delete;
};
```

### Performance: Hot Path Dispatch

The PluginGuard is designed for **zero-overhead dispatch** in hot paths:

```cpp
// Construction: One guard load, one pointer dereference, one indirect call
auto guard = rt.resolve_plugin(handle);  // FFI call happens here
const auto* vtable = guard.vtable();     // NO FFI — returns cached pointer

// Hot path: Direct vtable dispatch (no FFI overhead)
while (running) {
    result = vtable->process(data);  // Pure indirect call — no runtime overhead
}
```

**Why this matters:** The vtable pointer is fetched once during guard construction. Every subsequent `vtable()` call is a simple pointer return — no FFI, no locking, no overhead.

## Loader Package Structure

Polyplug supports loading plugins from multiple language runtimes. **Each loader is a separate package** under the `loaders/` directory:

```
host-libs/cpp/
├── polyplug.hpp              # Main include (includes all headers)
├── polyplug/
│   ├── runtime.hpp           # Runtime and PluginGuard API
│   ├── abi.hpp               # ABI definitions
│   ├── error.hpp             # Error handling
│   └── handle.hpp            # Handle types
└── loaders/
    ├── native/               # Native C/C++ loader
    │   ├── CMakeLists.txt
    │   └── polyplug_loaders_native.hpp
    ├── python/               # Python loader
    │   ├── CMakeLists.txt
    │   └── polyplug_loaders_python.hpp
    ├── lua/                  # LuaJIT loader
    │   ├── CMakeLists.txt
    │   └── polyplug_loaders_lua.hpp
    ├── js/                   # QuickJS loader
    │   ├── CMakeLists.txt
    │   └── polyplug_loaders_js.hpp
    ├── js_deno/              # Deno loader
    │   ├── CMakeLists.txt
    │   └── polyplug_loaders_js_deno.hpp
    └── dotnet/               # .NET loader
        ├── CMakeLists.txt
        └── polyplug_loaders_dotnet.hpp
```

### Loader Registration Pattern

Each loader provides a `register_*` function in the `polyplug::loaders` namespace:

```cpp
#include <polyplug/loaders/native/polyplug_loaders_native.hpp>
#include <polyplug/loaders/python/polyplug_loaders_python.hpp>

// Register native C/C++ loader
polyplug::loaders::register_native(rt);

// Register Python loader with minimum version requirement
polyplug::loaders::register_python(rt, "3.11");

// Register LuaJIT loader
polyplug::loaders::register_lua(rt);

// Register QuickJS loader
polyplug::loaders::register_js(rt);

// Register Deno loader
polyplug::loaders::register_js_deno(rt);

// Register .NET loader with minimum framework version
polyplug::loaders::register_dotnet(rt, "10.0");
```

### Installation Instructions

Each loader is a separate CMake package. Add to your `CMakeLists.txt`:

```cmake
# Find the core polyplug library
find_package(polyplug REQUIRED)

# Find and link specific loaders
find_package(polyplug_loaders_native REQUIRED)
find_package(polyplug_loaders_python REQUIRED)

target_link_libraries(your_app
    PRIVATE
        polyplug::polyplug
        polyplug::loaders_native
        polyplug::loaders_python
)
```

**Include paths:**

```cpp
// Core runtime
#include <polyplug/runtime.hpp>

// Individual loaders (separate packages)
#include <loaders/native/polyplug_loaders_native.hpp>
#include <loaders/python/polyplug_loaders_python.hpp>
#include <loaders/lua/polyplug_loaders_lua.hpp>
#include <loaders/js/polyplug_loaders_js.hpp>
#include <loaders/js_deno/polyplug_loaders_js_deno.hpp>
#include <loaders/dotnet/polyplug_loaders_dotnet.hpp>
```

### Loader-Specific Configuration

Some loaders accept configuration parameters:

**Python loader:**
```cpp
// Require Python 3.11 or later
polyplug::loaders::register_python(rt, "3.11");
```

**.NET loader:**
```cpp
// Require .NET 10.0 or later
polyplug::loaders::register_dotnet(rt, "10.0");
```

**Native, Lua, JS, Deno loaders:**
```cpp
// No configuration required
polyplug::loaders::register_native(rt);
polyplug::loaders::register_lua(rt);
polyplug::loaders::register_js(rt);
polyplug::loaders::register_js_deno(rt);
```

## Error Handling

### Exception-Based Errors

Runtime construction and bundle loading throw `std::runtime_error` on failure:

```cpp
try {
    auto rt = polyplug::Runtime::builder().build();
    rt.load_bundle("/path/to/bundle");
} catch (const std::runtime_error& e) {
    // Handle error: e.what() contains the error message
    std::cerr << "Runtime error: " << e.what() << std::endl;
}
```

### Loader Registration Errors

Loader registration functions throw `std::runtime_error` if creation or registration fails:

```cpp
try {
    polyplug::loaders::register_python(rt, "3.11");
} catch (const std::runtime_error& e) {
    // Handle error: loader creation or registration failed
    std::cerr << "Loader error: " << e.what() << std::endl;
}
```

### Null Guard Pattern

Plugin resolution does **not** throw exceptions. Failed resolution returns a null guard:

```cpp
// Check for null guard using boolean conversion
polyplug::PluginGuard guard = rt.resolve_plugin(handle);
if (!guard) {
    // Resolution failed — handle gracefully
    return;
}

// Or use explicit check
if (guard.is_null()) {
    // Resolution failed
    return;
}
```

### Handle Sentinel

The `find()` method returns `UINT64_MAX` as a sentinel for "not found":

```cpp
uint64_t handle = rt.find(CONTRACT_ID, 1);
if (handle == UINT64_MAX) {
    // Plugin not found — handle error
    return;
}
```

## Hot-Reload Notification

The C++ binding provides a callback mechanism to receive notifications during hot-reload operations. This allows your application to react to plugin reloads, handle stale handles, and manage resources appropriately.

### Registration

Register the callback **before** creating the Runtime instance. The callback is global and applies to all subsequently created runtimes:

```cpp
#include <polyplug/runtime.hpp>

// Register callback before creating runtime
polyplug::Runtime::on_reload([](const polyplug::ReloadPhase& phase) {
    switch (phase.type) {
        case polyplug::ReloadPhaseType::Preparing:
            // Reload is starting, includes retry count
            std::cout << "Preparing to reload bundle " 
                      << polyplug::StringView_to_string(phase.bundle_name)
                      << " (attempt " << phase.retry_count << ")" << std::endl;
            break;
            
        case polyplug::ReloadPhaseType::Reloaded:
            // Successful reload — vtable has been swapped
            std::cout << "Successfully reloaded bundle " 
                      << polyplug::StringView_to_string(phase.bundle_name) << std::endl;
            // Note: PluginGuard instances will re-resolve vtables automatically
            break;
            
        case polyplug::ReloadPhaseType::Failed:
            // Reload failed — includes reason string
            std::cerr << "Failed to reload bundle " 
                      << polyplug::StringView_to_string(phase.bundle_name)
                      << ": " << polyplug::StringView_to_string(phase.reason) << std::endl;
            break;
    }
});

// Now create runtime — callback will be invoked on reloads
auto rt = polyplug::Runtime::builder().build();
```

### ReloadPhase Struct

The `ReloadPhase` struct provides information about the current reload state:

```cpp
struct ReloadPhase {
    ReloadPhaseType type;         ///< Which phase (Preparing, Reloaded, or Failed)
    uint64_t        bundle_id;    ///< Bundle ID (valid for all variants)
    StringView      bundle_name;  ///< Bundle name (valid for all variants)
    uint32_t        retry_count;  ///< Retry count (valid only for Preparing)
    StringView      reason;       ///< Failure reason (valid only for Failed)
};
```

**Field validity by phase:**

| Field | Preparing | Reloaded | Failed |
|-------|-----------|----------|--------|
| `type` | ✓ | ✓ | ✓ |
| `bundle_id` | ✓ | ✓ | ✓ |
| `bundle_name` | ✓ | ✓ | ✓ |
| `retry_count` | ✓ | — | — |
| `reason` | — | — | ✓ |

### ReloadPhaseType Enum

```cpp
enum class ReloadPhaseType : uint32_t {
    Preparing = 0,  ///< Before vtable swap, includes retry count
    Reloaded  = 1,  ///< After successful vtable swap
    Failed    = 2   ///< Reload failed, includes reason string
};
```

### Runtime Configuration

Configure hot-reload behavior using `RuntimeConfig`:

```cpp
#include <polyplug/runtime.hpp>
#include <chrono>

// Configure hot-reload behavior
polyplug::RuntimeConfig config;
config.hot_reload_max_retries = 5;  // Max 5 retry attempts
config.hot_reload_retry_interval = std::chrono::milliseconds(100);  // 100ms between retries
config.hot_reload_abort_on_max_retries = false;  // Keep retrying forever

// Apply configuration before creating runtime
polyplug::Runtime::set_config(config);

// Now create runtime with the configured settings
auto rt = polyplug::Runtime::builder().build();
```

**Configuration fields:**

```cpp
struct RuntimeConfig {
    uint32_t hot_reload_max_retries{3U};           ///< Max retry attempts (0 = infinite)
    std::chrono::milliseconds hot_reload_retry_interval{1000};  ///< Interval between retries
    bool hot_reload_abort_on_max_retries{true};    ///< Abort when max retries exhausted
};
```

### Complete Hot-Reload Example

```cpp
#include <polyplug.hpp>
#include <polyplug/runtime.hpp>
#include <iostream>
#include <chrono>

int main() {
    // 1. Configure hot-reload behavior
    polyplug::RuntimeConfig config;
    config.hot_reload_max_retries = 3;
    config.hot_reload_retry_interval = std::chrono::milliseconds(500);
    config.hot_reload_abort_on_max_retries = true;
    polyplug::Runtime::set_config(config);

    // 2. Register reload notification callback
    polyplug::Runtime::on_reload([](const polyplug::ReloadPhase& phase) {
        std::string name = polyplug::StringView_to_string(phase.bundle_name);
        
        if (phase.type == polyplug::ReloadPhaseType::Preparing) {
            std::cout << "[RELOAD] Preparing " << name 
                      << " (attempt " << phase.retry_count << ")" << std::endl;
        } else if (phase.type == polyplug::ReloadPhaseType::Reloaded) {
            std::cout << "[RELOAD] Successfully reloaded " << name << std::endl;
        } else if (phase.type == polyplug::ReloadPhaseType::Failed) {
            std::string reason = polyplug::StringView_to_string(phase.reason);
            std::cerr << "[RELOAD] Failed to reload " << name 
                      << ": " << reason << std::endl;
        }
    });

    try {
        // 3. Create runtime (callback is now active)
        auto rt = polyplug::Runtime::builder()
            .plugin_dir("/path/to/plugins")
            .build();

        // 4. Load initial bundle
        rt.load_bundle("/path/to/my_plugin");

        // 5. Resolve plugin and use it
        uint64_t handle = rt.find(0xCC4232FAB0410D2BULL, 1);
        if (handle != UINT64_MAX) {
            auto guard = rt.resolve_plugin(handle);
            // Use plugin...
            
            // 6. Later, reload the bundle (callback will be invoked)
            rt.reload_bundle("/path/to/my_plugin_updated");
            
            // 7. Guard automatically re-resolves vtable on next access
            // No stale handle issues — PluginGuard handles hot-reload transparently
        }

    } catch (const std::runtime_error& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}
```

### Memory Safety

**Important:** All string pointers in `ReloadPhase` (`bundle_name`, `reason`) are **borrowed references** from the runtime's internal state. They are valid **only for the duration of the callback invocation**.

```cpp
// CORRECT — use strings within callback scope
polyplug::Runtime::on_reload([](const polyplug::ReloadPhase& phase) {
    std::string name = polyplug::StringView_to_string(phase.bundle_name);  // Copy if needed
    // Use name safely here...
});

// FORBIDDEN — storing borrowed pointers
static std::string_view stored_name;  // DON'T DO THIS
polyplug::Runtime::on_reload([](const polyplug::ReloadPhase& phase) {
    stored_name = polyplug::StringView_as_string_view(phase.bundle_name);  // DANGLING!
});
```

If you need to persist the strings beyond the callback, copy them to `std::string`.

## Memory Management

### RAII Guarantees

All polyplug C++ types use RAII for automatic cleanup:

- **Runtime** — Destroyed automatically when it goes out of scope
- **PluginGuard** — Releases the plugin when destroyed (moved-from guards are safe)
- **No manual cleanup required** — Destructors handle all resource release

```cpp
void use_plugin() {
    auto rt = polyplug::Runtime::builder().build();
    rt.load_bundle("/path/to/bundle");

    auto guard = rt.resolve_plugin(handle);
    const auto* vtable = guard.vtable();

    // Use plugin...

    // Automatic cleanup:
    // 1. guard destructor releases the plugin
    // 2. rt destructor destroys the runtime
    // No manual cleanup needed!
}
```

### Move Semantics

Runtime and PluginGuard support move semantics for efficient transfer:

```cpp
// Move Runtime
auto rt1 = polyplug::Runtime::builder().build();
auto rt2 = std::move(rt1);  // rt1 is now null, rt2 owns the runtime

// Move PluginGuard
auto guard1 = rt.resolve_plugin(handle);
auto guard2 = std::move(guard1);  // guard1 is now null, guard2 owns the plugin
```

**Important:** After a move, the source object is null. Do not use it:

```cpp
auto guard1 = rt.resolve_plugin(handle);
auto guard2 = std::move(guard1);

if (!guard1) {
    // guard1 is null after move — expected!
}
if (guard2) {
    // guard2 now owns the plugin
}
```

## Complete Example

```cpp
#include <polyplug.hpp>
#include <polyplug/runtime.hpp>
#include <loaders/native/polyplug_loaders_native.hpp>
#include <loaders/python/polyplug_loaders_python.hpp>
#include <iostream>

// Define your contract vtable structure
struct MathContractVTable {
    int32_t (*add)(int32_t a, int32_t b);
    int32_t (*multiply)(int32_t a, int32_t b);
    void (*destroy)();
};

int main() {
    try {
        // 1. Create runtime with plugin directory
        auto rt = polyplug::Runtime::builder()
            .plugin_dir("/usr/local/lib/polyplug/plugins")
            .build();

        // 2. Register loaders for different language runtimes
        polyplug::loaders::register_native(rt);
        polyplug::loaders::register_python(rt, "3.11");

        // 3. Load plugin bundles
        rt.load_bundle("/path/to/math_plugin");
        rt.load_bundle("/path/to/utils_plugin");

        // 4. Find and resolve the math plugin
        constexpr uint64_t MATH_CONTRACT = 0xCC4232FAB0410D2BULL;
        uint64_t handle = rt.find(MATH_CONTRACT, 1);

        if (handle == UINT64_MAX) {
            std::cerr << "Math plugin not found" << std::endl;
            return 1;
        }

        // 5. Resolve with cached vtable
        polyplug::PluginGuard guard = rt.resolve_plugin(handle);
        if (!guard) {
            std::cerr << "Failed to resolve math plugin" << std::endl;
            return 1;
        }

        // 6. Cast vtable to contract type
        const auto* math = static_cast<const MathContractVTable*>(guard.vtable());

        // 7. Call plugin functions (zero FFI overhead on vtable access)
        int32_t sum = math->add(10, 20);
        int32_t product = math->multiply(3, 7);

        std::cout << "10 + 20 = " << sum << std::endl;
        std::cout << "3 * 7 = " << product << std::endl;

        // 8. Automatic cleanup via RAII
        // guard releases plugin, rt destroys runtime
    } catch (const std::runtime_error& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}
```

## Build Integration

### CMake Example

```cmake
cmake_minimum_required(VERSION 3.16)
project(my_app LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Option 1: Auto-download from GitHub Releases (recommended for CI)
set(POLYPLUG_DOWNLOAD ON)
set(POLYPLUG_VERSION "0.1.0")

# Option 2: Or set POLYPLUG_LIB environment variable before running cmake
# export POLYPLUG_LIB=/path/to/libpolyplug.so

# Find polyplug and loaders
find_package(polyplug REQUIRED)
find_package(polyplug_loaders_native REQUIRED)
find_package(polyplug_loaders_python REQUIRED)

# Create executable
add_executable(my_app main.cpp)

# Link libraries (polyplug::polyplug target provided by FindPolyplug.cmake)
target_link_libraries(my_app
    PRIVATE
        polyplug::polyplug
        polyplug::loaders_native
        polyplug::loaders_python
)
```

### Using FindPolyplug.cmake Directly

If you're integrating polyplug into an existing CMake project, you can use the `FindPolyplug.cmake` module directly:

```cmake
# Add the cmake module path
list(APPEND CMAKE_MODULE_PATH "/path/to/polyplug/host-libs/cpp/cmake")

# Configure download (optional)
set(POLYPLUG_DOWNLOAD ON)
set(POLYPLUG_VERSION "0.1.0")

# Find the library
find_package(Polyplug REQUIRED)

# Use the imported target
target_link_libraries(your_app PRIVATE Polyplug::polyplug)
target_include_directories(your_app PRIVATE ${Polyplug_INCLUDE_DIR})
```

### CMake Configuration Options

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `POLYPLUG_LIB` | Env/Cache | - | Path to libpolyplug (checked first) |
| `POLYPLUG_DOWNLOAD` | Bool | OFF | Download from GitHub Releases if not found |
| `POLYPLUG_VERSION` | String | "0.1.0" | Version to download |

### Manual Build Integration

If you prefer not to use CMake, you can manually specify the paths:

```bash
# Compile
g++ -std=c++17 -I/path/to/polyplug/host-libs/cpp \
    -L/path/to/libpolyplug -lpolyplug \
    -o my_app main.cpp

# Run (ensure library is in LD_LIBRARY_PATH or rpath)
export LD_LIBRARY_PATH=/path/to/libpolyplug:$LD_LIBRARY_PATH
./my_app
```

### Compiler Flags

Minimum compiler requirements:

- **GCC**: 8.0 or later
- **Clang**: 7.0 or later
- **MSVC**: 2019 (v16.0) or later

Required flags:

```bash
# GCC/Clang
-std=c++17

# MSVC
/std:c++17
```

## ABI Stability

The polyplug ABI is **frozen at version 1**. All structures and function signatures in `abi.hpp` are stable and will not change between minor versions.

**Important:** If you see this error:

```cpp
static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");
```

It means your headers are out of sync with the compiled `libpolyplug.so`. Rebuild both the library and your application against matching header versions.

## Further Reading

- `TRUST_MODEL.md` — Bundle identity, declared dependencies, and ABI freeze details
- `host-libs/lua/README.md` — LuaJIT FFI binding documentation
- `crates/polyplug/` — Rust runtime core implementation
