# polyplug Lua Host Library

LuaJIT FFI host library for the polyplug plugin runtime.

## Prerequisites

- **LuaJIT** (version 2.0+) — standard Lua 5.x is NOT supported (requires the `ffi` module)
- A compiled `libpolyplug.so` shared library

## Quick Start

```lua
local polyplug = require("polyplug")

-- 1. Load the shared library (must use full path)
polyplug.load_lib("/usr/local/lib/libpolyplug.so")

-- 2. Create a runtime
local rt = polyplug.Runtime.new()

-- 3. Load a plugin bundle
rt:load_bundle("/path/to/my/plugin_bundle")

-- 4. Find a plugin by contract ID (use ffi.cast for u64 precision)
local ffi = require("ffi")
local contract_id = ffi.cast("uint64_t", 0xCC4232FAB0410D2BULL)
local handle = rt:find_by_contract(contract_id)

-- 5. Check if found
if ffi.cast("uint64_t", handle) == polyplug.NULL_HANDLE then
    error("plugin not found: " .. polyplug.last_error())
end

-- 6. Resolve to a guard and get vtable
local guard, err = rt:resolve_plugin(handle)
if not guard then
    error("resolve failed: " .. err)
end
local vtable = guard:vtable()
-- Cast vtable to your contract-specific struct and call functions
```

## Handle Encoding

Plugin handles are packed into `uint64_t` cdata values. The sentinel for "not found" is `NULL_HANDLE = u64::MAX`.

**Important**: Always use `ffi.cast("uint64_t", ...)` for contract IDs and bundle IDs. Lua `number` is a double-precision float and loses precision for integers above 2^53. LuaJIT cdata `uint64_t` handles the full 64-bit range correctly.

```lua
-- Correct: cdata uint64_t
local contract_id = ffi.cast("uint64_t", 0xCC4232FAB0410D2BULL)

-- Wrong: plain Lua number (loses precision for large IDs)
-- local contract_id = 0xCC4232FAB0410D2B  -- may lose bits!
```

## Vtable Dispatch Example

```lua
-- Declare your contract vtable type
ffi.cdef([[
    typedef struct {
        int32_t (*add)(int32_t a, int32_t b);
    } TestAddVTable;
]])

local guard, err = rt:resolve_plugin(handle)
if not guard then error(err) end

-- Cast the opaque vtable pointer to your contract type
local vt = ffi.cast("const TestAddVTable*", guard:vtable())
local result = vt.add(1, 2)
print(result)  -- 3
```

## Error Handling

When a function returns `nil` or a non-zero result code, call `polyplug.last_error()` to get the error message:

```lua
local guard, err = rt:resolve_plugin(handle)
if not guard then
    print("Error: " .. err)  -- err is already the error string
end

-- Or call last_error() directly:
local result = rt:load_bundle("/bad/path")
-- This will raise an error automatically (the lib calls error())
```

## GC / Memory Management

The library uses `ffi.gc()` per-instance to register cleanup finalizers:

- `Runtime` and `Guard` objects are **automatically freed** when garbage collected
- Call `:free()` for **explicit early cleanup** (this disarms the GC finalizer to prevent double-free)
- Never call `:free()` more than once on the same object

```lua
-- Automatic cleanup (recommended)
do
    local rt = polyplug.Runtime.new()
    rt:load_bundle("/path/to/bundle")
    -- rt is freed when it goes out of scope (GC)
end

-- Explicit cleanup (for deterministic resource release)
local rt = polyplug.Runtime.new()
rt:load_bundle("/path/to/bundle")
rt:free()  -- freed immediately, not waiting for GC
```

**Note**: Do NOT use `__gc` metamethods on metatypes for pointer cdata — this is a LuaJIT limitation. The `ffi.gc()` per-instance approach used here is the correct pattern.
