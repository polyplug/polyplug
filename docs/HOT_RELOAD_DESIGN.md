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

## API Design

### Runtime Side (Rust)

```rust
/// Notification phase for hot-reload
#[derive(Debug, Clone)]
pub enum ReloadPhase {
    /// BEFORE vtable swap - host must destroy instances
    Preparing {
        bundle_id: u64,
        bundle_name: String,
        /// 0 = first attempt, 1+ = retry (host missed some instances)
        retry_count: u32,
    },
    /// AFTER vtable swap - host can create new instances
    Reloaded {
        bundle_id: u64,
        bundle_name: String,
    },
    /// Reload ABORTED - instances not destroyed after max retries
    /// Old vtable is kept, no swap occurred
    Failed {
        bundle_id: u64,
        bundle_name: String,
        /// Human-readable reason for failure
        reason: String,
    },
}

/// Callback type for reload notifications
pub type ReloadCallback = Arc<dyn Fn(ReloadPhase) + Send + Sync>;

impl RuntimeBuilder {
    /// Register a callback for reload notifications
    pub fn on_reload(self, cb: impl Fn(ReloadPhase) + Send + Sync + 'static) -> Self;
}
```

### Runtime Configuration

Hot-reload behavior is configured via `RuntimeConfig`. See `RUNTIME_CONFIG.md` for full documentation.

```rust
/// Configuration for the polyplug runtime.
pub struct RuntimeConfig {
    /// Maximum number of retry attempts for hot-reload operations.
    /// Default: 3
    pub hot_reload_max_retries: u32,
    
    /// Interval between hot-reload retry attempts.
    /// Default: 1 second
    pub hot_reload_retry_interval: Duration,
    
    /// Whether to abort the runtime when max retries are exhausted.
    /// Default: true
    pub hot_reload_abort_on_max_retries: bool,
}
```

**Field Descriptions:**

- `hot_reload_max_retries`: Number of retry attempts before giving up (default: 3)
- `hot_reload_retry_interval`: Time to wait between retries (default: 1 second)
- `hot_reload_abort_on_max_retries`: If `true`, abort reload after max retries. If `false`, retry indefinitely.

**Example:**

```rust
use polyplug::RuntimeConfig;
use std::time::Duration;

let config = RuntimeConfig {
    hot_reload_max_retries: 5,
    hot_reload_retry_interval: Duration::from_secs(2),
    hot_reload_abort_on_max_retries: false,  // Keep retrying forever
};

let rt = Runtime::builder()
    .config(config)
    .build()?;
```

### ReloadPhase Enum (Rust)

```rust
/// Phase of a hot-reload operation for notification callbacks.
#[derive(Debug, Clone)]
pub enum ReloadPhase {
    /// Bundle is being prepared for reload (before vtable swap).
    Preparing {
        bundle_id: u64,
        bundle_name: String,
        retry_count: u32,  // 0 = first attempt, 1+ = retry
    },
    /// Bundle has been successfully reloaded.
    Reloaded {
        bundle_id: u64,
        bundle_name: String,
    },
    /// Bundle reload failed.
    Failed {
        bundle_id: u64,
        bundle_name: String,
        reason: String,  // Human-readable error description
    },
}
```

**Variant Details:**

- `Preparing`: Fired BEFORE vtable swap. Host should destroy all instances for this bundle.
  - `retry_count`: 0 on first attempt, increments on each retry (indicates missed cleanup)
- `Reloaded`: Fired AFTER vtable swap. Bundle is fresh, safe to create new instances.
- `Failed`: Fired when reload is aborted. Old vtable is kept, no swap occurred.
  - `reason`: Human-readable description of why the reload failed

### Generated Host Code (C++ Example)

```cpp
// generated/host/host_callers.hpp

/// Host caller for contract `pipeline.Decoder`
/// 
/// RAII: Instance holds guard internally. When destroyed, guard drops
/// and Arc reference count decreases automatically.
class PipelineDecoder {
public:
    /// Factory method - creates instance or nullopt if not found
    static std::optional<PipelineDecoder> create(Runtime& rt, uint64_t min_version = 0) {
        auto handle = rt.find(PIPELINE_DECODER_CONTRACT_ID, min_version);
        if (handle == UINT64_MAX) {
            return std::nullopt;
        }
        
        auto guard = rt.resolve_plugin(handle);
        if (!guard) {
            return std::nullopt;
        }
        
        return PipelineDecoder(std::move(guard));
    }
    
    // Move-only (guard is not copyable)
    PipelineDecoder(PipelineDecoder&&) noexcept = default;
    PipelineDecoder& operator=(PipelineDecoder&&) noexcept = default;
    PipelineDecoder(const PipelineDecoder&) = delete;
    PipelineDecoder& operator=(const PipelineDecoder&) = delete;
    
    /// Call the `decode` function (function_id=0)
    /// @throws std::runtime_error on ABI error
    std::string decode(std::string_view input) {
        // Prepare input
        StringView in_sv{reinterpret_cast<const uint8_t*>(input.data()), input.size()};
        StringView out_sv{nullptr, 0};
        
        // Get function pointer from hidden guard
        const PluginInterface* iface = guard_.interface();
        if (!iface || iface->function_count == 0) {
            throw std::runtime_error("invalid interface");
        }
        
        // Call plugin function
        auto fn = reinterpret_cast<AbiError(*)(const void*, void*)>(iface->functions[0]);
        AbiError err = fn(&in_sv, &out_sv);
        
        if (err.code != 0) {
            throw std::runtime_error("plugin returned error: " + std::to_string(err.code));
        }
        
        // Convert output
        std::string result(reinterpret_cast<const char*>(out_sv.ptr), out_sv.len);
        
        // Free output buffer allocated by plugin
        if (out_sv.ptr) {
            polyplug_host_free(const_cast<uint8_t*>(out_sv.ptr), out_sv.len, 1);
        }
        
        return result;
    }
    
    /// Check if instance is valid
    explicit operator bool() const noexcept { return static_cast<bool>(guard_); }
    bool is_valid() const noexcept { return static_cast<bool>(guard_); }
    
    /// Explicitly destroy instance (optional - destructor does this too)
    void reset() noexcept { guard_ = PluginGuard{}; }
    
    /// Destructor - guard drops, Arc count decreases
    ~PipelineDecoder() = default;

private:
    explicit PipelineDecoder(PluginGuard guard) : guard_(std::move(guard)) {}
    
    // Hidden from application developer
    PluginGuard guard_;
    
    static constexpr uint64_t PIPELINE_DECODER_CONTRACT_ID = 0x12F3C106B0C3DC1EULL;
};
```

---

## Host Language APIs

### C++ API

```cpp
#include <polyplug/runtime.hpp>

// ReloadPhase struct
enum class ReloadPhaseType { Preparing, Reloaded, Failed };

struct ReloadPhase {
    ReloadPhaseType type;
    uint64_t bundle_id;
    std::string bundle_name;
    uint32_t retry_count;  // Preparing only
    std::string reason;    // Failed only
};

// Runtime configuration
struct RuntimeConfig {
    uint32_t hot_reload_max_retries = 3;
    uint64_t hot_reload_retry_interval_ms = 1000;
    bool hot_reload_abort_on_max_retries = true;
};

// Register reload callback (must be called before creating Runtime)
polyplug::Runtime::on_reload([](const ReloadPhase& phase) {
    switch (phase.type) {
        case ReloadPhaseType::Preparing:
            if (phase.retry_count == 0) {
                // First attempt - normal cleanup
                instances_[phase.bundle_id].clear();
            } else {
                // Retry - missed cleanup!
                std::cerr << "WARNING: Reload retry " << phase.retry_count << "\n";
                instances_[phase.bundle_id].clear();
            }
            break;
        case ReloadPhaseType::Reloaded:
            std::cout << "Reloaded: bundle " << phase.bundle_id << "\n";
            break;
        case ReloadPhaseType::Failed:
            std::cerr << "ERROR: Reload failed: " << phase.reason << "\n";
            break;
    }
});

// Set runtime config (must be called before creating Runtime)
polyplug::RuntimeConfig config;
config.hot_reload_max_retries = 5;
config.hot_reload_retry_interval_ms = 2000;
config.hot_reload_abort_on_max_retries = false;
polyplug::Runtime::set_config(config);

// Create runtime
auto rt = polyplug::Runtime::builder()
    .plugin_dir("/path/to/plugins")
    .build();
```

**Key Points:**

- `Runtime::on_reload()` and `Runtime::set_config()` must be called BEFORE creating a `Runtime` instance
- Callback receives `ReloadPhase` struct with phase-specific fields
- Config applies to all subsequently created `Runtime` instances

---

### Python API

```python
from polyplug import Runtime, RuntimeConfig, ReloadPhase, ReloadPhaseType

# ReloadPhase class
class ReloadPhase:
    type: ReloadPhaseType  # PREPARING, RELOADED, or FAILED
    bundle_id: int
    bundle_name: str
    retry_count: int       # Preparing only
    reason: str            # Failed only

# Runtime configuration
class RuntimeConfig:
    hot_reload_max_retries: int = 3
    hot_reload_retry_interval_ms: int = 1000
    hot_reload_abort_on_max_retries: bool = True

# Register reload callback (must be called before creating Runtime)
def on_reload(phase: ReloadPhase):
    if phase.type == ReloadPhaseType.PREPARING:
        if phase.retry_count == 0:
            # First attempt - normal cleanup
            instances.pop(phase.bundle_id, None)
        else:
            # Retry - missed cleanup!
            print(f"WARNING: Reload retry {phase.retry_count}")
            instances.pop(phase.bundle_id, None)
    elif phase.type == ReloadPhaseType.RELOADED:
        print(f"Reloaded: bundle {phase.bundle_id}")
    elif phase.type == ReloadPhaseType.FAILED:
        print(f"ERROR: Reload failed: {phase.reason}")

Runtime.on_reload(on_reload)

# Set runtime config (must be called before creating Runtime)
config = RuntimeConfig(
    hot_reload_max_retries=5,
    hot_reload_retry_interval_ms=2000,
    hot_reload_abort_on_max_retries=False
)
Runtime.set_config(config)

# Create runtime
rt = Runtime()
```

**Key Points:**

- `Runtime.on_reload()` and `Runtime.set_config()` are class methods - call before instantiation
- Callback receives `ReloadPhase` object with `type`, `bundle_id`, `bundle_name`, etc.
- Supports both ctypes (default) and cffi (faster) backends

---

### C# API

```csharp
using Polyplug;

// ReloadPhase class
public enum ReloadPhaseType { Preparing, Reloaded, Failed }

public sealed class ReloadPhase {
    public ReloadPhaseType Type { get; }
    public ulong BundleId { get; }
    public string BundleName { get; }
    public uint RetryCount { get; }      // Preparing only
    public string Reason { get; }        // Failed only
    
    // Helper methods
    public bool IsPreparing();
    public bool IsReloaded();
    public bool IsFailed();
}

// Runtime configuration
public class RuntimeConfig {
    public uint HotReloadMaxRetries { get; set; } = 3;
    public uint HotReloadRetryIntervalMs { get; set; } = 1000;
    public bool HotReloadAbortOnMaxRetries { get; set; } = true;
}

// Register reload callback (must be called before creating Runtime)
Runtime.OnReload(phase => {
    if (phase.IsPreparing()) {
        if (phase.RetryCount == 0) {
            // First attempt - normal cleanup
            instances.Remove(phase.BundleId);
        } else {
            Console.WriteLine($"WARNING: Reload retry {phase.RetryCount}");
            instances.Remove(phase.BundleId);
        }
    } else if (phase.IsReloaded()) {
        Console.WriteLine($"Reloaded: bundle {phase.BundleId}");
    } else if (phase.IsFailed()) {
        Console.WriteLine($"ERROR: Reload failed: {phase.Reason}");
    }
});

// Set runtime config (must be called before creating Runtime)
var config = new RuntimeConfig {
    HotReloadMaxRetries = 5,
    HotReloadRetryIntervalMs = 2000,
    HotReloadAbortOnMaxRetries = false
};
Runtime.SetConfig(config);

// Create runtime
var rt = Runtime.Builder()
    .PluginDir("/path/to/plugins")
    .Build();
```

**Key Points:**

- `Runtime.OnReload()` and `Runtime.SetConfig()` are static methods - call before instantiation
- `ReloadPhase` provides helper methods (`IsPreparing()`, `IsReloaded()`, `IsFailed()`)
- Config and callback apply to all subsequently created `Runtime` instances

---

### Lua API

```lua
local polyplug = require("polyplug")
local ReloadPhase = require("polyplug.reload_phase")

-- ReloadPhase table
-- {
--     type: number,        -- 0=Preparing, 1=Reloaded, 2=Failed
--     bundle_id: uint64_t,
--     bundle_name: string,
--     retry_count: number,  -- Preparing only
--     reason: string        -- Failed only
-- }

-- Runtime configuration
-- {
--     hot_reload_max_retries: number,
--     hot_reload_retry_interval_ms: number,
--     hot_reload_abort_on_max_retries: boolean
-- }

-- Register reload callback (must be called before creating Runtime)
polyplug.on_reload(function(phase)
    if phase.type == ReloadPhase.PREPARING then
        if phase.retry_count == 0 then
            -- First attempt - normal cleanup
            instances[phase.bundle_id] = nil
        else
            -- Retry - missed cleanup!
            print("WARNING: Reload retry " .. phase.retry_count)
            instances[phase.bundle_id] = nil
        end
    elseif phase.type == ReloadPhase.RELOADED then
        print("Reloaded: bundle " .. tostring(phase.bundle_id))
    elseif phase.type == ReloadPhase.FAILED then
        print("ERROR: Reload failed: " .. phase.reason)
    end
end)

-- Set runtime config (must be called before creating Runtime)
polyplug.set_config({
    hot_reload_max_retries = 5,
    hot_reload_retry_interval_ms = 2000,
    hot_reload_abort_on_max_retries = false
})

-- Create runtime
local rt = polyplug.Runtime.new()
```

**Key Points:**

- `polyplug.on_reload()` and `polyplug.set_config()` are module-level functions
- Must be called BEFORE `Runtime.new()` is called
- Uses LuaJIT FFI for zero-overhead FFI calls
- `ReloadPhase` is a Lua table with phase-specific fields

---

### JavaScript (Deno) API

```typescript
import { Runtime, ReloadPhase, ReloadPhaseType, RuntimeConfig } from "./polyplug.ts";

// ReloadPhase class
class ReloadPhase {
    type: ReloadPhaseType;  // PREPARING, RELOADED, FAILED
    bundleId: bigint;
    bundleName: string;
    retryCount: number;     // Preparing only
    reason: string;         // Failed only
}

// Runtime configuration
class RuntimeConfig {
    hotReloadMaxRetries: number = 3;
    hotReloadRetryIntervalMs: bigint = 1000n;
    hotReloadAbortOnMaxRetries: boolean = true;
}

// Register reload callback (must be called before creating Runtime)
Runtime.onReload((phase: ReloadPhase) => {
    if (phase.type === ReloadPhaseType.PREPARING) {
        if (phase.retryCount === 0) {
            // First attempt - normal cleanup
            instances.delete(phase.bundleId);
        } else {
            console.warn(`WARNING: Reload retry ${phase.retryCount}`);
            instances.delete(phase.bundleId);
        }
    } else if (phase.type === ReloadPhaseType.RELOADED) {
        console.log(`Reloaded: bundle ${phase.bundleId}`);
    } else if (phase.type === ReloadPhaseType.FAILED) {
        console.error(`ERROR: Reload failed: ${phase.reason}`);
    }
});

// Set runtime config (must be called before creating Runtime)
const config: RuntimeConfig = {
    hotReloadMaxRetries: 5,
    hotReloadRetryIntervalMs: 2000n,
    hotReloadAbortOnMaxRetries: false
};
Runtime.setConfig(config);

// Create runtime
const lib = openPolyplug("/path/to/libpolyplug.so");
const rt = runtimeNew(lib);
```

**Key Points:**

- `Runtime.onReload()` and `Runtime.setConfig()` are static methods
- Must be called BEFORE `runtimeNew()` is called
- Uses `bigint` for 64-bit integers (bundle IDs, contract IDs)
- Supports `[Symbol.dispose]()` for automatic cleanup with `using` keyword

---

## Usage Examples

### C++ Host Application

```cpp
#include <polyplug/runtime.hpp>
#include <generated/host/host_callers.hpp>
#include <unordered_map>
#include <vector>

class PluginManager {
public:
    PluginManager() {
        rt_ = polyplug::Runtime::builder()
            .plugin_dir("plugins")
            .on_reload([this](ReloadPhase phase) {
                handle_reload_notification(phase);
            })
            .build();
        
        polyplug::loaders::register_native(rt_);
    }
    
    void load_bundle(const std::string& path) {
        rt_.load_bundle(path);
    }
    
    // Create and track instances per bundle
    std::optional<PipelineDecoder> create_decoder(uint64_t bundle_id) {
        auto decoder = PipelineDecoder::create(rt_);
        if (decoder) {
            instances_[bundle_id].push_back(std::move(*decoder));
        }
        return decoder;
    }

private:
    polyplug::Runtime rt_;
    
    // Track instances per bundle for cleanup during reload
    std::unordered_map<uint64_t, std::vector<PipelineDecoder>> instances_;
    
    void handle_reload_notification(const ReloadPhase& phase) {
        // Simple switch - just check the type field
        switch (phase.type) {
            case ReloadPhaseType::Preparing:
                // BEFORE swap - destroy all instances for this bundle
                if (phase.retry_count == 0) {
                    // First attempt - normal cleanup
                    instances_[phase.bundle_id].clear();
                    std::cout << "Prepared for reload: bundle " << phase.bundle_id << "\n";
                } else {
                    // Retry - we missed something!
                    std::cerr << "WARNING: Reload stuck for bundle " << phase.bundle_id
                              << ", retry " << phase.retry_count << "\n";
                    
                    // Force cleanup of any remaining instances
                    instances_[phase.bundle_id].clear();
                }
                break;
                
            case ReloadPhaseType::Reloaded:
                // AFTER swap - bundle is fresh, can create new instances
                std::cout << "Reloaded: bundle " << phase.bundle_id << "\n";
                break;
                
            case ReloadPhaseType::Failed:
                // Reload ABORTED - instances not destroyed after max retries
                // Old vtable is still active, no swap occurred
                std::cerr << "ERROR: Reload failed for bundle " << phase.bundle_id
                          << ": " << phase.reason << "\n";
                break;
        }
    }
};

// Usage
int main() {
    PluginManager mgr;
    mgr.load_bundle("plugins/decoder_plugin");
    
    // Create instance - RAII manages lifetime
    {
        auto decoder = PipelineDecoder::create(mgr.runtime());
        if (decoder) {
            auto result = decoder->decode("hello,world");
            std::cout << "Result: " << result << "\n";
        }
        // decoder destroyed here - guard drops, Arc count decreases
    }
    
    return 0;
}
```

### Python Host Application

```python
from polyplug import Runtime, ReloadPhase
from generated.host_callers import PipelineDecoder

class PluginManager:
    def __init__(self):
        self.rt = Runtime()
        self.rt.on_reload(self._on_reload)
        self._instances = {}  # bundle_id -> list of instances
    
    def create_decoder(self, bundle_id: int) -> PipelineDecoder:
        decoder = PipelineDecoder.create(self.rt)
        if decoder:
            self._instances.setdefault(bundle_id, []).append(decoder)
        return decoder
    
    def _on_reload(self, phase: ReloadPhase):
        # Simple if/elif - just check the type attribute
        if phase.type == ReloadPhase.PREPARING:
            self._instances.pop(phase.bundle_id, None)
            if phase.retry_count > 0:
                print(f"WARNING: Reload retry {phase.retry_count}")
        elif phase.type == ReloadPhase.RELOADED:
            print(f"Bundle {phase.bundle_id} reloaded")
        elif phase.type == ReloadPhase.FAILED:
            print(f"ERROR: Reload failed: {phase.reason}")
```

### C# Host Application

```csharp
using Polyplug;
using Generated;

public class PluginManager : IDisposable
{
    private readonly Runtime _rt;
    private readonly Dictionary<ulong, List<IDisposable>> _instances = new();
    
    public PluginManager()
    {
        _rt = Runtime.Builder()
            .PluginDir("plugins")
            .OnReload(HandleReload)
            .Build();
    }
    
    public PipelineDecoder? CreateDecoder(ulong bundleId)
    {
        var decoder = PipelineDecoder.Create(_rt);
        if (decoder != null)
        {
            if (!_instances.ContainsKey(bundleId))
                _instances[bundleId] = new List<IDisposable>();
            _instances[bundleId].Add(decoder);
        }
        return decoder;
    }
    
    private void HandleReload(ReloadPhase phase)
    {
        // Simple switch - just check the Type property
        switch (phase.Type)
        {
            case ReloadPhaseType.Preparing:
                if (_instances.TryGetValue(phase.BundleId, out var instances))
                {
                    foreach (var inst in instances)
                        inst.Dispose();
                    instances.Clear();
                }
                if (phase.RetryCount > 0)
                    Console.WriteLine($"WARNING: Reload retry {phase.RetryCount}");
                break;
                
            case ReloadPhaseType.Reloaded:
                Console.WriteLine($"Bundle {phase.BundleId} reloaded");
                break;
            
            case ReloadPhaseType.Failed:
                Console.WriteLine($"ERROR: Reload failed: {phase.Reason}");
                break;
        }
    }
    
    public void Dispose()
    {
        foreach (var instances in _instances.Values)
            foreach (var inst in instances)
                inst.Dispose();
        _instances.Clear();
    }
}
```

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

**Logging:** Uses existing `on_warning` callback, not stdout/stderr directly.

---

## Codegen Changes

### Factory Method Pattern

Generated contract classes use a factory method instead of a public constructor:

```cpp
// C++ example
class PipelineDecoder {
public:
    /// Factory method - creates instance or nullopt if not found
    static std::optional<PipelineDecoder> create(Runtime& rt, uint64_t min_version = 0) {
        auto handle = rt.find(PIPELINE_DECODER_CONTRACT_ID, min_version);
        if (handle == UINT64_MAX) {
            return std::nullopt;
        }
        
        auto guard = rt.resolve_plugin(handle);
        if (!guard) {
            return std::nullopt;
        }
        
        return PipelineDecoder(std::move(guard));
    }
    
private:
    explicit PipelineDecoder(PluginGuard guard) : guard_(std::move(guard)) {}
    PluginGuard guard_;
};
```

**Why factory method?**

- Can return `nullopt`/`None`/`null` on failure (no exceptions needed)
- Consistent with C++/C#/Python error handling patterns
- Hides `PluginGuard` construction details from application code

### Hidden Implementation Details

Generated code hides `PluginGuard` and `PluginInterface` from the public API:

```cpp
// PUBLIC API - what app developers see
class PipelineDecoder {
public:
    static std::optional<PipelineDecoder> create(Runtime& rt, uint64_t min_version);
    std::string decode(std::string_view input);
    bool is_valid() const noexcept;
    void reset() noexcept;
    
private:
    PluginGuard guard_;  // Hidden from public API
};

// INTERNAL - not exposed to app developers
struct PluginInterface {
    uint64_t contract_id;
    uint32_t contract_version;
    uint32_t function_count;
    const void** functions;
};
```

### is_valid() and reset() Methods

Generated contract classes expose two helper methods:

```cpp
/// Check if instance is valid (guard is not null)
explicit operator bool() const noexcept { return static_cast<bool>(guard_); }
bool is_valid() const noexcept { return static_cast<bool>(guard_); }

/// Explicitly destroy instance (optional - destructor does this too)
void reset() noexcept { guard_ = PluginGuard{}; }
```

**Usage:**

```cpp
auto decoder = PipelineDecoder::create(rt);
if (decoder && decoder->is_valid()) {
    auto result = decoder->decode(input);
    decoder->reset();  // Explicit cleanup (optional)
}
```

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

### 6. Why Use `on_warning` for Logging?

- **Flexibility**: Host decides how to handle warnings (log, alert, ignore)
- **Consistency**: Same mechanism used throughout polyplug
- **No stdout/stderr**: Runtime doesn't force output format

---
