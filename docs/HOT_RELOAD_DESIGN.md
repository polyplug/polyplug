# Hot-Reload Notification Design

## Overview

This document describes the hot-reload notification system for polyplug. The design achieves:
- **Zero overhead** on the hot path (no per-call checks)
- **Callback-based coordination** — host destroys instances before reload
- **Clean API** — app developers see contract objects, not interfaces/guards
- **Actionable notifications** — host knows exactly what to do

## Terminology Note

This document uses the following terminology (current as of v1.1):
- **GuestContractInterface**: The interface struct a plugin provides for the host to call
- **HostApi**: The runtime's ABI table provided to guests

Interfaces are stored in `RuntimeStore` as interface slots guarded by a single `RwLock`. On reload, the slot is swapped in place via `apply_reload_swap` under the write guard.

---

## Core Concepts

### 1. Callback-Based Coordination

The runtime notifies the host before and after interface swap, plus failure case. The host is responsible for tracking and destroying all guest contract instances it has created.

**Critical clarification:** Every host-created instance is real: the generated `create_instance` invokes the author factory (`polyplug_create_<plugin>` in Rust/C++, the registered factory in C#/Python) and boxes the implementation — together with its `HostContext`/host pointer — into `GuestContractInstance.data`. The host-side **caller wrappers** each own one such instance; destroying a wrapper destroys its instance. There is no process-wide singleton implementation and no static plugin storage.

### 2. Hidden Implementation

The generated caller wrappers hide the resolved `GuestContractInterface` pointer from the application developer. They only see:

```cpp
auto decoder = PipelineDecoder::create(rt, contract_id);  // Creates wrapper + plugin instance
auto result = decoder.decode(input);  // Dispatches into that instance
decoder.reset();  // Destroys the instance — or let it go out of scope
```

### 3. Three-Phase Notification

The runtime notifies the host before and after interface swap, plus failure case:

- **Preparing**: "I want to reload this bundle. Destroy your caller wrappers (drop all instances)."
- **Reloaded**: "Reload complete. You can create new caller wrappers (pointing to new interface)."
- **Failed**: "Reload aborted - old interface kept, no swap occurred."

### 4. Leak Check (Informational, Non-Blocking)

The `Preparing` callback fires exactly **once**; there is no retry loop. After
the callback returns, the runtime checks `Arc::strong_count` on each interface
slot for the bundle. If any count is still greater than 1 — meaning the host may
not have dropped all caller wrappers — the runtime emits a warning via
`emit_warning` and **proceeds with the reload anyway**. The check is purely
informational; it never blocks, retries, or aborts the reload.

The safety contract is therefore on the host: it MUST drop all instances inside
the `Preparing` callback. A dangling wrapper that calls into a swapped interface
is the host's responsibility.

### 5. Failure Handling

A reload fails only if the new library fails to load or its `polyplug_init`
returns an error. In that case the runtime:
1. Closes the reload window — no interface swap occurs
2. Fires the `Failed` notification with a reason string
3. Returns the error from `reload_bundle()`

The old interface stays active and valid.

---

## API Design (Rust)

### ReloadPhase Struct

`ReloadPhase` is an FFI-safe `#[repr(C)]` struct (a tagged union — `phase_type`
selects the active variant), not a Rust enum. There is no `retry_count` field.

```rust
#[repr(u32)]
pub enum ReloadPhaseType {
    Preparing = 0,  // BEFORE interface swap — host must destroy instances
    Reloaded  = 1,  // AFTER interface swap — host can create new instances
    Failed    = 2,  // reload aborted — old interface kept, no swap occurred
    Unloading = 3,  // BEFORE a bundle is invalidated on unload — host must quiesce
}

/// FFI-safe reload phase for hot-reload callbacks.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ReloadPhase {
    pub phase_type:  ReloadPhaseType,
    pub bundle_id:   BundleId,
    pub bundle_name: StringView,  // borrowed; do not store beyond the callback
    pub reason:      StringView,  // null view unless phase_type == Failed
}
```

The FFI callback receives `ReloadPhase` **by const pointer** (`*const
ReloadPhase`) — never by value. The runtime always passes a non-null pointer;
the pointee (and the `StringView`s inside it) is valid only for the duration of
the call, so callbacks must copy anything they retain.

### RuntimeBuilder

```rust
impl RuntimeBuilder {
    /// Register a callback for reload notifications.
    ///
    /// The callback receives the opaque user-data pointer (forwarded unchanged from
    /// `RuntimeConfig::on_reload_user_data`) and a `ReloadPhase`.
    pub fn on_reload(
        self,
        cb: impl Fn(*mut core::ffi::c_void, ReloadPhase) + Send + Sync + 'static,
    ) -> Self;
}
```

### RuntimeConfig

```rust
pub struct RuntimeConfig {
    /// Version compatibility policy for loaded bundles. (offset 0)
    pub compatibility: Compatibility,

    /// How a bundle's loader-owned resources are reclaimed on unload. (offset 4)
    /// `UnloadMode { Retire (default), Reclaim }`. See UNLOAD_DESIGN.md.
    pub unload_mode: UnloadMode,

    /// Whether hot-reload is enabled. Default: false (offset 8)
    pub hot_reload_enabled: bool,

    /// Optional FFI callback invoked for each reload phase. (offset 16)
    /// The first argument is the opaque `on_reload_user_data` pointer; the
    /// second is a const pointer to the phase — always non-null, valid only
    /// for the duration of the call.
    pub on_reload: Option<unsafe extern "C" fn(*mut core::ffi::c_void, *const ReloadPhase)>,

    /// Opaque user-data pointer forwarded to `on_reload` as its first argument. (offset 24)
    /// Owned by the host; the runtime only forwards it, never reads or frees it.
    pub on_reload_user_data: *mut core::ffi::c_void,

    /// Optional logger callback (offset 32), its user_data (offset 40), and the
    /// maximum delivered LogLevel (u32, offset 48). See ABI_ARCHITECTURE.md.
    pub log: Option<unsafe extern "C" fn(*mut core::ffi::c_void, u32, StringView, StringView)>,
    pub log_user_data: *mut core::ffi::c_void,
    pub log_max_level: u32,
}
// sizeof(RuntimeConfig) == 56, align 8 on 64-bit.
```

#### Why `hot_reload_enabled` Defaults to `false`

Hot-reload is **opt-in by design**, not because it's unsafe, but because it requires **host-side coordination**:

1. **Callback Registration**: The host must register an `on_reload` callback
2. **Instance Tracking**: The host must track instances per bundle and destroy them on `Preparing`
3. **Error Handling**: The host must handle `Failed` notifications
4. **Per-Call Overhead**: Guards re-resolve interface on each call (~10-50ns)

If an application doesn't need hot-reload, it shouldn't pay these costs.

#### Error When Disabled

```rust
let rt = Runtime::builder().build()?;  // hot_reload_enabled = false (default)
rt.reload_bundle(path)?;  // Returns Err(RuntimeError::HotReloadDisabled)
```

### Example: Enabling Hot-Reload

```rust
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_abi::runtime::{Compatibility, ReloadPhaseType, UnloadMode};

let config = RuntimeConfig {
    compatibility: Compatibility::Strict,
    unload_mode: UnloadMode::Retire,  // default; Reclaim opts into dlclose/VM drop on unload
    hot_reload_enabled: true,  // REQUIRED for reload_bundle()
    on_reload: None,
    on_reload_user_data: core::ptr::null_mut(),
};

let rt = Runtime::builder()
    .config(config)
    .on_reload(|_user_data, phase| match phase.phase_type {
        ReloadPhaseType::Preparing => {
            // Destroy all caller wrappers for phase.bundle_id here.
        }
        ReloadPhaseType::Reloaded => {
            println!("Reloaded bundle {:?}", phase.bundle_id);
        }
        ReloadPhaseType::Failed => {
            eprintln!("ERROR: Reload failed for bundle {:?}", phase.bundle_id);
        }
        ReloadPhaseType::Unloading => {
            // Fired before unload_bundle invalidates the bundle: quiesce / drop
            // all caller wrappers and instances for phase.bundle_id here.
        }
    })
    .build()?;
```

---

## Language-Specific APIs

For language-specific API details and examples, see the SDK documentation:

| Language | Documentation |
|----------|---------------|
| **C++** | [sdks/cpp/README.md](../sdks/cpp/README.md) |
| **Python** | [sdks/python/README.md](../sdks/python/README.md) |
| **C#** | [sdks/csharp/README.md](../sdks/csharp/README.md) |
| **Lua** | [sdks/lua/README.md](../sdks/lua/README.md) |
| **JavaScript** | [sdks/js/README.md](../sdks/js/README.md) |

---

## Reload Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HOT-RELOAD FLOW                                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  INITIAL STATE                                                      │
│  ─────────────                                                      │
│  Host holds instances for bundle (decoder, encoder, etc.)           │
│                                                                      │
│  RELOAD TRIGGERED                                                   │
│  ─────────────────                                                  │
│                                                                      │
│  1. Runtime: on_reload(Preparing { bundle_id })                     │
│     │                                                                │
│     ▼                                                                │
│  2. Host: instances[bundle_id].clear()                              │
│     │                                                                │
│     ├─ decoder_instance destroyed                                   │
│     ├─ encoder_instance destroyed                                   │
│     │                                                                │
│     ▼                                                                │
│  3. Runtime: Arc::strong_count check (informational only) — emits   │
│     a warning if refs remain, then proceeds with the reload anyway. │
│     │                                                                │
│     ▼                                                                │
│  4. Runtime: loader.reload() → load + polyplug_init                 │
│     │                                                                │
│     ├─ FAILURE ─► on_reload(Failed { bundle_id, reason })           │
│     │            keep old interface, return error                   │
│     │                                                                │
│     ▼ SUCCESS                                                       │
│  5. Runtime: apply_reload_swap(new_interface)  ← RwLock write      │
│     │                                                                │
│     ▼                                                                │
│  6. Runtime: on_reload(Reloaded { bundle_id })                      │
│     │                                                                │
│     ▼                                                                │
│  7. Host: Can create new instances now                              │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

The reload is a single attempt: `Preparing` fires once, the leak check is
informational, and the reload proceeds regardless. There is no waiting, no
retry loop, and no max-retry abort.

---

## Key Design Decisions

### 1. Why Hide Guard and Interface?

- **Simplicity**: App developers see clean contract objects
- **Safety**: Can't accidentally misuse interface/guard
- **Encapsulation**: Implementation details stay hidden
- **RAII**: Automatic cleanup when instance goes out of scope

### 2. Why Factory Method Instead of Constructor?

```cpp
// GOOD: Factory method can return nullopt
auto decoder = PipelineDecoder::create(rt);
if (!decoder) { /* handle error */ }

// BAD: Constructor would need to throw
PipelineDecoder decoder(rt);  // Throws on error? Inconsistent with C++ patterns.
```

### 3. Why an Informational Leak Check Instead of Retries?

- **Determinism**: A single reload attempt has predictable, bounded timing
- **No hangs**: The runtime never blocks waiting for the host to drop refs
- **Observability**: A warning surfaces a suspected leak without aborting
- **Clear contract**: The host owns instance lifetime in the `Preparing` callback

### 4. Why Per-Bundle Tracking?

- **Granularity**: Only destroy instances for the bundle being reloaded
- **Efficiency**: Don't need to destroy all instances
- **Simplicity**: Clear mental model for host developers

### 5. Why Fire `Failed` on Init Failure?

- **Visibility**: The `Failed` notification tells the host exactly what happened
- **Safety**: No swap occurs, so the old interface stays valid
- **Recovery**: Host can fix the bundle and trigger the reload again

### 6. Why Opt-In via `hot_reload_enabled`?

- Most applications don't need hot-reload
- Those that do must implement the callback pattern
- Forces conscious decision to accept the complexity
- Prevents accidental misuse where reload silently fails

---

## See Also

- [UNLOAD_DESIGN.md](./UNLOAD_DESIGN.md) — True unload design (generation-counted handles; Phases 1–4 shipped: invalidate-only unload, VM reclaim, native opt-in `dlclose` reclaim + Python `sys.modules` purge under `UnloadMode::Reclaim`; arena retain-and-rewind and D11 deferred)
- [PERFORMANCE.md](./PERFORMANCE.md) — Hot-reload safety architecture and overhead
- [ABI_ARCHITECTURE.md](./ABI_ARCHITECTURE.md) — ABI layer design
- [SDK Examples](../examples/) — Working code for all languages