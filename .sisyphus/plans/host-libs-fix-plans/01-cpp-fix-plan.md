# C++ Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The C++ host lib has several performance issues and architectural problems similar to what the C# host lib had before being fixed.

## Issues Found

### 1. Loaders Embedded in Main Package (CRITICAL)

**Current State:**
```
host-libs/cpp/polyplug/loaders/native.hpp
host-libs/cpp/polyplug/loaders/python.hpp
host-libs/cpp/polyplug/loaders/lua.hpp
host-libs/cpp/polyplug/loaders/js.hpp
host-libs/cpp/polyplug/loaders/js_deno.hpp
host-libs/cpp/polyplug/loaders/dotnet.hpp
```

All loaders are in the main `polyplug` package under `loaders/` subdirectory.

**Required Change:**
Each loader should be a separate C++ package/library, similar to C#:
```
host-libs/cpp/loaders/native/
host-libs/cpp/loaders/python/
host-libs/cpp/loaders/lua/
host-libs/cpp/loaders/js/
host-libs/cpp/loaders/js_deno/
host-libs/cpp/loaders/dotnet/
```

Each with its own:
- `CMakeLists.txt` or `meson.build`
- `polyplug_loaders_<name>.hpp`
- `polyplug_loaders_<name>.pc` (pkg-config)

### 2. No VTable Caching (PERFORMANCE)

**Current State:**
```cpp
// runtime.hpp - line 77-79
uint64_t find(uint64_t contract_id, uint32_t min_version) const noexcept {
    return polyplug_runtime_find_by_contract(handle_, contract_id, min_version);
}
```

The `Runtime` class has no mechanism to resolve plugins and cache vtable pointers.

**Required Change:**
Add a `PluginGuard` class that caches the vtable pointer at construction:
```cpp
class PluginGuard {
public:
    PluginGuard(RuntimeHandle rt, uint64_t packed_handle);
    ~PluginGuard();
    
    // Delete copy, allow move
    PluginGuard(const PluginGuard&) = delete;
    PluginGuard(PluginGuard&& other) noexcept;
    
    const PluginVTable* vtable() const noexcept { return vtable_; }
    
private:
    OpaqueGuard* guard_;
    const PluginVTable* vtable_;  // Cached at construction
};
```

### 3. Generated Code Calls resolve_plugin Every Time (PERFORMANCE)

**Current State (codegen cpp.rs lines 957-963):**
```cpp
out.push_str("        const PolyplugVTable* vtable_ = (host_->resolve_plugin)(handle_);\n");
out.push_str(&format!(
    "        auto fn_ = reinterpret_cast<AbiError(*)(const void*, void*)>(vtable_->functions[{}U]);\n",
    fn_id
));
```

Every function call resolves the plugin to get the vtable.

**Required Change:**
Generated code should accept a cached vtable pointer:
```cpp
class ImageDecodeContract {
public:
    ImageDecodeContract(const PluginVTable* vtable) : vtable_(vtable) {}
    
    Stats compute(const Image& image) {
        // Direct vtable access - one indirection
        auto fn = reinterpret_cast<AbiError(*)(const void*, void*)>(vtable_->functions[0U]);
        // ...
    }
    
private:
    const PluginVTable* vtable_;
};
```

### 4. Missing caller-provides-buffer Pattern (ARCHITECTURE)

**Current State:**
Generated code allocates output on stack but doesn't follow the PRD pattern for non-primitive returns.

**Required Change:**
For non-primitive returns, the caller should provide a buffer:
```cpp
// For primitive returns - return by value
int32_t add(int32_t a, int32_t b);

// For non-primitive returns - caller provides buffer
void get_stats(Stats* out);
```

### 5. Missing SuppressGCTransition Equivalent (N/A for C++)

C++ doesn't have GC, so this is not applicable. However, we should ensure no unnecessary synchronization.

## Files to Modify

1. **host-libs/cpp/polyplug/runtime.hpp**
   - Add `PluginGuard` class
   - Add `resolve_plugin()` method that returns `PluginGuard`

2. **host-libs/cpp/polyplug/loaders/*.hpp**
   - Move to separate packages

3. **crates/polyplug_codegen/src/generators/cpp.rs**
   - Update `generate_cpp_host_function` to use cached vtable
   - Accept vtable pointer in constructor instead of `HostVTable*`

## New Directory Structure

```
host-libs/cpp/
├── polyplug/                    # Core runtime (no loaders)
│   ├── abi.hpp
│   ├── error.hpp
│   ├── handle.hpp
│   ├── runtime.hpp              # Updated with PluginGuard
│   └── polyplug.hpp             # Single-include header
├── loaders/
│   ├── native/
│   │   ├── CMakeLists.txt
│   │   └── polyplug_loaders_native.hpp
│   ├── python/
│   │   ├── CMakeLists.txt
│   │   └── polyplug_loaders_python.hpp
│   ├── lua/
│   │   └── ...
│   ├── js/
│   │   └── ...
│   ├── js_deno/
│   │   └── ...
│   └── dotnet/
│       └── ...
└── CMakeLists.txt               # Workspace build
```

## Implementation Order

1. Add `PluginGuard` class to runtime.hpp
2. Update codegen to generate constructors accepting vtable pointer
3. Move loaders to separate packages
4. Update examples and tests

## Estimated Effort

- PluginGuard addition: 1 hour
- Codegen update: 2 hours
- Loader restructuring: 3 hours
- Testing: 2 hours

**Total: ~8 hours**

## PRD References

- PRD §8: "loaders/python.hpp, loaders/lua.hpp, etc. (one per loader)" - suggests separate headers but should be separate packages
- PRD §7: "Hot path call: One guard load. One pointer dereference. One indirect call."
- PRD §15: "Caller-owns memory: The caller allocates memory for return values."