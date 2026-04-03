# polyplug

## What This Is

A high-performance, zero/minimal-overhead cross-language plugin runtime for Rust. Enables host applications to load plugins written in Rust, Python, C#, Lua, JavaScript, or C++ through a unified FFI-based interface with hot-reload support.

## Core Value

The core runtime is loader-agnostic — the `polyplug` crate knows about the `BundleLoader` trait and `PluginRegistry`, but NOT about `libloading`, `dlopen`, or any specific loader implementation.

## Architecture

### Crate Structure

```
crates/
├── polyplug_abi/        # FFI types - ALL public structs are #[repr(C)]
│   └── src/
│       ├── contract/    # GuestContractInterface, HostContractInterface
│       ├── dispatch/    # DispatchType, DispatchMechanisms, NativeDispatch, VmDispatch
│       ├── context/     # RuntimeContext, PluginContext
│       ├── types/       # StringView, Buffer, Version, AbiError, etc.
│       ├── handle/      # ContractHandle, BundleId, GuestContractId, HostContractId
│       └── abi/         # RuntimeAbi (the ABI function table)
│
├── polyplug_utils/      # Zero-dependency ID types
│   └── src/
│       ├── bundle_id.rs        # BundleId (FNV-1a hash of bundle name)
│       ├── guest_contract_id.rs # GuestContractId (hash of "contract@major")
│       └── host_contract_id.rs  # HostContractId (hash of "host_contract:name@major")
│
├── polyplug/            # Core runtime - NO loader-specific code
│   └── src/
│       ├── runtime.rs           # Runtime struct (opaque, not repr(C))
│       ├── runtime_builder.rs   # Builder pattern for runtime creation
│       ├── registry/            # PluginRegistry - stores GuestContractInterface
│       ├── loader/              # BundleLoader trait, manifest types
│       ├── reload.rs            # Hot-reload callback mechanism
│       ├── ffi.rs               # C ABI exports for other languages
│       └── error.rs             # RuntimeError, LoaderError, etc.
│
├── polyplug_native/     # Native loader (libloading/dlopen)
├── polyplug_python/     # Python loader (pyo3)
├── polyplug_lua/        # Lua loader (mlua/LuaJIT)
├── polyplug_js/         # JavaScript loader (rquickjs/QuickJS)
├── polyplug_dotnet/     # .NET loader (netcorehost)
│
├── polyplugc/           # CLI code generator
└── polyplug_codegen/    # Code generation library (IR, language generators)

sdks/
├── rust/
│   ├── guest/           # Re-exports polyplug_abi + helpers
│   └── host/            # Runtime wrapper, manifest parsing
├── python/
│   ├── abi/             # ctypes mirrors of polyplug_abi types
│   ├── guest/           # Plugin author library
│   └── host/            # Runtime class, PluginGuard
├── csharp/
│   ├── Abi/             # C# mirrors of polyplug_abi types
│   ├── Guest/           # Plugin author library
│   └── Host/            # Runtime class, PluginGuard
├── lua/
│   ├── abi/             # FFI cdef of polyplug_abi types
│   ├── guest/           # Plugin author library
│   └── host/            # Runtime class
└── js/
    ├── abi/             # TypeScript interfaces
    ├── guest/           # Plugin author library
    └── host/            # Runtime class
```

### Key Concepts

#### Bundle
A deployment unit containing one or more plugin implementations. A bundle is a directory with:
- `manifest.toml` - metadata (name, version, runtime, dependencies, provides)
- Plugin file (`.so`, `.dll`, `.dylib`, `.py`, `.lua`, `.js`)

One bundle can provide multiple contracts (multi-plugin bundles).

#### Contract
A named interface with versioned methods. Two types:
- **Guest Contract** - Implemented by plugins, consumed by host
- **Host Contract** - Implemented by host, consumed by plugins

#### Interface
The `GuestContractInterface` and `HostContractInterface` structs define how to call a contract:
- Contract identification (ID, version)
- Dispatch mechanism (native function pointers or VM dispatch)
- Instance lifecycle (create_instance, destroy_instance)

#### Instance
A concrete instance of a contract with state. Instances are:
- Created by `interface.create_instance(rt_ctx)`
- Owned by the host (RAII pattern in generated code)
- Passed as first argument to all dispatch calls
- Destroyed by `interface.destroy_instance(rt_ctx, instance)`

Host contracts can be:
- **Singleton** - same instance returned every time
- **Multi-instance** - new instance created each time

#### Registry
Stores `GuestContractInterface` pointers, indexed by `GuestContractId`. Supports:
- `find_contract(contract_id, min_version)` - find any implementation
- `resolve_contract(handle)` - get interface pointer
- Multi-impl support (multiple bundles can implement same contract)

Does NOT store:
- Instance state (host owns instances)
- Bundle data (stored separately)

## FFI Types (polyplug_abi)

All public structs are `#[repr(C)]` for ABI stability.

### GuestContractInterface

```rust
#[repr(C)]
pub struct GuestContractInterface {
    pub contract_id: GuestContractId,
    pub contract_version: Version,
    pub dispatch_type: DispatchType,
    pub dispatch: DispatchMechanisms,
    pub create_instance: unsafe extern "C" fn(rt_ctx: *mut c_void) -> GuestContractInstance,
    pub destroy_instance: unsafe extern "C" fn(rt_ctx: *mut c_void, instance: GuestContractInstance),
}
```

### HostContractInterface

```rust
#[repr(C)]
pub struct HostContractInterface {
    pub contract_id: HostContractId,
    pub contract_version: Version,
    pub singleton: bool,
    pub dispatch_type: DispatchType,
    pub dispatch: DispatchMechanisms,
    pub create_instance: unsafe extern "C" fn(rt_ctx: *mut c_void) -> HostContractInstance,
    pub destroy_instance: unsafe extern "C" fn(rt_ctx: *mut c_void, instance: HostContractInstance),
}
```

### Instance Handles (Opaque)

```rust
/// Opaque handle to a guest contract instance.
/// Created by GuestContractInterface::create_instance, destroyed by destroy_instance.
/// Passed as first argument to all dispatch calls.
#[repr(C)]
pub struct GuestContractInstance {
    pub data: *mut c_void,
}

/// Opaque handle to a host contract instance.
/// Created by HostContractInterface::create_instance (or returned for singletons).
/// Destroyed by destroy_instance (for multi-instance only).
#[repr(C)]
pub struct HostContractInstance {
    pub data: *mut c_void,
}
```

### RuntimeAbi

The ABI table passed to plugins during initialization:

```rust
#[repr(C)]
pub struct RuntimeAbi {
    // Registration
    pub register_plugin: unsafe extern "C" fn(rt_ctx, descriptor, interface) -> AbiError,
    
    // Memory
    pub alloc: unsafe extern "C" fn(rt_ctx, size, align) -> *mut u8,
    pub free: unsafe extern "C" fn(rt_ctx, ptr, size, align),
    
    // Contract resolution (for plugin-plugin dependencies)
    pub find_contract: unsafe extern "C" fn(rt_ctx, contract_id, min_version) -> ContractHandle,
    pub resolve_contract: unsafe extern "C" fn(rt_ctx, handle) -> *const GuestContractInterface,
    
    // Host contract access
    pub get_host_contract: unsafe extern "C" fn(rt_ctx, contract_id, min_version) -> HostContractInstance,
    
    // Cross-dispatch method call
    pub call_method: unsafe extern "C" fn(
        rt_ctx,
        interface: *const GuestContractInterface,
        instance: GuestContractInstance,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
}
```

### Dispatch Mechanisms

```rust
#[repr(u32)]
pub enum DispatchType {
    Native = 0,         // Direct function pointer calls
    VirtualMachine = 1, // Call through VM dispatch function
}

#[repr(C)]
pub struct NativeDispatch {
    pub functions: *const *const (),  // Array of function pointers
}

#[repr(C)]
pub struct VmDispatch {
    pub call: unsafe extern "C" fn(
        loader_data: *mut c_void,       // VM state
        instance: GuestContractInstance, // Instance (opaque handle)
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    pub loader_data: *mut c_void,
}
```

`GuestContractInstance` is passed as first argument for both native and VM dispatch.

### Contexts

```rust
// Passed during polyplug_init only
#[repr(C)]
pub struct PluginContext {
    pub bundle_id: BundleId,
    pub bundle_path: StringView,
    pub bundle_version: Version,
}

// Opaque runtime pointer - passed to all ABI functions
// Type: *mut c_void, points to Runtime internally
```

### Handles

```rust
#[repr(C)]
pub struct ContractHandle {
    pub index: u32,  // No generation - instances destroyed before hot-reload
}

// ID types (from polyplug_utils)
pub struct BundleId(pub u64);           // FNV-1a of bundle name
pub struct GuestContractId(pub u64);    // FNV-1a of "contract@major"
pub struct HostContractId(pub u64);     // FNV-1a of "host_contract:name@major"
```

## Dispatch Flow

### Host Calling Guest Contract

```
Host Code
    │
    ▼
Generated Instance Wrapper (codegen)
    │
    │  runtime.find_contract(contract_id, 0) -> ContractHandle
    │  runtime.resolve_contract(handle) -> &GuestContractInterface
    │  interface.create_instance(rt_ctx) -> GuestContractInstance
    │
    ▼
Method Call
    │
    │  match interface.dispatch_type
    │    Native: dispatch.native.functions[fn_id](instance, args, out)
    │    VM: dispatch.vm.call(loader_data, instance, fn_id, args, out)
    │
    ▼
Plugin Implementation
    │
    ▼
Result/AbiError
```

### Plugin Calling Plugin (Guest to Guest)

Same flow, but plugin uses RuntimeAbi:
1. `abi.find_contract(rt_ctx, contract_id, 0)` → ContractHandle
2. `abi.resolve_contract(rt_ctx, handle)` → interface pointer
3. `(interface.create_instance)(rt_ctx)` → GuestContractInstance
4. Dispatch through interface (or use `abi.call_method` for cross-dispatch)
5. `(interface.destroy_instance)(rt_ctx, instance)`

### Plugin Calling Host Contract

1. `abi.get_host_contract(rt_ctx, contract_id, 0)` → HostContractInstance
2. Dispatch through stored HostContractInterface
3. For singleton: same instance each time
4. For multi-instance: new instance each time, caller owns it

## Hot-Reload

### Mechanism

1. Host triggers `runtime.reload_bundle(bundle_id)`
2. Runtime fires `ReloadPhase::Preparing` callback
3. Host destroys ALL instances from that bundle
4. Callback returns → Runtime assumes no remaining instances
5. Runtime atomically swaps `GuestContractInterface` pointers
6. Runtime fires `ReloadPhase::Reloaded` callback
7. Host creates new instances from new interfaces
8. Any remaining instances (leaked) → warning callback, UB if used

### Instance Safety

- No generation counters in handles
- Safety enforced by callback contract: host MUST destroy instances
- Leaked instances → undefined behavior after hot-reload
- Warning callback fires if runtime detects remaining instances

## Code Generation

### polyplugc CLI

```bash
# Generate host SDK (from api.toml)
polyplugc generate --api api.toml --lang rust --out src/generated

# Generate guest SDK (from bundle.toml)
polyplugc generate --bundle bundle.toml --lang rust --out src/generated
```

### Generated Artifacts

**Host SDK** (for app developers):
- `types.rs` - Enums, structs, contract ID constants
- `guest_callers.rs` - RAII instance wrappers with type-safe methods
- `host_contract_impl.rs` - Trait to implement host contracts
- `host_contract_vtables.rs` - Factory functions for HostContractInterface

**Guest SDK** (for plugin authors):
- `types.rs` - Shared types
- `contracts.rs` - Traits to implement for each guest contract
- `vtables.rs` - ABI wrappers and GuestContractInterface statics
- `init.rs` - `polyplug_init` entry point
- `host_callers.rs` - Wrappers to call host contracts

### Instance Wrapper Pattern (Generated)

```rust
// Generated by codegen for GUEST contracts
pub struct DecoderInstance {  // {ContractName}Instance
    interface: &'static GuestContractInterface,
    instance: GuestContractInstance,
    rt_ctx: *mut c_void,  // Stored for drop
}

impl DecoderInstance {
    pub fn create(runtime: &Runtime, contract_id: GuestContractId) -> Result<Self, Error> {
        let handle = runtime.find_contract(contract_id, 0)?;
        let interface = runtime.resolve_contract(handle)?;
        let instance = unsafe { (interface.create_instance)(runtime.ctx()) };
        if instance.data.is_null() {
            return Err(Error::CreateFailed);
        }
        Ok(Self { interface, instance, rt_ctx: runtime.ctx() })
    }
    
    pub fn decode(&self, input: &str) -> Result<String, Error> {
        // Pack args, dispatch through interface, unpack result
    }
}

impl Drop for DecoderInstance {
    fn drop(&mut self) {
        unsafe {
            (self.interface.destroy_instance)(self.rt_ctx, self.instance);
        }
    }
}

// Generated by codegen for HOST contracts
pub struct LoggerInstance {  // {ContractName}Instance
    interface: &'static HostContractInterface,
    instance: HostContractInstance,
    rt_ctx: *mut c_void,
}
```

## Bundle Loading

### Manifest (manifest.toml)

```toml
id = 123456789
name = "decoder_bundle"
version = "1.0.0"
runtime = "native"
file = { linux.x86_64 = "libdecoder.so", macos.aarch64 = "libdecoder.dylib" }

provides = ["pipeline.Decoder@1.0"]

[[dependency]]
kind = "contract"
contract = "image.loader@1.0"
contract_id = 0xABCDEF...
min_version = "1.0"

[function_count]
"pipeline.Decoder@1" = 3
```

### Load Flow

1. **Discovery**: Scanner finds `manifest.toml` in plugin directories
2. **Graph Build**: CapabilityGraph from manifests, detect cycles
3. **Topological Sort**: Providers before dependents
4. **Dispatch**: Match `manifest.runtime` to `BundleLoader::runtime_name()`
5. **Loader Load**:
   - Native: dlopen, check ABI version, resolve polyplug_init
   - VM: create VM instance, load script
6. **Init**: Call `polyplug_init(rt_ctx, abi, plugin_ctx)`
7. **Registration**: Plugin calls `abi.register_plugin(descriptor, interface)`
8. **Storage**: Runtime stores interface in registry, loader stores library handle

### BundleLoader Trait

```rust
pub trait BundleLoader: Send + Sync {
    fn runtime_name(&self) -> &'static str;
    fn runtime_names(&self) -> Vec<String> { vec![self.runtime_name().to_owned()] }
    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;
    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError>;
}
```

## Memory Management

### Host Allocator

All cross-boundary memory uses the host allocator:
- `polyplug_host_alloc(size, align)` - allocate
- `polyplug_host_free(ptr, size, align)` - free

### StringView vs Buffer

- `StringView { ptr, len }` - Borrowed string, receiver must NOT free
- `Buffer { ptr, len, cap }` - Owning buffer, must free with host_free

### Interface Lifetime

- `GuestContractInterface` must be `'static` or intentionally leaked
- Never stack-allocated
- Lives as long as the bundle is loaded

## Error Handling

### Error Types

```rust
pub enum RuntimeError {
    Loader(LoaderError),
    Registry(RegistryError),
    Graph(GraphError),
    HotReloadDisabled,
    QuiescenceTimeout { ... },
}

pub enum LoaderError {
    InitFailed { bundle: String, error: String },
    NoLoaderForRuntime { bundle: String, runtime_name: String },
    DuplicateLoader { runtime_name: String },
    // ... others
}

pub enum RegistryError {
    ContractNotFound { contract_id: u64, min_version: u32 },
    ContractIdCollision { id: u64, name_a: String, name_b: String },
}
```

### FFI Error Reporting

- FFI functions return `AbiError { code, message }`
- Runtime stores last error in `Mutex<String>`
- `polyplug_runtime_last_error()` retrieves error message

## Constraints

- **Architecture:** Core crate must have zero loader-specific code or dependencies
- **Safety:** Host must destroy all instances before hot-reload completes
- **Compatibility:** Breaking changes acceptable — not published yet
- **FFI:** All public ABI structs are `#[repr(C)]`
- **Pointers:** Raw pointers only at FFI boundary, not in internal Rust code
- **Manifest:** Core runtime should NOT parse TOML - manifest parsing is external concern
- **Type Source:** No `*C` suffix types - all FFI types defined once in `polyplug_abi`

## Types Moving to polyplug_abi

These types currently exist in `crates/polyplug/src/` but should move to `polyplug_abi`:

| Type | Current Location | New Location |
|------|------------------|--------------|
| `RuntimeConfig` | `runtime_config.rs` | `polyplug_abi/src/config/` |
| `ReloadPhase` | `reload.rs` (Rust enum) | `polyplug_abi/src/types/` |
| `RuntimeCreateOptions` | `ffi.rs` | `polyplug_abi/src/config/` |

After move:
- Rust SDK imports from `polyplug_abi` (same as Python/C#/Lua/JS)
- No duplicate `RuntimeConfigC` in each SDK
- Single source of truth

## Manifest Parsing Location

**Decision:** Manifest parsing stays in core runtime for now (not a blocker for architecture refactor).

**Future consideration:** Move to `polyplug_manifest` crate or host SDKs.

**Why not urgent:**
- Runtime needs `ManifestData` internally for loading
- TOML parsing doesn't leak through FFI
- Host SDKs can wrap `load_bundle(path)` without parsing

## Rust SDK Restructure

Currently Rust uses `polyplug` crate directly. After refactor:

```
sdks/rust/
├── guest/              # Plugin author library
│   ├── src/lib.rs      # Re-exports polyplug_abi + helpers
│   └── Cargo.toml      # Depends on polyplug_abi, polyplug_utils
│
└── host/               # Host application library  
    ├── src/lib.rs      # Runtime wrapper, uses polyplug_abi types
    ├── src/runtime.rs  # High-level Runtime API
    └── Cargo.toml      # Depends on polyplug, polyplug_abi
```

Rust becomes a "first-class FFI consumer" - uses same types as other languages.

## Registry Simplification

**Current (complex):**
```rust
struct RegistrySlot {
    generation: AtomicU32,        // For stale handle detection
    entry: Option<RegistryEntry>, // Descriptor + metadata
    vtable: Option<ArcSwap<VTableSlot>>, // VTableSlot wraps *const PluginInterface
}
```

**Target (simple):**
```rust
struct RegistrySlot {
    interface: Arc<GuestContractInterface>, // Direct storage, no wrapper
    descriptor: ContractDescriptor,          // Metadata
    bundle_id: BundleId,                     // Which bundle
}

// Index by contract_id for find_contract()
// No generation counter - hot-reload destroys instances first
// No VTableSlot wrapper - store interface directly
```

## Current Milestone: v1.1 Architecture Refactor

### Goal

Refactor the core architecture to:
1. Remove "vtable" terminology - use `GuestContractInterface`
2. Remove `VTableSlot` wrapper - registry stores interfaces directly
3. Replace `PluginGuard` with instance-based RAII pattern
4. Make all public ABI structs `#[repr(C)]` in `polyplug_abi`
5. Remove `*C` suffix types - single source of truth in `polyplug_abi`
6. Move manifest parsing out of core runtime
7. Implement instance model with factory/RAII pattern
8. Support singleton and multi-instance host contracts

### Target Features

- Instance-based plugin model (host creates/owns instances)
- Hot-reload via callback-based instance destruction
- Cross-dispatch method calls for plugin-plugin communication
- Clear Host/Guest naming throughout
- FFI-first design - Rust SDK uses same types as other languages

---
*Last updated: 2026-04-03*