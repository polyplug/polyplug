# Architecture Clarifications: Singleton Implementations + Caller Wrappers

**Critical clarification about polyplug's instance model.**

## Terminology Note

This document uses the following terminology (current as of v1.1):
- **GuestContractInterface**: The interface struct a plugin provides for the host to call
- **HostApi**: The runtime's ABI table provided to guests

Interfaces are stored in `RuntimeStore` as interface slots guarded by a single `RwLock`. There is no separate slot wrapper struct around individual interfaces.

---

## The Truth About "Instances"

### What polyplug DOESN'T Have

❌ **NO factory pattern** that creates multiple plugin instances
❌ **NO per-instance state** on the plugin side
❌ **NO ability** for the host to spawn multiple instances of the same plugin

### What polyplug DOES Have

✅ **Singleton implementations** - Each contract has ONE implementation per loaded bundle
✅ **Caller wrappers** - Host creates multiple wrappers that reference the same interface
✅ **Callback-based lifecycle** - Host destroys instances before hot-reload completes

---

## Architecture Deep Dive

### Plugin Side: Singleton via `OnceLock`

Every plugin contract implementation is stored in a **static singleton**:

```rust
// examples/guests/rust/validator/generated/guest/interfaces.rs:50
pub static VALIDATOR_IMPL: OnceLock<Box<dyn PipelineValidatorPlugin>> = OnceLock::new();

pub fn set_validator_impl(impl_: Box<dyn PipelineValidatorPlugin>) -> Result<(), &'static str> {
    VALIDATOR_IMPL.set(impl_).map_err(|_| "validator already registered")
}
```

**Consequences:**
- `OnceLock::set()` can only succeed **once** per process lifetime
- Attempting to register a second implementation returns an error
- The plugin has exactly ONE implementation, period

### Host Side: Caller Wrappers (NOT Instances)

What the generated code creates are **caller wrappers**, not plugin instances:

```rust
// examples/hosts/rust/src/generated/host/host_callers.rs:415-420
pub struct PipelineValidatorContract {
    guard: PluginGuard,  // Holds reference to interface
}

impl PipelineValidatorContract {
    pub fn new(handle: GuestContractHandle, runtime: &'static Runtime) -> Option<Self> {
        let guard: PluginGuard = runtime.registry().resolve_guard(handle).ok()?;
        Some(PipelineValidatorContract { guard })  // New wrapper, SAME interface
    }
}
```

**What's actually happening:**
```rust
// Host creates THREE wrappers
let wrapper1 = PipelineValidatorContract::new(handle, runtime)?;  // wrapper 1
let wrapper2 = PipelineValidatorContract::new(handle, runtime)?;  // wrapper 2
let wrapper3 = PipelineValidatorContract::new(handle, runtime)?;  // wrapper 3

// All three call the SAME singleton implementation
wrapper1.validate(input)?;  // → VALIDATOR_IMPL (singleton)
wrapper2.validate(input)?;  // → VALIDATOR_IMPL (same singleton)
wrapper3.validate(input)?;  // → VALIDATOR_IMPL (same singleton)
```

### The `PluginGuard`: Reference to Interface

```rust
// crates/polyplug/src/registry.rs:43-62
pub struct PluginGuard {
    pub(crate) slot: Arc<InterfaceSlot>,  // Reference to shared interface
    _not_send: PhantomData<Cell<()>>,  // Intentionally !Send
}
```

**Purpose:**
- Holds reference to keep interface accessible during call
- **NOT** a per-instance state container
- **NOT** creating new plugin instances
- Just a reference-counted pointer wrapper

---

## Visual Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        PLUGIN SIDE (Guest)                          │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │  static VALIDATOR_IMPL: OnceLock<Box<dyn Validator>>      │    │
│  │         ↑                                                  │    │
│  │         │ SINGLETON - only ONE implementation exists       │    │
│  │         │                                                  │    │
│  │  [ValidatorImpl { ... }]                                   │    │
│  └────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
                              ↑
                              │ GuestContractInterface pointer
                              │ (registered once at init)
┌─────────────────────────────────────────────────────────────────────┐
│                         HOST SIDE (Runtime)                         │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │  RuntimeStore {                                            │    │
│  │    interface slot (RwLock-guarded)  ← ONE per contract     │    │
│  │  }                                                         │    │
│  └────────────────────────────────────────────────────────────┘    │
│                              ↑                                      │
│            ┌─────────────────┼─────────────────┐                   │
│            │                 │                 │                   │
│      ┌─────┴─────┐   ┌──────┴──────┐   ┌─────┴─────┐              │
│      │ Wrapper 1 │   │  Wrapper 2  │   │ Wrapper 3 │              │
│      │           │   │             │   │           │              │
│      └───────────┘   └─────────────┘   └───────────┘              │
│            │                 │                 │                   │
│            └─────────────────┴─────────────────┘                   │
│                              │                                      │
│                              ↓                                      │
│                    All call SAME singleton                          │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Why This Design?

### 1. **Simplicity**
- No instance lifecycle management complexity
- No per-instance state synchronization
- No factory pattern boilerplate

### 2. **Hot-Reload Safety**
- Callback-based coordination — host destroys instances before reload
- Clean interface swap without dangling pointers
- No instance invalidation logic needed

### 3. **Performance**
- No instance allocation overhead
- Direct interface dispatch after wrapper creation
- Minimal indirection: wrapper → guard → interface → singleton

### 4. **Correct Mental Model**
- Plugins are **services**, not objects
- Host **consumes services**, doesn't instantiate objects
- Wrappers are **references**, not instances

---

## Common Misconceptions

### Misconception: "Factory Pattern Creates Instances"

**Reality:** The "factory methods" create **caller wrappers**, not plugin instances.

```rust
// WRONG mental model:
let decoder1 = ImageDecoder::create(runtime)?;  // Creates instance 1?
let decoder2 = ImageDecoder::create(runtime)?;  // Creates instance 2?

// CORRECT mental model:
let wrapper1 = ImageDecoder::create(runtime)?;  // Wrapper 1 → singleton interface
let wrapper2 = ImageDecoder::create(runtime)?;  // Wrapper 2 → SAME singleton interface
```

### Misconception: "Multiple Wrappers = Multiple Instances"

**Reality:** Multiple wrappers can exist, but they all call the same singleton.

```rust
// All three wrappers call the SAME implementation
wrapper1.decode(data)?;  // → singleton
wrapper2.decode(data)?;  // → same singleton
wrapper3.decode(data)?;  // → same singleton
```

### Misconception: "Plugin Has Per-Instance State"

**Reality:** Plugin state is global within the plugin (static), not per-wrapper.

```rust
// Plugin side - static state shared by ALL callers
static COUNTER: AtomicU32 = AtomicU32::new(0);

extern "C" fn process(args: *const (), out: *mut ()) -> AbiError {
    // ALL wrappers (from all hosts) see the SAME counter
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    // ...
}
```

---

## Hot-Reload Implications

### Callback-Based Coordination

The host must destroy all instances when receiving the `Preparing` notification:

```rust
// Hot-reload: PREPARING phase
// Host drops all wrappers
drop(w1); drop(w2); drop(w3);

// Runtime can now safely swap the interface slot via apply_reload_swap
// (under the RuntimeStore RwLock write guard)
```

### Why Wrappers Must Be Dropped

The host **must** drop all wrappers before hot-reload completes because:

1. Old interface will be unloaded (DLL/SO freed)
2. Wrappers holding references to old interface would crash on use
3. Callback coordination prevents dangling references

**Notification flow:**
```
1. Runtime fires PREPARING notification
2. Host drops all wrappers for this bundle
3. Runtime swaps interface
4. Runtime fires RELOADED notification
5. Host creates new wrappers (pointing to new interface)
```

---

## Multiple Implementations (The Exception)

The ONLY way to have "multiple instances" is loading **multiple bundles** that each implement the same contract:

```rust
// Load bundle A with validator implementation
runtime.load_bundle("./plugins/validator_v1")?;

// Load bundle B with ALSO validator implementation
runtime.load_bundle("./plugins/validator_v2")?;

// Find ALL implementations
let mut handles = [GuestContractHandle::null(); 16];
let count = runtime.find_all_by_contract(VALIDATOR_ID, 1, &mut handles)?;
// count == 2 (one from each bundle)

// Create wrappers to different implementations
let wrapper_v1 = ValidatorContract::new(handles[0], runtime)?;  // → bundle A
let wrapper_v2 = ValidatorContract::new(handles[1], runtime)?;  // → bundle B

// These call DIFFERENT implementations
wrapper_v1.validate(data)?;  // → validator_v1
wrapper_v2.validate(data)?;  // → validator_v2 (different!)
```

**This is NOT the same as factory instances** - these are completely separate bundles with separate static state.

---

## Terminology Guide

| Term | What It Means | What It DOESN'T Mean |
|------|---------------|---------------------|
| **Caller Wrapper** | Host-side object providing access to interface | NOT a plugin instance |
| **Plugin Instance** | This term is misleading - avoid it | N/A |
| **Singleton Implementation** | ONE `OnceLock<Box<dyn Trait>>` per contract | NOT per-wrapper |
| **Factory Method** | Creates caller wrapper, checks if plugin exists | NOT creating instances |
| **PluginGuard** | Reference keeper for interface access | NOT instance state |
| **Hot-Reload** | Interface swap via callback coordination | NOT instance migration |

---

## For Documentation Writers

**AVOID these terms:**
- "Create instance"
- "Plugin instance"
- "Factory pattern" (without heavy qualification)
- "Instance lifecycle"

**USE these terms instead:**
- "Create caller wrapper"
- "Plugin implementation" (singleton)
- "Caller wrapper pattern"
- "Wrapper lifecycle"

---

## Summary

1. **Plugins use `OnceLock`** - ONE implementation per contract per bundle
2. **Host creates wrappers** - Multiple wrappers reference the SAME interface
3. **Callback coordination** - Host destroys instances before hot-reload
4. **No factory pattern** - "Factory methods" create wrappers, not instances
5. **Hot-reload via callback** - Wrappers must be dropped before interface swap

**The architecture is: Singleton Implementations + Callback-Based Coordination**

This is simpler, safer, and more performant than a factory/instance model.
