# polyplug_codegen

Code generator for polyplug host-side contract callers. Generates type-safe, hot-reload-aware wrapper code for multiple target languages.

## Overview

`polyplug_codegen` reads a bundle manifest and generates host-side caller code that:

- Provides type-safe access to plugin contract functions
- Implements the factory method pattern for safe hot-reload handling
- Hides internal `PluginGuard` and vtable details from application code
- Supports automatic instance tracking via RAII/guard patterns

## Supported Target Languages

- **Rust** — Native Rust host callers with `PluginVTableGuard`
- **C++** — C++17 classes with `PluginGuard` and `std::optional` factory methods
- **C#** — .NET classes with `PluginGuard` and nullable factory methods
- **Python** — Python 3.10+ classes with `PluginGuard` and `Optional` factory methods
- **Lua** — LuaJIT FFI with table-based OOP and factory functions
- **JavaScript (Deno)** — Deno FFI with private fields and factory methods
- **JavaScript (QuickJS)** — QuickJS FFI with function pointer caching

## Installation

```bash
# Install the CLI tool
cargo install --path crates/polyplugc

# Or use directly from workspace
cargo run --bin polyplugc -- generate --bundle bundle.toml --lang rust --out src/generated
```

## Usage

### Basic Generation

```bash
polyplugc generate \
    --bundle /path/to/bundle.toml \
    --lang rust \
    --out src/generated
```

### Multiple Languages

```bash
# Generate for all supported languages
polyplugc generate --bundle bundle.toml --lang rust --out rust_out
polyplugc generate --bundle bundle.toml --lang cpp --out cpp_out
polyplugc generate --bundle bundle.toml --lang csharp --out csharp_out
polyplugc generate --bundle bundle.toml --lang python --out python_out
polyplugc generate --bundle bundle.toml --lang lua --out lua_out
polyplugc generate --bundle bundle.toml --lang js_deno --out js_deno_out
polyplugc generate --bundle bundle.toml --lang js_quickjs --out js_quickjs_out
```

## Factory Method Pattern

All generated code uses the **factory method pattern** for safe hot-reload handling. This pattern ensures:

- Instances can only be created if a plugin implementing the contract exists
- Instances hold a guard that keeps the vtable alive during hot-reload
- Instances can be validated before use
- Instances can be explicitly reset during hot-reload

### Rust Pattern

```rust
pub struct ImageDecodeContract {
    guard: PluginVTableGuard,
}

impl ImageDecodeContract {
    /// Factory method - creates instance or None if not found
    pub fn create(runtime: &'static Runtime, min_version: u32) -> Option<Self> {
        let handle: PluginHandle = runtime.find_by_contract(IMAGE_DECODE_CONTRACT_ID, min_version).ok()?;
        let guard: PluginVTableGuard = runtime.registry().resolve_guard(handle).ok()?;
        Some(Self { guard })
    }
    
    /// Check if instance is valid (always true for Rust - guard holds Arc)
    pub fn is_valid(&self) -> bool { true }
    
    /// Reset instance (no-op for Rust - guard holds Arc)
    pub fn reset(&mut self) { /* no-op */ }
    
    pub fn decode(&self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        let vtable_ptr: *const PluginVTable = self.guard.vtable();
        // ... ABI call
    }
}

// Usage
if let Some(decoder) = ImageDecodeContract::create(runtime, 1) {
    if decoder.is_valid() {
        let result = decoder.decode(&input)?;
    }
    decoder.reset();  // Optional
}
```

### C++ Pattern

```cpp
class ImageDecodeContract {
public:
    /// Factory method - creates instance or nullopt if not found
    static std::optional<ImageDecodeContract> create(polyplug::Runtime& rt, uint32_t min_version = 0) noexcept {
        uint64_t handle = rt.find(IMAGE_DECODE_CONTRACT_ID, min_version);
        if (handle == UINT64_MAX) {
            return std::nullopt;
        }
        
        polyplug::PluginGuard guard = rt.resolve_plugin(handle);
        if (!guard) {
            return std::nullopt;
        }
        
        return ImageDecodeContract(std::move(guard));
    }
    
    // Move-only (guard is not copyable)
    ImageDecodeContract(ImageDecodeContract&&) noexcept = default;
    ImageDecodeContract& operator=(ImageDecodeContract&&) noexcept = default;
    ImageDecodeContract(const ImageDecodeContract&) = delete;
    ImageDecodeContract& operator=(const ImageDecodeContract&) = delete;
    
    /// Check if instance is valid
    explicit operator bool() const noexcept { return static_cast<bool>(guard_); }
    bool is_valid() const noexcept { return static_cast<bool>(guard_); }
    
    /// Explicitly destroy instance (optional - destructor does this too)
    void reset() noexcept { guard_ = polyplug::PluginGuard{}; }
    
    std::string decode(std::string_view input) {
        const PluginVTable* vt = guard_.vtable();
        // ... ABI call
    }
    
private:
    explicit ImageDecodeContract(polyplug::PluginGuard guard) noexcept
        : guard_(std::move(guard)) {}
    
    polyplug::PluginGuard guard_;
};

// Usage
auto decoder = ImageDecodeContract::create(rt, 1);
if (decoder && decoder->is_valid()) {
    auto result = decoder->decode(input);
}
decoder->reset();  // Optional
```

### C# Pattern

```csharp
/// <summary>
/// Host caller for contract `image.decode`
/// </summary>
public sealed class ImageDecodeContractCaller : IDisposable {
    private PluginGuard _guard;

    private ImageDecodeContractCaller(PluginGuard guard) { _guard = guard; }

    /// <summary>Factory method - creates an instance if a plugin implementing this contract is found.</summary>
    public static ImageDecodeContractCaller? Create(Runtime rt, uint minVersion = 0) {
        var handle = rt.FindByContract(ImageDecodeContractConstants.IMAGE_DECODE_CONTRACT_ID, minVersion);
        if (!handle.IsValid) { return null; }
        var guard = rt.GetGuard(handle);
        if (!guard.IsValid) { return null; }
        return new ImageDecodeContractCaller(guard);
    }

    /// <summary>Check if this caller instance is still valid.</summary>
    public bool IsValid => _guard.IsValid;

    /// <summary>Explicitly release the guard reference.</summary>
    public void Reset() { _guard = default; }

    /// <summary>Dispose pattern for explicit cleanup.</summary>
    public void Dispose() { Reset(); }

    public byte[] Decode(byte[] input) {
        var vtable = _guard.VTable;
        // ... ABI call
    }
}

// Usage
using var decoder = ImageDecodeContractCaller.Create(rt, 1);
if (decoder?.IsValid == true) {
    var result = decoder.Decode(input);
}
decoder?.Reset();  // Optional - Dispose does this
```

### Python Pattern

```python
class ImageDecodeContractCaller:
    """Host caller for contract with hot-reload support."""
    
    def __init__(self, guard: PluginGuard) -> None:
        self._guard: PluginGuard = guard
    
    @classmethod
    def create(cls, rt: Runtime, min_version: int = 0) -> Optional[Self]:
        handle: int = rt.find_by_contract(IMAGE_DECODE_CONTRACT_ID, min_version)
        if handle == NULL_HANDLE:
            return None
        guard: PluginGuard = rt.resolve_plugin(handle)
        if guard.is_null():
            return None
        return cls(guard)
    
    def is_valid(self) -> bool:
        return not self._guard.is_null()
    
    def reset(self) -> None:
        self._guard = PluginGuard.__new__(PluginGuard)
        self._guard._backend = None
        self._guard._runtime = 0
        self._guard._handle = NULL_HANDLE
    
    def __bool__(self) -> bool:
        return self.is_valid()
    
    def decode(self, input: bytes) -> bytes:
        vtable_ptr: int = self._guard.vtable
        # ... ABI call

# Usage
decoder = ImageDecodeContractCaller.create(rt, min_version=1)
if decoder and decoder.is_valid():
    result = decoder.decode(input)
decoder.reset()  # Optional
```

### Lua Pattern

```lua
-- Methods for ImageDecodeContract
local ImageDecodeContract_methods = {
    is_valid = function(self)
        return self._guard ~= nil
    end,

    reset = function(self)
        self._guard = nil
    end,

    decode = function(self, input)
        local vtable = self._guard:vtable()
        if vtable == nil then
            error("invalid guard", 2)
        end
        -- ... ABI call
    end,
}

-- Metatable for ImageDecodeContract
local ImageDecodeContract_mt = {
    __index = ImageDecodeContract_methods
}

-- Factory function for ImageDecodeContract
function M.ImageDecodeContract_create(runtime, min_version)
    if min_version == nil then min_version = 0 end
    local handle = runtime:find_by_contract(IMAGE_DECODE_CONTRACT_ID, min_version)
    if handle == nil then
        return nil
    end
    local guard = runtime:resolve_guard(handle)
    if guard == nil then
        return nil
    end
    local instance = {
        _guard = guard
    }
    setmetatable(instance, ImageDecodeContract_mt)
    return instance
end

-- Usage
local decoder = M.ImageDecodeContract_create(runtime, 1)
if decoder and decoder:is_valid() then
    local result = decoder:decode(input)
end
decoder:reset()  -- Optional
```

### JavaScript (Deno) Pattern

```typescript
export class ImageDecodeContract {
    #guard: any;

    private constructor(guard: any) {
        this.#guard = guard;
    }

    static create(rt: any, minVersion: number = 0): ImageDecodeContract | null {
        const handle = rt.findByContract(ContractIds.IMAGE_DECODE_CONTRACT_ID, minVersion);
        if (handle === null || handle === undefined) {
            return null;
        }
        const guard = rt.getGuard(handle);
        if (!guard) {
            return null;
        }
        return new ImageDecodeContract(guard);
    }

    isValid(): boolean {
        return this.#guard !== null && this.#guard !== undefined;
    }

    reset(): void {
        this.#guard = null;
    }

    decode(input: Uint8Array): Uint8Array {
        const vtable = this.#guard?.vtable?.();
        if (!vtable) throw new Error('caller is not valid');
        // ... ABI call
    }
}

// Usage
const decoder = ImageDecodeContract.create(runtime, 1);
if (decoder && decoder.isValid()) {
    const result = decoder.decode(input);
}
decoder.reset();  // Optional
```

## Hot-Reload Integration

Generated code is designed to work seamlessly with the hot-reload notification system:

### 1. Register Reload Callback

Before creating the runtime, register a callback:

```cpp
// C++ example
Runtime::Builder()
    .on_reload([](const ReloadPhase& phase) {
        if (phase.type == ReloadPhaseType::Preparing) {
            // Destroy all instances for this bundle
            instances[phase.bundle_id].clear();
        }
    })
    .build();
```

### 2. Track Instances Per Bundle

**Instance tracking is flexible** - use whatever approach fits your application:

- **Map/Dictionary**: `Map<bundle_id, List<Instance>>` - most common, easy cleanup
- **Per-contract tracking**: Separate maps per contract type for fine-grained control
- **Weak references**: Let the GC clean up automatically (Python, C#, Lua, JS)
- **RAII guards**: Let the guard's lifetime manage cleanup (Rust, C++)
- **No tracking**: If you don't hold instances across reloads, no cleanup needed

```python
# Python example - Map-based tracking
class PluginManager:
    def __init__(self):
        self._instances = {}  # bundle_id -> list of instances
        
    def create_decoder(self, bundle_id: int):
        decoder = ImageDecodeContractCaller.create(self.rt, 1)
        if decoder:
            self._instances.setdefault(bundle_id, []).append(decoder)
        return decoder
    
    def _on_reload(self, phase: ReloadPhase):
        if phase.is_preparing():
            # Clear all instances for this bundle
            self._instances.pop(phase.bundle_id, None)
```

```rust
// Rust example - No explicit tracking needed!
// PluginVTableGuard uses Arc, so old vtables stay alive until all guards are dropped.
// Just destroy your contract instances in the Preparing callback.
static INSTANCES: LazyLock<Mutex<HashMap<u64, Vec<Box<dyn Any + Send>>>> = ...;

Runtime::builder()
    .on_reload(|phase| {
        if let ReloadPhase::Preparing { bundle_id, .. } = phase {
            INSTANCES.lock().unwrap().remove(&bundle_id);
        }
    })
    .build();
```

### 3. Use Factory Methods for Re-Creation

After reload completes, use factory methods to create new instances:

```csharp
// C# example
Runtime.OnReload(phase => {
    if (phase.IsReloaded()) {
        // Re-create instances with factory method
        _decoder = ImageDecodeContractCaller.Create(_rt, 1);
    }
});
```

## Thread Safety

### Rust
- Generated callers are `!Send` due to `PluginVTableGuard` containing `PhantomData<Cell<()>>`
- Each thread must call `create()` independently
- Prevents cross-thread vtable access during hot-reload

### C++
- Generated callers are move-only, not copyable
- `PluginGuard` is move-only
- Safe to move between threads, but each thread should create its own instances

### C#
- Generated callers are reference types
- `PluginGuard` is a readonly struct
- Thread-safe for reading, but instances should not be shared across threads during hot-reload

### Python/Lua/JavaScript
- Single-threaded by design (GIL for Python, single-threaded for Lua/JS)
- No special thread safety considerations

## Bundle Manifest Format

The generator reads a TOML bundle manifest:

```toml
[bundle]
name = "image_processing"
version = "1.0.0"

[[contracts]]
name = "image.decode"
version = 1
functions = ["decode", "encode"]

[[contracts]]
name = "image.transform"
version = 1
functions = ["rotate", "scale", "crop"]
```

## Generated File Structure

### Rust
```
src/generated/
├── mod.rs
├── contracts/
│   ├── mod.rs
│   ├── image_decode.rs
│   └── image_transform.rs
└── constants.rs
```

### C++
```
generated/
├── host_callers.hpp
├── constants.hpp
└── contracts/
    ├── image_decode.hpp
    └── image_transform.hpp
```

### C#
```
Generated/
├── Constants.cs
├── ImageDecodeContractCaller.cs
└── ImageTransformContractCaller.cs
```

### Python
```
generated/
├── __init__.py
├── constants.py
├── image_decode_caller.py
└── image_transform_caller.py
```

## Error Handling

Generated code handles errors appropriately for each target language:

- **Rust** — Returns `Result<T, ContractError>` or `Option<T>`
- **C++** — Throws `std::runtime_error` or returns `std::optional<T>`
- **C#** — Throws exceptions or returns nullable types
- **Python** — Raises exceptions or returns `None`
- **Lua** — Returns `nil` + error message or uses `pcall`
- **JavaScript** — Throws errors or returns `null`

## Performance Considerations

### Vtable Access
- All generated code caches the vtable pointer in the guard
- No FFI overhead on vtable access after initial resolution
- Hot-reload safe: guard re-resolves vtable on each call (Python) or holds Arc (Rust/C++)

### Function Dispatch
- Direct vtable dispatch after guard resolution
- No per-call overhead for contract ID lookup
- Minimal indirection: guard → vtable → function pointer

## Testing

The generator includes comprehensive tests:

```bash
# Run unit tests
cargo test --package polyplug_codegen

# Run generator correctness tests
cargo test --package polyplug_codegen --test generator_correctness

# Run integration tests
cargo test --test integration
```

## License

Apache 2.0 — See `../../LICENSE` for details.
