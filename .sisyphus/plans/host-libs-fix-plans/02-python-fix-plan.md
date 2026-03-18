# Python Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The Python host lib has significant performance overhead due to ctypes patterns that create new objects on every call. Python's ctypes is inherently slower than native code, but we can still optimize for zero-overhead on the hot path.

## Issues Found

### 1. Loaders Embedded in Main Package (CRITICAL)

**Current State:**
```
host-libs/python/polyplug/loaders/native.py
host-libs/python/polyplug/loaders/python.py
host-libs/python/polyplug/loaders/lua.py
host-libs/python/polyplug/loaders/js.py
host-libs/python/polyplug/loaders/js_deno.py
host-libs/python/polyplug/loaders/dotnet.py
```

All loaders are in the main `polyplug` package.

**Required Change:**
Each loader should be a separate pip-installable package:
```
host-libs/python/loaders/polyplug-loaders-native/
host-libs/python/loaders/polyplug-loaders-python/
host-libs/python/loaders/polyplug-loaders-lua/
host-libs/python/loaders/polyplug-loaders-js/
host-libs/python/loaders/polyplug-loaders-js-deno/
host-libs/python/loaders/polyplug-loaders-dotnet/
```

Each with:
- `pyproject.toml`
- `polyplug_loaders_<name>/__init__.py`

### 2. No VTable Caching in PluginGuard (PERFORMANCE - HIGH)

**Current State (runtime.py lines 40-49):**
```python
def get_vtable(self) -> ctypes.c_void_p:
    if self._guard is None or self._guard == 0:
        raise RuntimeError("PluginGuard is null")
    vtable_ptr: ctypes.c_void_p = self._lib.polyplug_runtime_plugin_vtable(
        self._guard
    )
    # ... P/Invoke call every time
    return vtable_ptr
```

Every call to `get_vtable()` performs a P/Invoke call.

**Required Change:**
Cache the vtable pointer at construction:
```python
class PluginGuard:
    def __init__(self, lib: ctypes.CDLL, guard_ptr: ctypes.c_void_p) -> None:
        self._lib: ctypes.CDLL = lib
        self._guard: ctypes.c_void_p = guard_ptr
        # Cache vtable at construction - one P/Invoke
        self._vtable: ctypes.c_void_p = lib.polyplug_runtime_plugin_vtable(guard_ptr)
    
    @property
    def vtable(self) -> ctypes.c_void_p:
        return self._vtable
```

### 3. call_plugin_fn Creates New ctypes Types Every Call (PERFORMANCE - CRITICAL)

**Current State (helpers.py lines 100-146):**
```python
def call_plugin_fn(lib: ctypes.CDLL, vtable_ptr: int, func_idx: int, input: str) -> str:
    # Creates new VTableStruct EVERY CALL
    class VTableStruct(ctypes.Structure):
        _fields_ = [
            ("contract_id", ctypes.c_uint64),
            ("contract_version", ctypes.c_uint32),
            ("function_count", ctypes.c_uint32),
            ("functions", ctypes.c_void_p),
        ]

    vtable = VTableStruct.from_address(vtable_ptr)
    # ...
    
    # Creates new FUNC_TYPE EVERY CALL
    FUNC_TYPE = ctypes.CFUNCTYPE(ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
    func = FUNC_TYPE(func_ptr)
```

**Required Change:**
Cache types and function pointers at module level:
```python
# Module-level cached types
_VTableStruct = None
_DispatchFnType = None

def _init_types():
    global _VTableStruct, _DispatchFnType
    
    class _VTable(ctypes.Structure):
        _fields_ = [
            ("contract_id", ctypes.c_uint64),
            ("contract_version", ctypes.c_uint32),
            ("function_count", ctypes.c_uint32),
            ("functions", ctypes.c_void_p),
        ]
    _VTableStruct = _VTable
    
    _DispatchFnType = ctypes.CFUNCTYPE(
        ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p
    )

# Call at import time
_init_types()

# Cache function pointer wrappers
_func_cache: dict[int, ctypes._CFuncPtr] = {}

def call_plugin_fn(vtable_ptr: int, func_idx: int, input: str) -> str:
    vtable = _VTableStruct.from_address(vtable_ptr)
    func_ptr = ctypes.cast(vtable.functions, ctypes.POINTER(ctypes.c_void_p))[func_idx]
    
    # Check cache for function wrapper
    if func_ptr not in _func_cache:
        _func_cache[func_ptr] = _DispatchFnType(func_ptr)
    
    func = _func_cache[func_ptr]
    # ...
```

### 4. No Pre-bound argtypes (PERFORMANCE)

**Current State (runtime.py lines 102-166):**
```python
@staticmethod
def _bind_functions(lib: ctypes.CDLL) -> None:
    lib.polyplug_runtime_create.argtypes = []
    lib.polyplug_runtime_create.restype = ctypes.c_void_p
    # ... set every time Runtime is created
```

**Required Change:**
Bind types once at module level:
```python
# Module-level function bindings
_lib_bindings_initialized = False

def _init_lib_bindings(lib: ctypes.CDLL) -> None:
    global _lib_bindings_initialized
    if _lib_bindings_initialized:
        return
    
    lib.polyplug_runtime_create.argtypes = []
    lib.polyplug_runtime_create.restype = ctypes.c_void_p
    
    lib.polyplug_runtime_find_by_contract.argtypes = [
        ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32
    ]
    lib.polyplug_runtime_find_by_contract.restype = ctypes.c_uint64
    
    # ... all other bindings
    
    _lib_bindings_initialized = True
```

### 5. Generated Code Not Optimized for Python

**Current State (codegen python.rs):**
The host callers create dispatch function wrappers without caching:
```python
out.push_str("        err: int = self._dispatch_fn(args_ptr, out_ptr)\n")
```

**Required Change:**
Generated code should use pre-cached types and minimize ctypes overhead.

## Files to Modify

1. **host-libs/python/polyplug/runtime.py**
   - Add vtable caching to `PluginGuard`
   - Move function bindings to module level
   - Cache dispatch function types

2. **host-libs/python/polyplug/helpers.py**
   - Rewrite `call_plugin_fn` with caching
   - Add module-level type definitions

3. **host-libs/python/polyplug/loaders/*.py**
   - Move to separate packages

4. **crates/polyplug_codegen/src/generators/python.rs**
   - Update to generate optimized callers with caching

## New Directory Structure

```
host-libs/python/
├── polyplug/                    # Core runtime (no loaders)
│   ├── __init__.py
│   ├── __init__.pyi
│   ├── abi.py
│   ├── abi.pyi
│   ├── runtime.py
│   ├── runtime.pyi
│   ├── helpers.py
│   └── helpers.pyi
├── loaders/
│   ├── polyplug-loaders-native/
│   │   ├── pyproject.toml
│   │   └── polyplug_loaders_native/__init__.py
│   ├── polyplug-loaders-python/
│   │   ├── pyproject.toml
│   │   └── polyplug_loaders_python/__init__.py
│   ├── polyplug-loaders-lua/
│   │   └── ...
│   ├── polyplug-loaders-js/
│   │   └── ...
│   ├── polyplug-loaders-js-deno/
│   │   └── ...
│   └── polyplug-loaders-dotnet/
│       └── ...
└── pyproject.toml               # Workspace build
```

## Performance Expectations

Python ctypes has inherent overhead (typically 100-500ns per call), but with caching we can minimize it:

| Operation | Current | Optimized |
|-----------|---------|-----------|
| VTable access | ~200ns (P/Invoke) | ~0ns (cached) |
| Function pointer cast | ~100ns (new type) | ~0ns (cached) |
| Type creation | ~500ns (class def) | ~0ns (module-level) |
| **Hot path** | ~800ns | ~100-200ns |

## Implementation Order

1. Add vtable caching to `PluginGuard`
2. Move type definitions to module level
3. Add function pointer caching
4. Move loaders to separate packages
5. Update codegen

## Estimated Effort

- VTable caching: 30 minutes
- Type caching: 1 hour
- Loader restructuring: 2 hours
- Codegen update: 1 hour
- Testing: 1 hour

**Total: ~5.5 hours**

## PRD References

- PRD §8: "loaders/python.py, loaders/lua.py, etc. (one per loader)" - should be separate packages
- PRD §10 (Python): "All ctypes function objects cached at module level — no per-call lookup"
- PRD §10 (Python): "All argtypes/restype set once at import time"