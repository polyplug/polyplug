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
│   On each vtable() call:                                         │
│   1. Guard.vtable() → resolve_plugin(runtime, handle)           │
│   2. Runtime validates generation counter                        │
│   3. If stale (hot-reload happened) → returns null/error        │
│   4. If valid → returns current vtable pointer                   │
│                                                                  │
│   This ensures:                                                  │
│   - Hot-reload invalidates old handles                           │
│   - No dangling vtable pointers                                  │
│   - Safe concurrent access                                       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Why not cache vtable?**

When hot-reload happens, the Rust runtime:
1. Swaps the vtable Arc in the slot
2. Returns the old Arc
3. If no Rust guard holds it, the old vtable is **freed**

Any cached raw pointer becomes a **dangling pointer** → use-after-free crash.

**Overhead:**

| Operation | Cost | Impact |
|-----------|------|--------|
| Cached vtable | ~0-5 ns | ❌ Crash on hot-reload |
| Re-resolve vtable | ~10-50 ns | ✅ Safe |

For typical plugin calls (>1μs), the 10-50ns overhead is <5%.

**Future consideration:** A "red-green" state mechanism where the runtime pauses all plugin calls during hot-reload, allowing cached vtables to be safely invalidated. This would eliminate the per-call overhead while maintaining safety.

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

## VM Loader Performance

### JavaScript/QuickJS Guest Plugins

QuickJS guest plugins use a cached Context architecture for minimal dispatch overhead:

```
┌─────────────────────────────────────────────────────────────────┐
│                 QUICKJS DISPATCH ARCHITECTURE                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Bundle Load (one-time):                                        │
│   1. Create QuickJS Runtime (per-bundle, owned by JsLoaderData) │
│   2. Create Context for this bundle                             │
│   3. Evaluate bundle JS, extract vtable                         │
│   4. Store Runtime + Context + Persistent<Function> in LoaderData│
│                                                                  │
│   Dispatch Call (hot path):                                      │
│   1. data.ctx.with(|ctx| { ... })     // Reuse cached Context   │
│   2. func.clone().restore(&ctx)       // ~10-50 ns              │
│   3. func.call(args)                  // JS execution           │
│                                                                  │
│   Total overhead: ~75 ns (excluding JS execution time)          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### Benchmark Results

| Benchmark | Time | Description |
|-----------|------|-------------|
| `cached_context_single_call` | **75 ns** | Single dispatch with cached Context |
| `cached_context_10_calls` | 656 ns | 10 calls (~66 ns/call) |
| `old_fresh_context_per_call` | ~~124 µs~~ | OLD: Context created each call |
| `new_cached_context_reuse` | **75 ns** | NEW: Context cached and reused |

**Speedup: 1,659x faster** than creating Context per call.

#### Why This Is Minimal Overhead

The ~75 ns overhead is the theoretical minimum for QuickJS dispatch:

| Component | Time | Description |
|-----------|------|-------------|
| `ctx.with()` scope entry | ~10 ns | Enter QuickJS context |
| `Persistent::clone()` | ~10-20 ns | Clone the persistent reference |
| `restore(&ctx)` | ~30-50 ns | Restore JS function in context |
| JS function call overhead | ~10-20 ns | QuickJS internal dispatch |
| **Total** | **~60-100 ns** | **Cannot be reduced further** |

This overhead cannot be eliminated because:
1. QuickJS requires a context scope for any JS operation
2. `Persistent<Function>` must be restored to the current context
3. The JS function call itself has minimal QuickJS overhead

#### Per-Bundle Runtime Isolation

Each bundle gets its own QuickJS Runtime stored in `JsLoaderData`. This ensures:
- Complete isolation between bundles
- Complete isolation between polyplug Runtime instances
- Tests can run in parallel without state pollution
- No shared global state between different plugin bundles

#### Comparison with Other VM Loaders

| Loader | Dispatch Overhead | Architecture |
|--------|-------------------|--------------|
| **Native** | ~1 ns | Direct function pointer |
| **QuickJS** | **~75-90 ns** | Per-bundle Runtime + Cached Context |
| **Lua** | ~1-5 µs | mlua Function call |
| **Python** | ~1-5 µs | PyO3 GIL + callable |
| **.NET** | ~1-5 µs | CLR function pointer |

QuickJS is the fastest VM loader due to:
1. Per-bundle Runtime (complete isolation)
2. Cached Context (no per-call creation)
3. `Persistent<Function>` for direct function reference
4. Minimal QuickJS runtime overhead

---

## Loader Dispatch Benchmarks

### Native Baseline

All loaders are compared against native function calls:

| Benchmark | Time |
|-----------|------|
| Native function call | ~900 ps (0.9 ns) |
| Native function pointer call | ~1.3 ns |

### QuickJS (JS Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `cached_context_single_call` | **~87 ns** | Single dispatch with cached Context |
| `cached_context_10_calls` | ~667 ns | 10 calls (~67 ns/call) |
| `old_fresh_context_per_call` | ~~124 µs~~ | OLD: Context created each call |
| `new_cached_context_reuse` | **~87 ns** | NEW: Context cached and reused |

**Speedup: 1,400x faster** than creating Context per call.

### Lua (LuaJIT Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `lua_dispatch_single_call` | ~1-5 µs | VM dispatch via mlua |
| `lua_vm_creation` | ~100+ µs | One-time VM creation cost |
| `native_baseline` | ~0.9 ns | Native function call reference |

Lua uses LuaJIT with FFI for near-native performance. The dispatch overhead is higher than QuickJS due to the mlua binding layer.

### Python (CPython Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `python_dispatch_single_call` | ~1-5 µs | GIL acquisition + function call |
| `gil_acquisition` | ~100-500 ns | GIL acquisition cost |
| `native_baseline` | ~0.9 ns | Native function call reference |

Python dispatch requires GIL acquisition, which adds overhead. For performance-critical paths, consider using native or QuickJS plugins.

### .NET (CLR Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `native_dispatch` | ~1-5 ns | Native function pointer call |
| `dispatch_signature` | ~1-5 ns | Exact .NET dispatch signature |
| `native_baseline` | ~0.9 ns | Native function call reference |

.NET uses native function pointers for dispatch, providing near-native performance. The CLR initialization cost is amortized across all calls.

### Summary

| Loader Type | Dispatch Overhead | Best For |
|-------------|-------------------|----------|
| **Native** | ~1 ns | Maximum performance |
| **.NET** | ~1-5 ns | Near-native with CLR ecosystem |
| **QuickJS** | ~75-90 ns | Fastest VM dispatch |
| **Lua** | ~1-5 µs | Embedded scripting |
| **Python** | ~1-5 µs | Data science, ML ecosystem |

---

## See Also

- [ABI Architecture](./ABI_ARCHITECTURE.md)
- [ABI Types](./abi_types.md)
- [Python README](../host-libs/python/README.md)
- [C++ README](../host-libs/cpp/README.md)
- [Lua README](../host-libs/lua/README.md)
- [JavaScript README](../host-libs/js-deno/README.md)