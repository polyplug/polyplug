# Hot-Reload Notification Design

## Overview

This document describes the hot-reload notification system for polyplug. The design achieves:
- **Zero overhead** on the hot path (no per-call checks)
- **Automatic instance tracking** via Arc reference counting
- **Clean API** — app developers see contract objects, not vtables/guards
- **Actionable notifications** — host knows exactly what to do

---

## Core Concepts

### 1. Instance-Based Tracking

Each plugin contract instance holds an internal `PluginGuard` which contains an `Arc<VTableSlot>`. The Arc reference count IS the instance counter.

```
Arc::strong_count == 1  →  No instances exist (only registry holds it)
Arc::strong_count > 1   →  Instances exist (each instance holds an Arc)
```

### 2. Hidden Implementation

The generated contract classes hide `PluginGuard` and `PluginInterface` from the application developer. They only see:

```cpp
auto decoder = PipelineDecoder::create(rt, contract_id);
auto result = decoder.decode(input);
decoder.reset();  // Or let it go out of scope
```

### 3. Three-Phase Notification

The runtime notifies the host before and after vtable swap, plus failure case:

- **Preparing**: "I want to reload this bundle. Destroy your instances."
- **Reloaded**: "Reload complete. You can create new instances."
- **Failed**: "Reload aborted - instances not destroyed. Old vtable kept."

### 4. Retry Mechanism

If instances are still held after the initial `Preparing` notification:

1. Runtime waits for `hot_reload_retry_interval` (default: 1 second)
2. Fires `Preparing` again with incremented `retry_count`
3. Repeats until `hot_reload_max_retries` is reached
4. If `hot_reload_abort_on_max_retries=true`, fires `Failed` and aborts
5. If `hot_reload_abort_on_max_retries=false`, continues retrying indefinitely

This gives the host multiple chances to clean up leaked instances before the reload is aborted.

### 5. Stuck Detection and Abort

If the host doesn't destroy all instances after max retries, the runtime:
1. Sends `Failed` notification to host with reason string
2. Keeps old vtable (no swap occurred)
3. Logs warning via `on_warning` callback
4. Returns error from `reload_bundle()`

**This is safer than force-proceed** (could crash) or infinite retry (hangs).

The default behavior (`abort_on_max_retries=true`) aborts after 3 retries (~4 seconds total).
Set `abort_on_max_retries=false` to retry indefinitely (useful for development).

---

## API Design (Rust)

### ReloadPhase Enum

```rust
/// Phase of a hot-reload operation for notification callbacks.
#[derive(Debug, Clone)]
pub enum ReloadPhase {
    /// BEFORE vtable swap - host must destroy instances
    Preparing {
        bundle_id: u64,
        bundle_name: String,
        retry_count: u32,  // 0 = first attempt, 1+ = retry
    },
    /// AFTER vtable swap - host can create new instances
    Reloaded {
        bundle_id: u64,
        bundle_name: String,
    },
    /// Reload ABORTED - old vtable kept, no swap occurred
    Failed {
        bundle_id: u64,
        bundle_name: String,
        reason: String,
    },
}
```

### RuntimeBuilder

```rust
impl RuntimeBuilder {
    /// Register a callback for reload notifications
    pub fn on_reload(self, cb: impl Fn(ReloadPhase) + Send + Sync + 'static) -> Self;
}
```

### RuntimeConfig

```rust
pub struct RuntimeConfig {
    /// Whether hot-reload is enabled. Default: false
    pub hot_reload_enabled: bool,
    
    /// Maximum retry attempts. Default: 3
    pub hot_reload_max_retries: u32,
    
    /// Interval between retries. Default: 1 second
    pub hot_reload_retry_interval: Duration,
    
    /// Whether to abort after max retries. Default: true
    pub hot_reload_abort_on_max_retries: bool,
}
```

#### Why `hot_reload_enabled` Defaults to `false`

Hot-reload is **opt-in by design**, not because it's unsafe, but because it requires **host-side coordination**:

1. **Callback Registration**: The host must register an `on_reload` callback
2. **Instance Tracking**: The host must track instances per bundle and destroy them on `Preparing`
3. **Error Handling**: The host must handle `Failed` notifications
4. **Per-Call Overhead**: Guards re-resolve vtable on each call (~10-50ns)

If an application doesn't need hot-reload, it shouldn't pay these costs.

#### Error When Disabled

```rust
let rt = Runtime::builder().build()?;  // hot_reload_enabled = false (default)
rt.reload_bundle(path)?;  // Returns Err(PolyplugError::HotReloadDisabled)
```

### Example: Enabling Hot-Reload

```rust
use polyplug::RuntimeConfig;
use std::time::Duration;

let config = RuntimeConfig {
    hot_reload_enabled: true,  // REQUIRED for reload_bundle()
    hot_reload_max_retries: 5,
    hot_reload_retry_interval: Duration::from_secs(2),
    hot_reload_abort_on_max_retries: false,
};

let rt = Runtime::builder()
    .config(config)
    .on_reload(|phase| match phase {
        ReloadPhase::Preparing { bundle_id, retry_count, .. } => {
            if retry_count > 0 {
                eprintln!("WARNING: Reload retry {} for bundle {}", retry_count, bundle_id);
            }
            // Destroy instances for this bundle
        }
        ReloadPhase::Reloaded { bundle_id, .. } => {
            println!("Reloaded bundle {}", bundle_id);
        }
        ReloadPhase::Failed { bundle_id, reason, .. } => {
            eprintln!("ERROR: Reload failed for bundle {}: {}", bundle_id, reason);
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
│  Arc::strong_count = 3                                              │
│  (registry + decoder_instance + encoder_instance)                   │
│                                                                      │
│  RELOAD TRIGGERED                                                   │
│  ─────────────────                                                  │
│                                                                      │
│  1. Runtime: on_reload(Preparing { bundle_id, retry: 0 })           │
│     │                                                                │
│     ▼                                                                │
│  2. Host: instances[bundle_id].clear()                              │
│     │                                                                │
│     ├─ decoder_instance destroyed → guard drops                     │
│     ├─ encoder_instance destroyed → guard drops                     │
│     │                                                                │
│     ▼                                                                │
│  3. Arc::strong_count = 1 (only registry holds it)                  │
│     │                                                                │
│     ▼                                                                │
│  4. Runtime: swap_vtable(new_vtable)  ← ATOMIC                      │
│     │                                                                │
│     ▼                                                                │
│  5. Runtime: on_reload(Reloaded { bundle_id })                      │
│     │                                                                │
│     ▼                                                                │
│  6. Host: Can create new instances now                              │
│                                                                      │
│  ─────────────────────────────────────────────────────────────────  │
│                                                                      │
│  IF STUCK (Arc count > 1 after 1 second):                           │
│                                                                      │
│  2b. Runtime: on_reload(Preparing { bundle_id, retry: 1 })          │
│      Host: "I missed something!" → Force cleanup                    │
│      Wait 1 second...                                               │
│                                                                      │
│  2c. Runtime: on_reload(Preparing { bundle_id, retry: 2 })          │
│      Host: Still stuck? Search for leaks                            │
│      Wait 1 second...                                               │
│                                                                      │
│  2d. Runtime: on_reload(Preparing { bundle_id, retry: 3 })          │
│      Host: Last chance!                                             │
│      Wait 1 second...                                               │
│                                                                      │
│  ─────────────────────────────────────────────────────────────────  │
│                                                                      │
│  IF STILL STUCK AFTER MAX RETRIES (3):                              │
│                                                                      │
│  3. Runtime: emit_warning("reload stuck, aborting...")              │
│     │                                                                │
│     ▼                                                                │
│  4. Runtime: on_reload(Failed { bundle_id, reason })                │
│     │                                                                │
│     ▼                                                                │
│  5. Runtime: Return error, keep old vtable                          │
│     │                                                                │
│     ▼                                                                │
│  6. Host: Log error, alert user, or investigate leak                │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Retry and Abort Behavior

| Retry | Action | Timeout |
|-------|--------|---------|
| 0 | First `Preparing` notification | 1 second |
| 1 | Second `Preparing` (retry) | 1 second |
| 2 | Third `Preparing` (retry) | 1 second |
| 3 | Fourth `Preparing` (retry) | 1 second |
| >3 | **ABORT** → `Failed` notification | - |

**Total time before abort: ~4 seconds** (initial + 3 retries)

---

## Key Design Decisions

### 1. Why Hide Guard and VTable?

- **Simplicity**: App developers see clean contract objects
- **Safety**: Can't accidentally misuse vtable/guard
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

### 3. Why Retry Count?

- **Debugging**: Host knows it missed cleanup
- **Recovery**: Host can search for leaked instances
- **Observability**: Logs show "stuck" situations

### 4. Why Per-Bundle Tracking?

- **Granularity**: Only destroy instances for the bundle being reloaded
- **Efficiency**: Don't need to destroy all instances
- **Simplicity**: Clear mental model for host developers

### 5. Why Abort After Max Retries?

- **Safety**: Force-proceed could cause use-after-free
- **Visibility**: `Failed` notification tells host exactly what happened
- **Recovery**: Host can investigate and fix the leak, then retry reload

### 6. Why Opt-In via `hot_reload_enabled`?

- Most applications don't need hot-reload
- Those that do must implement the callback pattern
- Forces conscious decision to accept the complexity
- Prevents accidental misuse where reload silently fails

---

## See Also

- [PERFORMANCE.md](./PERFORMANCE.md) — Hot-reload safety architecture and overhead
- [ABI_ARCHITECTURE.md](./ABI_ARCHITECTURE.md) — ABI layer design
- [SDK Examples](../examples/) — Working code for all languages