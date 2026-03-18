# polyplug Performance Guide

This document covers performance characteristics and optimization strategies for all polyplug host libraries.

## Overview

polyplug is designed for **zero-overhead hot path calls**. The architecture ensures:

1. **One guard load** - Resolve handle to vtable once
2. **One pointer dereference** - Access cached vtable
3. **One indirect call** - Dispatch to plugin function

```
┌─────────────────────────────────────────────────────────────────┐
│                    HOT PATH CALL FLOW                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   1. Runtime.resolve_plugin(handle)                              │
│      └─> Validates generation counter                           │
│      └─> Returns Guard with vtable pointer                      │
│                                                                  │
│   2. Guard.vtable()                                              │
│      └─> Returns cached pointer (no FFI)                        │
│                                                                  │
│   3. vtable.functions[fn_id](args, out)                         │
│      └─> Direct indirect call                                   │
│                                                                  │
│   Total overhead: ~10-50ns (native) to ~400ns (Python ctypes)   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Host Library Performance Comparison

### Call Overhead by Language

| Language | Backend | Call Overhead | Speedup vs Python ctypes |
|----------|---------|---------------|--------------------------|
| **C++** | Native | ~10-20 ns | 30-60x |
| **Lua** | LuaJIT FFI | ~20-50 ns | 10-30x |
| **JavaScript** | Deno FFI | ~50-100 ns | 5-10x |
| **Python** | cffi ABI | ~380 ns | 1.7x |
| **Python** | ctypes | ~670 ns | 1.0x (baseline) |

### Why the Differences?

| Language | FFI Mechanism | Overhead Source |
|----------|---------------|-----------------|
| C++ | Direct function call | None - same language |
| Lua | LuaJIT FFI | JIT-compiled, near-native |
| JavaScript | V8 FFI | V8 fast calls, some GC |
| Python cffi | libffi | Pre-parsed bindings |
| Python ctypes | libffi + Python wrappers | Dynamic type checking |

---

## Language-Specific Optimization

### C++ (Optimal)

**Already zero-overhead:**
- `PluginGuard` caches vtable at construction
- Direct pointer access, no FFI on hot path
- Move semantics for efficient transfer

```cpp
// Hot path - zero FFI overhead
auto guard = rt.resolve_plugin(handle);
const auto* vtable = guard.vtable();  // Cached pointer
vtable->process(data);  // Direct indirect call
```

**Hot-reload safety:** Guard stores handle, re-resolves vtable on each call.

### Lua (Near-Optimal)

**LuaJIT FFI is extremely fast (~2x native C):**
- Module-level type caching (`VTableType`, `DispatchFnType`)
- Function pointer cache (`func_cache`)
- JIT-compiled calls

```lua
-- Hot path
local guard = rt:resolve_plugin(handle)
local result = guard:call(0, input)  -- Re-resolves vtable for hot-reload safety
```

**Hot-reload safety:** Guard stores `runtime + handle`, re-resolves vtable each call.

### JavaScript / Deno (Good)

**V8 FFI is fast:**
- Module-level caches (`_funcCache`, `_DISPATCH_FN_TYPE`)
- `BigUint64Array` for fast vtable reads
- `UnsafeFnPointer` for direct calls

```javascript
// Hot path
const guard = rt.resolvePlugin(handle);
const result = guard.call(0, input);  // Re-resolves vtable for hot-reload safety
```

**Hot-reload safety:** Guard stores `runtime + handle`, re-resolves vtable each call.

### Python (Acceptable)

**Two backend options:**

#### ctypes (default)
- **Overhead**: ~670 ns per call
- **Requirements**: None (built-in)
- **Best for**: Plugin functions >10μs

#### cffi ABI (optional)
- **Overhead**: ~380 ns per call (1.7x faster)
- **Requirements**: `pip install cffi`
- **Best for**: Performance-sensitive applications

```python
# Automatic backend selection
from polyplug import Runtime

rt = Runtime()
guard = rt.resolve_plugin(handle)
vtable = guard.vtable  # Cached pointer
```

**Hot-reload safety:** Guard stores handle, re-resolves vtable each call.

---

## Decision Matrix

### When to Use Each Backend

| Plugin Function Duration | Python ctypes | Python cffi | Other Languages |
|-------------------------|---------------|-------------|-----------------|
| < 1 μs (trivial) | 50-70% overhead | 30-40% overhead | Use C++/Lua |
| 1-10 μs (light) | 5-50% overhead | 3-30% overhead | Any language OK |
| 10-100 μs (moderate) | 0.5-5% overhead | 0.3-3% overhead | Negligible |
| > 100 μs (heavy) | < 0.5% overhead | < 0.3% overhead | Negligible |

### Language Selection Guide

| Use Case | Recommended Language | Reason |
|----------|---------------------|--------|
| Maximum performance | C++ | Zero FFI overhead |
| Game engines | C++ or Lua | LuaJIT is extremely fast |
| Web backends | JavaScript (Deno) | Good FFI, async support |
| Data science | Python | Ecosystem, acceptable overhead |
| Scripting/embedded | Lua | Small footprint, fast FFI |

---

## Hot-Reload Safety Architecture

All host libraries implement the same hot-reload safety pattern:

```
┌─────────────────────────────────────────────────────────────────┐
│                    HOT-RELOAD SAFE GUARD                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Guard stores: runtime + handle (NOT cached vtable)            │
│                                                                  │
│   On each call:                                                  │
│   1. Guard.call() → resolve_plugin(runtime, handle)             │
│   2. Runtime validates generation counter                        │
│   3. If stale (hot-reload happened) → returns error             │
│   4. If valid → returns current vtable pointer                   │
│   5. Call through vtable                                         │
│                                                                  │
│   This ensures:                                                  │
│   - Hot-reload invalidates old handles                           │
│   - No dangling vtable pointers                                  │
│   - Safe concurrent access                                       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Why not cache vtable?**

If we cached the vtable pointer in Guard:
1. Hot-reload swaps the vtable
2. Old Guard still has old vtable pointer
3. Call through old vtable = use-after-free

By storing handle and re-resolving:
1. Hot-reload increments generation counter
2. Old handle's generation doesn't match
3. `resolve_plugin` returns error
4. Caller gets clear error instead of crash

---

## Benchmarking

### Run Benchmarks

```bash
# Python
cd host-libs/python
python -m venv .venv && source .venv/bin/activate
pip install cffi
POLYPLUG_LIB=/path/to/libpolyplug.so python benchmarks/benchmark_ffi_final.py

# Rust core
cargo bench -p polyplug
```

### Expected Results

**Python FFI (1M iterations):**
```
ctypes:   ~670 ns/call
cffi ABI: ~380 ns/call (1.7x faster)
```

**Rust Core (from previous benchmarks):**
```
ffi/resolve_plugin:          ~10 ns
ffi/find_all_by_contract:    ~25 ns
registry/find_by_contract:   ~21 ns
```

---

## Optimization Tips

### 1. Batch Operations

Instead of multiple FFI calls:
```python
# Bad: Multiple FFI calls
for contract_id in contract_ids:
    handle = rt.find_by_contract(contract_id, 1)

# Good: Single FFI call
handles = rt.find_all_by_contract(contract_id, 1)
```

### 2. Reuse Guards

```python
# Bad: Resolve on every call
for data in dataset:
    guard = rt.resolve_plugin(handle)
    result = call_plugin(guard.vtable, data)

# Good: Resolve once, call many times
guard = rt.resolve_plugin(handle)
for data in dataset:
    result = call_plugin(guard.vtable, data)
```

### 3. Choose the Right Language

For hot paths called millions of times:
- C++: ~10-20 ns overhead
- Python ctypes: ~670 ns overhead
- Difference: 30-60x

If your hot path is truly performance-critical, consider C++ or Lua.

---

## See Also

- [ABI Architecture](./ABI_ARCHITECTURE.md)
- [ABI Types](./abi_types.md)
- [Python README](../host-libs/python/README.md)
- [C++ README](../host-libs/cpp/README.md)
- [Lua README](../host-libs/lua/README.md)
- [JavaScript README](../host-libs/js/README.md)