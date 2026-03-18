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

The generated contract classes hide `PluginGuard` and `PluginVTable` from the application developer. They only see:

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

### 4. Stuck Detection and Abort

If the host doesn't destroy all instances, the runtime:
1. Sends retry notifications with incrementing `retry_count` (1 second between retries)
2. After **3 retries**, aborts the reload
3. Sends `Failed` notification to host
4. Keeps old vtable (no swap)
5. Logs warning via `on_warning` callback

**This is safer than force-proceed** (could crash) or infinite retry (hangs).

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
// In RuntimeConfig (see RUNTIME_CONFIG.md)
pub struct RuntimeConfig {
    pub hot_reload_max_retries: u32,        // default: 3
    pub hot_reload_retry_interval: Duration, // default: 1 second
    pub hot_reload_abort_on_max_retries: bool, // default: true
    // ... other runtime options
}
```

### ReloadPhase Enum (Rust)

```rust
pub enum ReloadPhase {
    Preparing { bundle_id: u64, bundle_name: String, retry_count: u32 },
    Reloaded { bundle_id: u64, bundle_name: String },
    Failed { bundle_id: u64, bundle_name: String, reason: String },
}
```

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
        const PluginVTable* vt = guard_.vtable();
        if (!vt || vt->function_count == 0) {
            throw std::runtime_error("invalid vtable");
        }
        
        // Call plugin function
        auto fn = reinterpret_cast<AbiError(*)(const void*, void*)>(vt->functions[0]);
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

### ReloadPhase (C++)

```cpp
// Separate enum class - cleaner C++ style
enum class ReloadPhaseType { Preparing, Reloaded, Failed };

struct ReloadPhase {
    ReloadPhaseType type;
    uint64_t bundle_id;
    std::string bundle_name;
    uint32_t retry_count;  // Preparing only
    std::string reason;    // Failed only
};
```

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

## Implementation Checklist

See `.sisyphus/plans/hot-reload-notification.md` for detailed implementation tasks.