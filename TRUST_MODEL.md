# Trust Model — polyplug

This document defines the security boundaries, dependency enforcement mechanisms, and trust assumptions of the polyplug runtime platform.

## 1. Overview

The polyplug trust model governs how independent plugin bundles interact within a shared process space. Unlike OS-level process isolation, polyplug operates within a single address space, prioritizing performance and architectural integrity over hostile-actor sandboxing.

### Design Philosophy
The model is built on three pillars: **Bundle Identity**, **Declared Dependencies**, and a strictly defined **Enforcement Window**. Our design philosophy favors a "catch-at-load-time" approach. By verifying the dependency graph during the initialization phase, we eliminate the need for expensive per-call authorization checks in the runtime hot path. If a dependency exists and was declared, the call proceeds at the speed of a raw function pointer dereference.

### Scope of Trust
- **Host Application**: Fully trusted (Bundle ID 0). It has unrestricted access to all registered contracts.
- **Plugin Bundles**: Semi-trusted. They are restricted during their initialization phase to only the contracts they explicitly declared in their manifest.
- **Runtime**: The root of trust. It manages the registry, handles dlopen/dlsym operations, and enforces the dependency contracts.

## 2. Bundle Identity

Every plugin bundle is uniquely identified by a `bundle_id`. This 64-bit identifier is the FNV1a-64 hash of the bundle name string provided in the `bundle.toml` (or `manifest.toml`).

### ID Computation
The hash is computed using the FNV-1a algorithm, implemented in `crates/polyplug/src/abi/mod.rs`.
```rust
pub fn bundle_id(name: &str) -> u64 {
    fnv1a_64(name.as_bytes())
}
```
The use of a 64-bit hash space ensures that for typical deployment sizes (hundreds or thousands of plugins), the probability of a collision is mathematically negligible.

### Deployment Constraints
- **Unique Names**: Bundle names must be unique within a single application deployment. A name collision results in a `bundle_id` collision, which the runtime will reject during the second bundle's registration.
- **Baking the ID**: The `polyplugc` compiler bakes the computed ID into the generated guest code as a constant. This allows the guest to identify itself to the host during the `polyplug_init` call.

### The Null Bundle (ID 0)
A `bundle_id` of `0` is reserved. It represents the "System Context" or "Host Context".
- Internal runtime operations use ID 0 to bypass enforcement.
- The host application itself operates under ID 0.
- Any lookup performed when no `BundleInitGuard` is active defaults to ID 0, effectively disabling enforcement for the host.

## 3. Declared Dependencies

Dependencies are not discovered implicitly; they must be explicitly declared in the bundle's manifest file. This declaration forms a binding contract between the bundle and the polyplug registry.

### Manifest Example
A typical `bundle.toml` declaration looks like this:
```toml
[bundle]
name = "audio-engine"
runtime = "native"

[[dependency]]
kind = "contract"
contract = "audio.Decoder"
min_version = 1
```

### The Registration Flow
When a bundle is loaded, the `BundleLoader` performs the following steps:
1. Parses the manifest dependencies.
2. Converts contract names to `contract_id` hashes.
3. Calls `registry.declare_deps(bundle_id, contract_ids)`.
4. If `declare_deps` fails (e.g., due to an internal registry error), the bundle load is aborted.

### Enforcement Mechanism
The `Registry` maintains a `HashMap<u64, HashSet<u64>>` mapping `bundle_id` to its allowed `contract_id` set. During the initialization phase, every `find_by_contract` call checks this set. If a plugin attempts to resolve a contract it did not declare, the runtime returns a null handle, preventing the plugin from ever obtaining the vtable.

## 4. Enforcement Window

To maintain maximum performance, polyplug does not enforce dependencies on every single call. Instead, it uses a high-integrity "Enforcement Window" during the plugin's lifecycle.

### Phase 1 vs. Phase 2
The runtime distinguishes between the **Initialization Phase (Phase 1)** and the **Execution Phase (Phase 2)**.

```
### Diagram: Enforcement Flow
```
------------------------------|----------------------------
INIT_BUNDLE_ID != 0           |  INIT_BUNDLE_ID == 0
Strict Enforcement            |  Zero Overhead
Checks manifest declarations  |  Trusts Phase 1 results
Returns null if undeclared    |  Direct pointer dispatch
                              |
[Plugin] -> find_contract()   | [Plugin] -> call_vtable()
    |                         |      |
    v                         |      v
(Check Registry Deps)         | (Direct Dereference)
    |                         |
    +-- Allowed? -> Handle    |
    +-- Denied?  -> Null      |
```
Strict Enforcement            |  Zero Overhead
Checks manifest declarations  |  Trusts Phase 1 results
Returns null if undeclared    |  Direct pointer dispatch
```

### The BundleInitGuard
The transition is managed by a thread-local RAII guard called `BundleInitGuard`.
- **Entrance**: When `load_bundle` is about to call `polyplug_init`, it sets the thread-local `INIT_BUNDLE_ID` to the current bundle's ID.
- **Enforcement**: The host callbacks (`host_find_by_contract`, etc.) check this thread-local. If it's non-zero, they verify the contract against the declared dependencies.
- **Exit**: When `polyplug_init` returns, the guard is dropped, resetting `INIT_BUNDLE_ID` to 0.

### Why Hot-paths are Unchecked
Once a plugin has successfully obtained a `PluginHandle` during Phase 1, it has effectively "cleared customs." Since the registry and the plugin's dependency set are immutable for the life of the process, there is no architectural reason to re-verify the same contract on every hot-path call.

## 5. Multi-impl Resolution

Polyplug allows multiple bundles to implement the same contract, enabling a rich ecosystem of providers. The runtime provides three distinct query APIs to resolve these implementations.

### Query APIs
1. **`find_by_contract(contract_id, min_version)`**:
   The standard lookup. It returns the `PluginHandle` for the **first registered** provider that satisfies the version requirement. This is deterministic based on the load order.
2. **`find_by_bundle(bundle_id, contract_id, min_version)`**:
   A scoped lookup. This allows a caller to request an implementation from a specific provider bundle, bypassing the default resolution order.
3. **`find_all_by_contract(contract_id, min_version)`**:
   The enumeration API. It returns all providers for a contract. In the C ABI, the caller provides a pre-allocated buffer of `PluginHandle` elements which the host populates.

### Implementation Integrity
- **DuplicateProvider Rule**: The same `bundle_id` cannot register the same `contract_id` twice. This prevents internal ambiguity within a single bundle.
- **Cross-Bundle Multi-impl**: Different bundles *can* implement the same contract. The registry tracks these in a `Vec<u32>` of slot indices per contract ID.
- **Stale Handle Protection**: Every `PluginHandle` contains a generation counter. The `resolve_guard` (or `resolve_plugin` in C) compares the handle's generation against the registry slot. If they mismatch (e.g., after a bundle is unloaded and a new one takes its slot), the resolution returns `ABI_ERROR_STALE_HANDLE`.

### Multi-impl Scenario
Consider an application that supports multiple audio decoders. Both `flac-bundle` and `mp3-bundle` might register the same `audio.Decoder` contract.

1. **`find_by_contract`**: The first one to register (e.g., `flac-bundle`) will be returned as the system default.
2. **`find_by_bundle`**: The host can explicitly ask for the `mp3-bundle` implementation.
3. **`find_all_by_contract`**: The UI can enumerate all available decoders to show a selection list.

### Reference: Frozen Struct Layouts
The following table summarizes the sizes and alignments of the core ABI types on 64-bit systems.

| Type | Size (bytes) | Alignment (bytes) | Key Fields |
|------|--------------|-------------------|------------|
| `HostVTable` | 56 | 8 | 7 function pointers |
| `PluginVTable` | 24 | 8 | `contract_id`, `functions` ptr |
| `PluginHandle` | 8 | 4 | `index`, `generation` |
| `StringView` | 16 | 8 | `ptr`, `len` |
| `Buffer` | 24 | 8 | `ptr`, `len`, `cap` |
| `AbiError` | 24 | 8 | `code`, `message` (StringView) |
| `PluginDescriptor` | 48 | 8 | `name`, `contract_name`, `version` |

### Rust-only Safety: `PluginVTableGuard`
While the C ABI deals in raw handles and pointers, the Rust guest and host libraries use `PluginVTableGuard` for safe access.
```rust
// The guard ensures the vtable stays valid while we are using it.
// It is !Send to prevent cross-thread use of a resolved vtable.
pub struct PluginVTableGuard {
    pub(crate) slot: Arc<VTableSlot>,
    _not_send: PhantomData<Cell<()>>,
}

impl PluginVTableGuard {
    pub fn vtable(&self) -> *const PluginVTable {
        self.slot.0
    }
}
```
If a bundle is unloaded, any new `resolve_guard` calls for that handle will fail with `RegistryError::StaleHandle`.
## 6. Threat Model

The polyplug trust model is a **Software Architecture Enforcement Tool**, not a security sandbox. It is designed to prevent architectural erosion in large-scale systems.

### Capabilities Matrix

| Protection Type | Status | Description |
|-----------------|--------|-------------|
| Undeclared Dependencies | **YES** | Caught during initialization lookup. |
| Version Mismatches | **YES** | Rejected by `find_by_contract` if version < `min_version`. |
| Use-after-Unload | **YES** | Caught by generational index checks in `PluginHandle`. |
| Malicious Memory Access | **NO** | Plugins share the same address space and can read/write any memory. |
| Malicious Symbol Access | **NO** | A plugin can use `dlopen(NULL, ...)` to find host symbols directly. |
| Denial of Service | **NO** | A plugin can loop infinitely or exhaust host memory. |

### The "Trusted Same-Process" Assumption
Polyplug assumes that all loaded bundles are authorized to run by the host application. If you require protection against hostile code, you must wrap the polyplug host in an OS-level sandbox (e.g., Firecracker, WebAssembly, or Linux Namespaces).

## 7. ABI Freeze Notice

The core polyplug ABI is frozen as of Epic 9.7. This freeze ensures that bundles compiled today remain binary-compatible with future versions of the runtime.

### Frozen Surface Areas
The following structures have fixed layouts and sizes. Any modification to these (e.g., adding a field or changing field order) is a breaking change.
- **`HostVTable` (56 bytes)**: Contains 7 function pointers (`alloc`, `free`, `find_by_contract`, `find_by_bundle`, `find_all_by_contract`, `resolve_plugin`, `get_extension`).
- **`PluginVTable` (24 bytes)**: Fixed header before the function pointer array.
- **`PluginHandle` (8 bytes)**: 4-byte index, 4-byte generation.
- **`StringView` (16 bytes)**: 8-byte pointer, 8-byte length.
- **`Buffer` (24 bytes)**: 8-byte pointer, 8-byte length, 8-byte capacity.

### Extensibility via `get_extension`
To support future features without breaking the ABI, the `HostVTable` includes `get_extension(extension_id)`. This allows the host to expose new capability-specific VTables to plugins that know how to ask for them.

## 8. Future Work

The trust model continues to evolve as polyplug expands its reach into more dynamic environments.

### Hot-Reload (Epic 10)
Future versions will support reloading bundles at runtime. The generation counter in `PluginHandle` is the foundation for this feature. When a bundle is reloaded, its previous registry slots are marked stale, and any existing handles held by other plugins will be rejected by the `resolve_guard`, forcing them to re-resolve the contract.

### Scripting Bindings (Epics 10/11)
Python and Lua bindings are planned. These will respect the same trust model rules. Scripted plugins will have their own "Virtual Bundle ID" and will declare dependencies via their respective script manifests. The runtime will enforce these through the same `INIT_BUNDLE_ID` mechanism used by native code.

### Priority Resolution
We plan to introduce a weighting system for multi-impl providers. This will allow the host or a "Coordinator Bundle" to assign priorities to implementations, ensuring that `find_by_contract` returns the "best" provider rather than just the first one registered.
