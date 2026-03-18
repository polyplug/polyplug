# Lua Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The Lua host lib uses LuaJIT FFI which is extremely fast (~2x native), but the current implementation has overhead due to repeated casts and lookups. LuaJIT's FFI can achieve near-native performance with proper optimization.

## Issues Found

### 1. Loaders Embedded in Main Package (CRITICAL)

**Current State:**
```
host-libs/lua/loaders/native.lua
host-libs/lua/loaders/python.lua
host-libs/lua/loaders/lua.lua
host-libs/lua/loaders/js.lua
host-libs/lua/loaders/js_deno.lua
host-libs/lua/loaders/dotnet.lua
```

All loaders in the main `polyplug` module.

**Required Change:**
Each loader should be a separate LuaRocks package:
```
host-libs/lua/loaders/polyplug-loaders-native/
host-libs/lua/loaders/polyplug-loaders-python/
host-libs/lua/loaders/polyplug-loaders-lua/
host-libs/lua/loaders/polyplug-loaders-js/
host-libs/lua/loaders/polyplug-loaders-js-deno/
host-libs/lua/loaders/polyplug-loaders-dotnet/
```

Each with:
- `polyplug-loaders-<name>-1.0-1.rockspec`
- `polyplug/loaders/<name>.lua`

### 2. find_by_bundle is Stubbed (CRITICAL - FUNCTIONALITY)

**Current State (polyplug.lua lines 87-91):**
```lua
function M.Runtime:find_by_bundle(bundle_name, contract, min_version)
    -- Simplified: just return a handle for testing
    local lib = self._lib
    return ffi.cast("uint64_t", 1)
end
```

This is completely broken - returns a dummy handle instead of calling the actual function.

**Required Change:**
```lua
function M.Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    local lib = self._lib
    return lib.polyplug_runtime_find_by_bundle(self._ptr, bundle_id, contract_id, min_version)
end
```

### 3. call_plugin_fn Does ffi.cast Every Call (PERFORMANCE - CRITICAL)

**Current State (polyplug.lua lines 201-251):**
```lua
function M.call_plugin_fn(rt_ptr, packed_handle, func_idx, input)
    -- Resolves plugin EVERY CALL
    local guard_ptr = lib.polyplug_runtime_resolve_plugin(...)
    
    -- Gets vtable EVERY CALL
    local vtable_ptr = lib.polyplug_runtime_guard_vtable(guard_ptr)
    
    -- Casts EVERY CALL
    local vtable = ffi.cast("const void**", vtable_ptr)
    local func_count = ffi.cast("size_t*", vtable)[0]
    local funcs = ffi.cast("void***", vtable + 1)[0]
    
    -- Casts function pointer EVERY CALL
    local func = ffi.cast("uint32_t (*)(const void*, void*)", func_ptr)
    
    -- Destroys guard EVERY CALL
    lib.polyplug_runtime_guard_destroy(guard_ptr)
end
```

**Required Change:**
Use cached vtable and pre-cast function pointers:
```lua
-- Module-level cached types
local VTableType = ffi.typeof("const PluginVTable*")
local DispatchFnType = ffi.typeof("uint32_t (*)(const void*, void*)")

-- Function pointer cache
local func_cache = {}

function M.call_plugin_fn(vtable_ptr, func_idx, input)
    local vtable = ffi.cast(VTableType, vtable_ptr)
    
    if func_idx >= vtable.function_count then
        error("function index " .. func_idx .. " out of bounds")
    end
    
    -- Get function pointer (no cast needed if vtable is properly typed)
    local func_ptr = vtable.functions[func_idx]
    
    -- Check cache
    local func = func_cache[func_ptr]
    if not func then
        func = ffi.cast(DispatchFnType, func_ptr)
        func_cache[func_ptr] = func
    end
    
    -- Prepare input
    local input_sv = ffi.new("StringView", { ptr = input_data, len = #input })
    local output_sv = ffi.new("StringView", { ptr = nil, len = 0 })
    
    -- Call (one indirect call)
    local err_code = func(input_sv, output_sv)
    
    -- ...
end
```

### 4. Guard Doesn't Cache VTable (PERFORMANCE)

**Current State:**
The Lua implementation doesn't have a proper `Guard` class that caches the vtable.

**Required Change:**
Add a `Guard` class similar to C#:
```lua
local Guard = {}
Guard.__index = Guard

function Guard.new(lib, guard_ptr)
    local self = {
        _lib = lib,
        _guard = guard_ptr,
        -- Cache vtable at construction
        _vtable = lib.polyplug_runtime_guard_vtable(guard_ptr)
    }
    return setmetatable(self, Guard)
end

function Guard:vtable()
    return self._vtable  -- No FFI call
end

function Guard:destroy()
    if self._guard then
        self._lib.polyplug_runtime_guard_destroy(self._guard)
        self._guard = nil
    end
end
```

### 5. Generated Code Creates Structs Every Call (CODEGEN)

**Current State (codegen lua.rs lines 327-349):**
```lua
-- Generated code creates new FFI casts every call
out.push_str("    local fn = ffi.cast(\"uint32_t (*)(const void*, void*)\", fn_ptr)\n")
```

**Required Change:**
Generated code should use cached types and minimize casts.

### 6. Missing Proper Error Handling

**Current State:**
Error messages are thrown as strings without proper error codes.

**Required Change:**
Use structured error handling:
```lua
local PolyplugError = {
    NOT_FOUND = 4,
    STALE_HANDLE = 5,
    FUNCTION_NOT_AVAIL = 6,
}

function M.last_error(lib)
    local len = lib.polyplug_runtime_error_message_len()
    if len == 0 then return "" end
    local buf = ffi.new("uint8_t[?]", len)
    lib.polyplug_runtime_last_error(buf, len)
    return ffi.string(buf, len)
end
```

## Files to Modify

1. **host-libs/lua/polyplug.lua**
   - Fix `find_by_bundle` (currently stubbed!)
   - Add `Guard` class with vtable caching
   - Add function pointer caching
   - Rewrite `call_plugin_fn` with caching

2. **host-libs/lua/loaders/*.lua**
   - Move to separate packages

3. **crates/polyplug_codegen/src/generators/lua.rs**
   - Update to generate optimized callers

## New Directory Structure

```
host-libs/lua/
├── polyplug.lua                 # Core runtime (no loaders)
├── polyplug.d.lua               # Type definitions
├── scanner.lua
├── loaders/
│   ├── polyplug-loaders-native/
│   │   ├── polyplug-loaders-native-1.0-1.rockspec
│   │   └── polyplug/loaders/native.lua
│   ├── polyplug-loaders-python/
│   │   └── ...
│   ├── polyplug-loaders-lua/
│   │   └── ...
│   ├── polyplug-loaders-js/
│   │   └── ...
│   ├── polyplug-loaders-js-deno/
│   │   └── ...
│   └── polyplug-loaders-dotnet/
│       └── ...
└── README.md
```

## Performance Expectations

LuaJIT FFI is extremely fast. With proper optimization:

| Operation | Current | Optimized |
|-----------|---------|-----------|
| VTable access | ~100ns (FFI calls) | ~2ns (cached) |
| Function cast | ~50ns (ffi.cast) | ~0ns (cached) |
| Guard operations | ~200ns (create/destroy) | ~10ns (reused) |
| **Hot path** | ~350ns | ~50-100ns |

LuaJIT can achieve ~2x native speed for FFI calls when properly optimized.

## Critical Bug

The `find_by_bundle` function is **completely broken** - it returns a dummy handle `1` instead of calling the actual runtime function. This will cause any code using bundle-specific plugin lookups to fail silently.

## Implementation Order

1. **FIX find_by_bundle** (Critical bug!)
2. Add `Guard` class with vtable caching
3. Add function pointer caching
4. Rewrite `call_plugin_fn`
5. Move loaders to separate packages
6. Update codegen

## Estimated Effort

- Fix `find_by_bundle`: 15 minutes (critical bug!)
- Guard class: 1 hour
- Function caching: 1 hour
- Loader restructuring: 2 hours
- Testing: 1 hour

**Total: ~5 hours**

## PRD References

- PRD §8: "LuaJIT FFI host lib, Runtime metatable, Guard metatable"
- PRD §8: "register_*_loader() functions (one per loader)" - should be separate packages
- PRD §10 (Lua): "Performance: LuaJIT FFI call overhead is within 2x of native vtable dispatch"
- PRD §10 (Lua): "JIT-compiled to near-native speed (~800M ops/sec vs ~45M for lightuserdata)"