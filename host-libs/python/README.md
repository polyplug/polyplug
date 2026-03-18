# polyplug Python Host Library

ctypes-based host library for the polyplug plugin runtime.

## Prerequisites

- **Python 3.10+**
- A compiled `libpolyplug.so` shared library

## Installation

```bash
# Core library (ctypes-based, no dependencies)
pip install polyplug

# Optional: Faster cffi backend (1.7x faster FFI calls)
pip install polyplug[cffi]
```

## Quick Start

```python
from polyplug import Runtime

# Create a runtime
rt = Runtime()

# Load a plugin bundle
rt.load_bundle("/path/to/my_plugin_bundle")

# Find a plugin by contract ID
from polyplug.abi import contract_id
handle = rt.find_by_contract(contract_id("my.contract", 1), 1)

# Resolve to a guard and get vtable
guard = rt.resolve_plugin(handle)
vtable = guard.vtable
```

## Performance Backends

polyplug supports two FFI backends:

### ctypes (default)

- **Overhead**: ~670 ns per FFI call
- **Requirements**: None (built into Python)
- **Best for**: Plugin functions >10μs

### cffi ABI (optional, faster)

- **Overhead**: ~380 ns per FFI call (1.7x faster)
- **Requirements**: `pip install cffi`
- **Best for**: Performance-sensitive applications

To use cffi:

```bash
pip install cffi
```

The library automatically uses cffi if available, falling back to ctypes otherwise.

## API Reference

### Runtime

```python
class Runtime:
    def load_bundle(self, path: str | Path) -> None:
        """Load a plugin bundle from disk."""
    
    def reload_bundle(self, path: str | Path) -> None:
        """Reload a plugin bundle (hot-reload)."""
    
    def find_by_contract(self, contract_id: int, min_version: int) -> int:
        """Find a plugin by contract ID. Returns packed handle or NULL_HANDLE."""
    
    def find_by_bundle(self, bundle_id: int, contract_id: int, min_version: int) -> int:
        """Find a specific bundle's implementation of a contract."""
    
    def find_all_by_contract(self, contract_id: int, min_version: int) -> list[int]:
        """Find all plugins implementing a contract."""
    
    def resolve_plugin(self, packed_handle: int) -> PluginGuard:
        """Resolve a handle to a guard with cached vtable."""
```

### PluginGuard

```python
class PluginGuard:
    @property
    def vtable(self) -> ctypes.c_void_p:
        """Return cached vtable pointer (no FFI call)."""
```

### Utility Functions

```python
def contract_id(name: str, major_version: int) -> int:
    """Compute FNV-1a 64-bit contract ID."""

def bundle_id(name: str) -> int:
    """Compute FNV-1a 64-bit bundle ID."""
```

## Error Handling

```python
from polyplug import Runtime
from polyplug.abi import NULL_HANDLE

rt = Runtime()
handle = rt.find_by_contract(contract_id, 1)

if handle == NULL_HANDLE:
    # Plugin not found
    pass
else:
    guard = rt.resolve_plugin(handle)
```

## Hot-Reload Safety

The `PluginGuard` stores the handle, not a cached vtable pointer. On each call, it re-resolves the vtable to detect stale handles after hot-reload:

```python
# Hot-reload scenario
rt.load_bundle("/path/to/plugin")
handle = rt.find_by_contract(contract_id, 1)
guard = rt.resolve_plugin(handle)

# Hot-reload the bundle
rt.reload_bundle("/path/to/plugin")

# Old handle is now stale - resolve_plugin will fail
# This prevents use-after-free bugs
```

## Performance Considerations

| Plugin Function Duration | ctypes Overhead | Impact |
|-------------------------|-----------------|--------|
| < 1 μs (trivial) | 50-70% | Consider cffi backend |
| 1-10 μs (light) | 5-50% | cffi recommended |
| 10-100 μs (moderate) | 0.5-5% | ctypes is fine |
| > 100 μs (heavy) | < 0.5% | Negligible |

Run benchmarks:

```bash
cd host-libs/python
python -m venv .venv && source .venv/bin/activate
pip install cffi
POLYPLUG_LIB=/path/to/libpolyplug.so python benchmarks/benchmark_ffi_final.py
```

## See Also

- [Performance Documentation](../../docs/PERFORMANCE.md) - Cross-language performance comparison
- [ABI Types](../../docs/abi_types.md) - ABI type definitions
- [C++ Host Library](../cpp/README.md) - C++ bindings
- [Lua Host Library](../lua/README.md) - LuaJIT bindings