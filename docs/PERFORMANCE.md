# polyplug Performance Guide

This document covers performance characteristics and optimization strategies for all polyplug host libraries.

## Terminology Note

This document uses the following terminology (current as of v1.1):
- **GuestContractInterface**: The interface struct a plugin provides for the host to call
- **HostApi**: The runtime's ABI table provided to guests

## Overview

polyplug is designed for **zero-overhead hot path calls**. The architecture ensures:

1. **One guard load** - Resolve handle to interface once
2. **One pointer dereference** - Access cached interface
3. **One indirect call** - Dispatch to plugin function

```
┌─────────────────────────────────────────────────────────────────┐
│                    HOT PATH CALL FLOW                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   1. Runtime.resolve_plugin(handle)                              │
│      └─> Reads the interface slot under the RuntimeStore RwLock │
│      └─> Returns Guard with interface pointer                   │
│                                                                  │
│   2. Guard.interface()                                           │
│      └─> Returns cached pointer (no FFI)                        │
│                                                                  │
│   3. interface.functions[fn_id](args, out)                      │
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
- `PluginGuard` caches interface at construction
- Direct pointer access, no FFI on hot path
- Move semantics for efficient transfer

```cpp
// Hot path - zero FFI overhead
auto guard = rt.resolve_plugin(handle);
const auto* interface = guard.interface();  // Cached pointer
interface->process(data);  // Direct indirect call
```

**Hot-reload safety:** Guard stores handle, re-resolves interface on each call.

### Lua (Near-Optimal)

**LuaJIT FFI is extremely fast (~2x native C):**
- Module-level type caching (`InterfaceType`, `DispatchFnType`)
- Function pointer cache (`func_cache`)
- JIT-compiled calls

```lua
-- Hot path
local guard = rt:resolve_plugin(handle)
local result = guard:call(0, input)  -- Re-resolves interface for hot-reload safety
```

**Hot-reload safety:** Guard stores `runtime + handle`, re-resolves interface each call.

### JavaScript / Deno (Good)

**V8 FFI is fast:**
- Module-level caches (`_funcCache`, `_DISPATCH_FN_TYPE`)
- `BigUint64Array` for fast interface reads
- `UnsafeFnPointer` for direct calls

```javascript
// Hot path
const guard = rt.resolvePlugin(handle);
const result = guard.call(0, input);  // Re-resolves interface for hot-reload safety
```

**Hot-reload safety:** Guard stores `runtime + handle`, re-resolves interface each call.

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
interface = guard.interface  # Cached pointer
```

**Hot-reload safety:** Guard stores handle, re-resolves interface each call.

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
│   Guard stores: runtime + handle (NOT cached interface)         │
│                                                                  │
│   On each interface() call:                                      │
│   1. Guard.interface() -> resolve_plugin(runtime, handle)       │
│   2. Runtime reads the interface slot under the RwLock          │
│   3. Returns the current interface pointer for the slot         │
│                                                                  │
│   This ensures:                                                  │
│   - Hot-reload swaps the slot to the new interface              │
│   - The superseded interface is retired, not freed, so          │
│     in-flight pointers stay valid                                │
│   - Safe concurrent access                                       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Why re-resolve instead of caching a raw interface pointer in the SDK?**

When hot-reload happens, the Rust runtime, under the `RuntimeStore` RwLock write guard:
1. Swaps the interface in the slot to the newly registered one (`apply_reload_swap`)
2. Pushes the superseded interface onto `retired_interfaces`

The retire-not-drop model keeps superseded interfaces alive for the runtime lifetime,
so a previously resolved pointer never dangles. Re-resolving on each call ensures the
SDK observes the **new** interface after a swap rather than continuing to dispatch into
the retired one.

**Overhead:**

| Operation | Cost | Impact |
|-----------|------|--------|
| Cached interface | ~0-5 ns | Keeps dispatching into the retired interface after reload |
| Re-resolve interface | ~10-50 ns | Observes the swapped-in interface |

For typical plugin calls (>1us), the 10-50ns overhead is <5%.

---

## Benchmarking

### Run Benchmarks

```bash
# Python
cd sdks/python/host
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
registry/find_guest_contract:   ~21 ns
```

---

## Optimization Tips

### 1. Batch Operations

Instead of multiple FFI calls:
```python
# Bad: Multiple FFI calls
for contract_id in contract_ids:
    handle = rt.find_guest_contract(contract_id, 1)

# Good: Single FFI call
handles = rt.find_all_by_contract(contract_id, 1)
```

### 2. Reuse Guards

```python
# Bad: Resolve on every call
for data in dataset:
    guard = rt.resolve_plugin(handle)
    result = call_plugin(guard.interface, data)

# Good: Resolve once, call many times
guard = rt.resolve_plugin(handle)
for data in dataset:
    result = call_plugin(guard.interface, data)
```

### 3. Choose the Right Language

For hot paths called millions of times:
- C++: ~10-20 ns overhead
- Python ctypes: ~670 ns overhead
- Difference: 30-60x

If your hot path is truly performance-critical, consider C++ or Lua.

---

## Call Arena (zero-allocation returns for VM guests)

### What it is

A `CallArena` is a small per-call **bump allocator** the host hands to a VM
dispatch call. The guest writes its variable-size return values (strings,
arrays) into the arena's region instead of calling `host->alloc` once per value.
The arena is a 40-byte `#[repr(C)]` struct: a primary `[base, end)` bump region
(an inline buffer owned by the caller) plus a fallback chain of host-allocated
overflow blocks for returns larger than the primary region.

The host caller **resets** the arena at the start of each call (a single pointer
rewind, freeing any overflow blocks in one pass). So after a warmup phase the
common case — a small string return — is served entirely from the bump region
with **zero host allocations**.

### Why it matters

Without the arena, every dispatch call that returns a string does one
`host->alloc` + one `host->free` per value. The arena turns the steady-state
return path into a pointer increment, removing the allocator round trip from hot
dispatch loops. For a 10,000-iteration echo loop the integration tests assert the
host allocator is hit **zero** times after warmup.

### Lifetime rule

> A view returned from an arena-backed call is valid **until the next
> arena-backed call on the same caller.**

The caller resets the arena at the start of each call, which invalidates the
previous call's arena allocations. Guests **never free** arena memory — the
arena owns it and reclaims it on reset. If you need a return value to outlive the
next call, copy it out, or use the explicit `alloc`/`free` path instead of the
arena helper.

### Which paths use it

| Path | Uses arena? | How |
|---|---|---|
| JS (QuickJS) guest returns | Yes | `polyplug.arenaAlloc` bridge → `allocStringArena` in the guest SDK |
| Lua (LuaJIT) guest returns | Yes | `_polyplug_arena_alloc` bridge → `alloc_string_arena` in the guest SDK |
| Rust host callers | Yes | per-caller `CallArena` field threaded into VM dispatch when a return needs it |
| Native Rust / C++ / C# guest returns | N/A | returns are already **borrowed zero-allocation views** into guest-owned memory — nothing to allocate, so no arena is needed |
| Python guest returns | N/A | Python guests dispatch through **`DispatchType::Native`** (ctypes function pointers); the native ABI signature carries no arena, exactly like native Rust/C++ |

### Null-arena fallback

The VM dispatch ABI signature is always
`call(loader_data, instance, fn_id, args, out, arena)`. Passing a **null arena**
means "no arena": the guest bridge (`arenaAlloc` / `_polyplug_arena_alloc`) falls
back to per-value `host->alloc`. Host callers that cannot hold a per-caller arena
(e.g. the Lua/Python guest-side host-contract callers) pass null and remain
correct — just not zero-allocation. Every loader passes the arena slot, so the
signature is uniform across all languages.

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
│   3. Evaluate bundle JS, extract interface                       │
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
| `cached_context_single_call` | **90-100 ns** | Single dispatch with cached Context |
| `cached_context_10_calls` | 674-716 ns | 10 calls (~67-72 ns/call) |

#### Why This Is Minimal Overhead

The ~85 ns overhead is the theoretical minimum for QuickJS dispatch:

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
| **Native** | ~2 ns | Direct function pointer |
| **.NET** | **~8 ns** | CLR `[UnmanagedCallersOnly]` function pointer |
| **Lua** | **~35 ns** | LuaJIT FFI + mlua |
| **QuickJS** | **~95 ns** | Per-bundle Runtime + Cached Context |
| **Python** | ~13 µs (GIL) / ~63 ns (cached) | PyO3 GIL + callable |

**.NET dispatch is near-native** thanks to `[UnmanagedCallersOnly]` function pointers. Lua is the fastest VM loader due to LuaJIT's extremely efficient FFI.

---

## Loader Dispatch Benchmarks

> **All numbers below are from actual benchmark runs on the current codebase.**
> Run `cargo bench -p polyplug --bench contract_dispatch`, `cargo bench -p polyplug_js`, etc. to reproduce.

### Native (Rust/C++/NativeAOT Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `noop` | **2.2 ns** | Trivial function call (add(0,0)) |
| `struct_arg_and_return` | **2.2 ns** | Struct args with return value |
| `buffer_arg` | **30 ns** | 4096-byte buffer operation |
| `cross_plugin` | **43 ns** | find_guest_contract + resolve + dispatch |

**Native dispatch is essentially zero overhead** - direct function pointer calls with no VM layer.

### QuickJS (JS Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `cached_context_single_call` | **90-100 ns** | Single dispatch with cached Context |
| `cached_context_10_calls` | 674-716 ns | 10 calls (~67-72 ns/call) |

### Lua (LuaJIT Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `vm_dispatch_single_call` | **34-37 ns** | VM dispatch via mlua |
| `vm_dispatch_10_calls` | 339-382 ns | 10 calls (~34-38 ns/call) |
| `cached_function_single_call` | **33 ns** | Cached function dispatch |
| `cached_function_10_calls` | 328-371 ns | 10 cached calls (~33-37 ns/call) |
| `create_unsafe_vm` | 85-99 µs | One-time VM creation cost |

**Lua is the fastest VM loader** - LuaJIT's FFI provides near-native performance.

### Python (CPython Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `gil_acquire_and_call` | **12.7-13.8 µs** | GIL acquisition + function call |
| `gil_acquire_and_10_calls` | 12.8-12.9 µs | GIL + 10 calls (GIL amortized) |
| `gil_acquire_only` | 37 ns | GIL acquisition only |
| `cached_function_single_call` | **59-67 ns** | Cached function (GIL already held) |
| `cached_function_10_calls` | 290-335 ns | 10 cached calls (~29-34 ns/call) |

**Key insight**: Python's GIL acquisition dominates overhead (~13 µs). Once GIL is held, cached dispatch is fast (~63 ns). For batch operations, acquire GIL once and make multiple calls.

### .NET (CLR Guest Plugins)

| Benchmark | Time | Description |
|-----------|------|-------------|
| `clr_init_call` | **7.4-8.3 ns** | Real CLR dispatch via `[UnmanagedCallersOnly]` |
| `clr_init_10_calls` | 70-76 ns | 10 calls (~7-7.6 ns/call) |
| `native_function_pointer_call` | 1.3-1.5 ns | Native baseline for comparison |

**.NET dispatch is near-native** - only ~5x slower than a native function pointer call. The `[UnmanagedCallersOnly]` attribute exposes native function pointers from the CLR, enabling zero-overhead interop.

### Summary

| Loader Type | Dispatch Overhead | Best For |
|-------------|-------------------|----------|
| **Native** | ~2 ns | Maximum performance |
| **.NET** | ~8 ns | Near-native with CLR ecosystem |
| **Lua** | ~35 ns | Fastest VM dispatch, embedded scripting |
| **QuickJS** | ~95 ns | Fast VM dispatch, JS ecosystem |
| **Python** | ~13 µs (GIL) / ~63 ns (cached) | Data science, ML ecosystem |

### Performance Insights

1. **Native is zero overhead** - Direct function pointer calls
2. **.NET is near-native** - `[UnmanagedCallersOnly]` enables ~8 ns dispatch through CLR
3. **Lua is the fastest VM loader** - LuaJIT's FFI provides ~35 ns dispatch
4. **QuickJS follows closely** - ~95 ns with cached context architecture
5. **Python's GIL is the bottleneck** - ~13 µs to acquire GIL, but only ~63 ns once held
6. **All VM loaders are "fast enough"** - Even Python's 13 µs is negligible for functions >100 µs

---

## See Also

- [ABI Architecture](./ABI_ARCHITECTURE.md)
- [ABI Types](./abi_types.md)
- [Python README](../sdks/python/host/README.md)
- [C++ README](../sdks/cpp/host/README.md)
- [Lua README](../sdks/lua/host/README.md)
- [JavaScript README](../sdks/js/host/README.md)