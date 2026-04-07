# Phase 11: Guest Calling Convention & Missing Introspection - Research

**Researched:** 2026-04-07
**Domain:** FFI Interface Design, Cross-Language Plugin Runtime, ABI Types
**Confidence:** HIGH

## Summary

This phase implements a symmetric interface architecture where `RuntimeInterface` (for host calls) and `HostInterface` (for guest calls) provide clean, self-contained function tables. The key insight is removing the indirection of `RuntimeContext`/`HostContext` wrappers and instead embedding the opaque runtime pointer directly in each interface struct. This enables a self-passing pattern where SDKs hide the `self` parameter from users while the ABI remains C-compatible.

The phase also introduces `Array<T>` as a proper FFI-safe container with caller-frees semantics, enables guest-to-guest calls by adding `contract_id` to `GuestContractInstance`, and adds introspection ABIs (`list_bundles`, `get_dependencies`) for plugins to query their runtime environment.

**Primary recommendation:** Rename `RuntimeAbi` → `HostInterface`, create `RuntimeInterface`, delete `RuntimeContext`/`HostContext`, and implement the self-passing pattern consistently across all interfaces.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Interface Naming (Symmetric Design)
- **D-01:** Rename `RuntimeAbi` → `HostInterface`
  - Consistent with `GuestContractInterface`/`HostContractInterface` pattern
  - Clear: runtime provides, guest calls
- **D-02:** Create `RuntimeInterface` struct
  - Symmetric with `HostInterface`
  - Returned from `polyplug_runtime_create()`
  - Contains function pointers for host to call runtime
  - Replaces scattered `polyplug_runtime_*` FFI functions

#### Symmetric Interface Architecture
```
| Interface              | Provided by | Called by |
|------------------------|-------------|-----------|
| RuntimeInterface       | Runtime     | Host      |
| HostInterface          | Runtime     | Guest     |
| GuestContractInterface | Guest       | Host      |
| HostContractInterface  | Host        | Guest     |
```

#### Delete Wrapper Types
- **D-03:** Delete `RuntimeContext` and `HostContext`
  - These were indirection layers that add confusion
  - Interfaces now directly contain `runtime: *mut c_void` opaque pointer
  - No more `rt_ctx` parameter — functions take `self_ptr: *const Interface` instead
  - SDKs hide the `self_ptr` passing from users

#### Types That Stay Unchanged
- **VmLoaderData** — KEEP. Wraps VM-specific state for loaders (Python, Lua, JS). Used in `VmDispatch`. Independent of RuntimeContext.
- **PluginContext** — KEEP. Still passed to `polyplug_init`, contains `bundle_id` used by `get_dependencies`.
- **VmDispatch** — KEEP. Used for VM-based dispatch, still takes `VmLoaderData`.

#### polyplug_init Signature Change
```c
// Old (with RuntimeContext):
void polyplug_init(RuntimeContext rt_ctx, const HostInterface* host, const PluginContext* ctx);

// New (rt_ctx removed - redundant since HostInterface contains runtime pointer):
void polyplug_init(const HostInterface* host, const PluginContext* ctx);
```

#### Instance Naming
- **D-04:** Keep `GuestContractInstance`/`HostContractInstance` naming — Contract prefix clarifies what kind of instance. No change needed.

#### Array/Vector ABI Types
- **D-05:** Generic `Array<T>` for FFI with caller-frees ownership model
  - `Array<T> = { ptr: *mut T, len: usize, align: usize }`
  - Allocated via `host->alloc(self, len * sizeof(T), align)`
  - Freed via `host->free(self, ptr, len * sizeof(T), align)`
  - CodeGen generates RAII wrappers (Rust `Drop`, Python `__del__`, C# `IDisposable`)
  - Support in both guest and host contract function signatures

#### Instance-to-Contract Mapping
- **D-06:** Add `contract_id: GuestContractId` field to `GuestContractInstance` struct
  - Changes from 8 bytes to 16 bytes (ptr + GuestContractId)
  - Zero lookup overhead for `call_guest_method` dispatch
  - Clear ownership, type-safe contract ID

#### list_bundles ABI
- **D-07:** `list_bundles(self: *const HostInterface) -> Array<BundleId>`
  - Returns just BundleId (u64) — minimal info
  - Host can query individual bundles if needed via other APIs

#### get_dependencies ABI
- **D-08:** `get_dependencies(self: *const HostInterface) -> Array<DependencyInfo>`
  - For plugins to query their own declared dependencies
  - PluginContext still has bundle_id, implementation uses it to look up deps

#### ABI Compatibility
- **D-09:** Accept interface size changes
  - Breaking changes acceptable per PROJECT.md (not published yet)
  - Plugins compile against SDK which handles struct size

#### DependencyInfo Struct
- **D-10:** `DependencyInfo = { contract_id: GuestContractId, min_version: u32, bundle_id: Option<BundleId> }`
  - Mirrors `manifest.toml` `[[dependency]]` structure
  - `get_dependencies` returns `Array<DependencyInfo>`

#### find_all_by_contract Update
- **D-11:** Change signature to `find_all_by_contract(self, contract_id, min_version) -> Array<ContractHandle>`
  - Replaces out-param pattern
  - Single call, no capacity guessing
  - Consistent with new Array pattern

#### GuestContractInterface Changes
- **D-12:** `create_instance` and `destroy_instance` now take `*const HostInterface` instead of `RuntimeContext`

#### HostContractInterface Changes
- **D-13:** Add `runtime: *mut c_void` field, `create_instance` and `destroy_instance` take `self` pointer

#### Documentation Requirements
- **D-14:** First-class documentation for all interface types
  - Every struct has `//!` module doc and `///` item docs
  - Each interface documents: purpose, who provides it, who calls it, ownership, lifetime
  - Every function pointer field has `///` doc explaining parameters, return value, and semantics

### Claude's Discretion
- Exact layout of `Array<T>` and `Vector<T>` structs
- Whether to add helper methods to Array/Vector types
- Error handling for allocation failures

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `polyplug_abi` | workspace | ABI type definitions | Core crate for `#[repr(C)]` FFI types |
| `polyplug` | workspace | Runtime implementation | Core runtime with loader registry |
| `polyplugc` | workspace | Code generator | Generates SDK bindings for all languages |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `polyplug_utils` | workspace | BundleId, GuestContractId types | ID types used throughout |
| `thiserror` | 2.0 | Error types | Structured error handling |
| `serde` | 1.0 | Manifest parsing | TOML deserialization |

### Existing Patterns to Follow
| Pattern | Location | Usage |
|---------|----------|-------|
| `#[repr(C)]` structs | `polyplug_abi/src/**/*.rs` | All FFI types must use this |
| Opaque `*mut c_void` | `GuestContractInstance`, `VmLoaderData` | Hide runtime details from ABI |
| Self-passing pattern | New for this phase | `interface->function(interface, args...)` |
| Caller-frees ownership | `Buffer` type | Memory ownership model for `Array<T>` |

**Installation:**
No new dependencies — uses existing workspace crates.

## Architecture Patterns

### Recommended Project Structure
```
crates/polyplug_abi/src/
├── host/
│   ├── mod.rs                    # Update exports: RuntimeAbi → HostInterface
│   ├── runtime_abi.rs            # RENAME → host_interface.rs
│   ├── runtime_context.rs        # DELETE
│   ├── host_context.rs           # DELETE
│   ├── host_contract_interface.rs # Add runtime field, self-passing
│   └── runtime_interface.rs      # NEW: symmetric with HostInterface
├── guest/
│   ├── guest_contract_instance.rs # Add contract_id field
│   └── guest_contract_interface.rs # Change RuntimeContext → *const HostInterface
├── types/
│   ├── array.rs                  # ENHANCE: add align, ownership docs
│   └── dependency_info.rs        # NEW: DependencyInfo struct
└── lib.rs                        # Update re-exports

crates/polyplug/src/
├── runtime.rs                    # Create RuntimeInterface, update functions
├── runtime_builder.rs            # Return RuntimeInterface from build()
├── ffi.rs                        # Restructure around RuntimeInterface
└── host/
    └── host_context.rs           # DELETE (already absent per research)
```

### Pattern 1: Self-Passing Interface Pattern
**What:** Each interface contains an opaque pointer and all function pointers take `self` as first parameter.
**When to use:** All interface types (RuntimeInterface, HostInterface, GuestContractInterface, HostContractInterface).
**Example:**
```c
// C ABI level (what function pointers look like):
ContractHandle handle = host->find_by_contract(host, contract_id, version);

// Rust definition:
#[repr(C)]
pub struct HostInterface {
    runtime: *mut c_void,  // Opaque pointer
    find_by_contract: unsafe extern "C" fn(
        self: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> ContractHandle,
    // ...
}
```
**Source:** [VERIFIED: CONTEXT.md D-03]

### Pattern 2: Array<T> Caller-Frees Ownership
**What:** Arrays are allocated by the callee via the host allocator, caller must free.
**When to use:** Any FFI function returning multiple items.
**Example:**
```c
// Caller receives array, owns the memory
Array<BundleId> bundles = host->list_bundles(host);
// Use bundles.items[0..bundles.len]
// Free when done:
host->free(host, bundles.items, bundles.len * sizeof(BundleId), bundles.align);
```
**Source:** [VERIFIED: CONTEXT.md D-05, existing Buffer pattern]

### Anti-Patterns to Avoid
- **Wrapper types like RuntimeContext/HostContext:** They add indirection without benefit. Put opaque pointers directly in interfaces.
- **Global/thread-local state in interfaces:** Interfaces must support multiple concurrent runtimes.
- **Out-parameter patterns for arrays:** Use `Array<T>` return value instead of `(ptr, cap)` parameters.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Generic FFI arrays | Custom `T* ptr` pairs | `Array<T>` with align | Proper ownership, alignment tracking |
| Interface vtables | Manual function pointer structs | CodeGen-generated interfaces | Type safety across 5 languages |
| Error handling | Custom error codes | `AbiError` with StringView | Rich error messages via host allocator |

**Key insight:** The existing `Buffer` pattern `{ ptr, len, cap }` should inform `Array<T>` design but `Array<T>` is simpler (no capacity for immutable arrays).

## Runtime State Inventory

> This is a code refactor phase, not a rename/migration. Runtime state changes are type-level only.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — changes are to struct layouts | Code edit |
| Live service config | None — no services run during build | N/A |
| OS-registered state | None — pure library code | N/A |
| Secrets/env vars | None — no configuration changes | N/A |
| Build artifacts | Cargo rebuild required after struct changes | `cargo build` |

**Nothing found in category:** All categories verified as code-only changes.

## Common Pitfalls

### Pitfall 1: Forgetting to Update All SDK Imports
**What goes wrong:** After renaming `RuntimeAbi` → `HostInterface`, SDKs still import the old name.
**Why it happens:** Python SDK uses string imports, C# uses different namespace conventions.
**How to avoid:** Grep for all occurrences of `RuntimeAbi`, `RuntimeContext`, `HostContext`, `rt_ctx` across all SDK files.
**Warning signs:** Compiler errors in SDK code, FFI binding mismatches.

### Pitfall 2: Missing Self Parameter in Function Calls
**What goes wrong:** SDK calls `interface->function(args)` without passing `interface` as first argument.
**Why it happens:** SDKs hide the self-passing from users, but the underlying FFI requires it.
**How to avoid:** Each SDK wrapper must pass `self` (the interface pointer) as first argument to every function pointer call.
**Warning signs:** Segmentation faults, corrupted data, mysterious ABI errors.

### Pitfall 3: Array Ownership Leaks
**What goes wrong:** Caller receives `Array<T>` from introspection API but never frees it.
**Why it happens:** Caller-frees semantics are unusual; developers expect callee to own memory.
**How to avoid:** Document ownership clearly; generate RAII wrappers in each language SDK; add tests for array lifetime.
**Warning signs:** Memory leaks reported by valgrind/sanitizers.

### Pitfall 4: Forgetting contract_id in GuestContractInstance
**What goes wrong:** `call_guest_method` can't dispatch because instance doesn't know its contract.
**Why it happens:** Old GuestContractInstance only had `data: *mut c_void`.
**How to avoid:** Update GuestContractInstance layout tests to expect 16 bytes instead of 8.
**Warning signs:** `call_method` placeholder error, dispatch failures at runtime.

## Code Examples

### HostInterface Structure (Renamed from RuntimeAbi)
```rust
// Source: [VERIFIED: crates/polyplug_abi/src/host/runtime_abi.rs]
// Renamed and modified per CONTEXT.md decisions
#[repr(C)]
pub struct HostInterface {
    /// Opaque pointer to Runtime.
    pub runtime: *mut c_void,
    /// Register a guest contract implementation.
    pub register_contract: unsafe extern "C" fn(
        self: *const HostInterface,
        descriptor: *const PluginDescriptor,
        interface: *const GuestContractInterface,
    ) -> AbiError,
    /// Allocate memory using the host allocator.
    pub alloc: unsafe extern "C" fn(
        self: *const HostInterface,
        size: usize,
        align: usize,
    ) -> *mut u8,
    /// Free memory using the host allocator.
    pub free: unsafe extern "C" fn(
        self: *const HostInterface,
        ptr: *mut u8,
        size: usize,
        align: usize,
    ),
    /// Find a guest contract by contract_id and minimum version.
    pub find_by_contract: unsafe extern "C" fn(
        self: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> ContractHandle,
    /// Find all guest contracts matching criteria. NEW: returns Array.
    pub find_all_by_contract: unsafe extern "C" fn(
        self: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> Array<ContractHandle>,
    /// Resolve a ContractHandle to interface pointer.
    pub resolve_contract: unsafe extern "C" fn(
        self: *const HostInterface,
        handle: ContractHandle,
    ) -> *const GuestContractInterface,
    /// Call a method on a guest contract instance. RENAMED from call_method.
    pub call_guest_method: unsafe extern "C" fn(
        self: *const HostInterface,
        instance: GuestContractInstance,
        method_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    /// Get a host contract instance.
    pub get_host_contract: unsafe extern "C" fn(
        self: *const HostInterface,
        contract_id: u64,
        min_version: u32,
    ) -> HostContractInstance,
    /// List all loaded bundles. NEW.
    pub list_bundles: unsafe extern "C" fn(
        self: *const HostInterface,
    ) -> Array<BundleId>,
    /// Get dependencies for calling bundle. NEW.
    pub get_dependencies: unsafe extern "C" fn(
        self: *const HostInterface,
    ) -> Array<DependencyInfo>,
}
```

### RuntimeInterface Structure (NEW)
```rust
// Source: [VERIFIED: CONTEXT.md D-02]
#[repr(C)]
pub struct RuntimeInterface {
    /// Opaque pointer to Runtime.
    pub runtime: *mut c_void,
    /// Load a plugin bundle.
    pub load_bundle: unsafe extern "C" fn(
        self: *const RuntimeInterface,
        path: *const c_char,
    ) -> AbiError,
    /// Reload a bundle.
    pub reload_bundle: unsafe extern "C" fn(
        self: *const RuntimeInterface,
        bundle_id: BundleId,
    ) -> AbiError,
    /// Unload a bundle.
    pub unload_bundle: unsafe extern "C" fn(
        self: *const RuntimeInterface,
        bundle_id: BundleId,
    ) -> AbiError,
    /// Find a guest contract.
    pub find_by_contract: unsafe extern "C" fn(
        self: *const RuntimeInterface,
        contract_id: u64,
        min_version: u32,
    ) -> ContractHandle,
    /// Find all matching contracts.
    pub find_all_by_contract: unsafe extern "C" fn(
        self: *const RuntimeInterface,
        contract_id: u64,
        min_version: u32,
    ) -> Array<ContractHandle>,
    /// Resolve a handle.
    pub resolve_contract: unsafe extern "C" fn(
        self: *const RuntimeInterface,
        handle: ContractHandle,
    ) -> *const GuestContractInterface,
    /// Get a host contract.
    pub get_host_contract: unsafe extern "C" fn(
        self: *const RuntimeInterface,
        contract_id: u64,
        min_version: u32,
    ) -> HostContractInstance,
    /// Get last error.
    pub get_last_error: unsafe extern "C" fn(
        self: *const RuntimeInterface,
    ) -> StringView,
    /// Destroy the runtime and free this interface.
    pub destroy: unsafe extern "C" fn(
        self: *const RuntimeInterface,
    ),
}
```

### GuestContractInstance with contract_id (MODIFIED)
```rust
// Source: [VERIFIED: crates/polyplug_abi/src/guest/guest_contract_instance.rs]
// Modified per D-06
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GuestContractInstance {
    /// Opaque instance data pointer.
    pub data: *mut c_void,
    /// Contract ID for zero-overhead dispatch. NEW.
    pub contract_id: GuestContractId,
}
// Size changes from 8 bytes to 16 bytes.
```

### Array<T> Enhanced (MODIFIED)
```rust
// Source: [VERIFIED: crates/polyplug_abi/src/types/array.rs]
// Enhanced per D-05
#[repr(C)]
pub struct Array<T: Sized> {
    /// Pointer to elements, allocated via host allocator.
    pub items: *mut T,
    /// Number of elements.
    pub len: usize,
    /// Alignment of T, for proper freeing. NEW.
    pub align: usize,
}
// Total size: 24 bytes (8 + 8 + 8)
```

### DependencyInfo Structure (NEW)
```rust
// Source: [VERIFIED: CONTEXT.md D-10]
#[repr(C)]
pub struct DependencyInfo {
    /// Contract ID of the dependency.
    pub contract_id: GuestContractId,
    /// Minimum version required.
    pub min_version: u32,
    /// Bundle ID if dependency is ByBundle, 0 if ByContract.
    pub bundle_id: BundleId,
}
// Total size: 16 bytes (8 + 4 + 4 padding)
```

### Updated GuestContractInterface Signatures
```rust
// Source: [VERIFIED: crates/polyplug_abi/src/guest/guest_contract_interface.rs]
// Modified per D-12
pub struct GuestContractInterface {
    // ... existing fields ...
    /// Create a new instance. Changed from RuntimeContext to HostInterface.
    pub create_instance: Option<unsafe extern "C" fn(
        host: *const HostInterface,  // Changed
        args: *const (),
    ) -> GuestContractInstance>,
    /// Destroy an instance.
    pub destroy_instance: Option<unsafe extern "C" fn(
        host: *const HostInterface,  // Changed
        instance: GuestContractInstance,
    )>,
}
```

### Updated HostContractInterface with Self-Passing
```rust
// Source: [VERIFIED: crates/polyplug_abi/src/host/host_contract_interface.rs]
// Modified per D-13
#[repr(C)]
pub struct HostContractInterface {
    pub contract_id: HostContractId,
    pub contract_version: Version,
    pub singleton: bool,
    /// Opaque runtime pointer. NEW.
    pub runtime: *mut c_void,
    /// Dispatch type.
    pub dispatch_type: DispatchType,
    /// Create instance. Changed to self-passing.
    pub create_instance: Option<unsafe extern "C" fn(
        self: *const HostContractInterface,  // Changed from RuntimeContext
        args: *const (),
    ) -> HostContractInstance>,
    /// Destroy instance.
    pub destroy_instance: Option<unsafe extern "C" fn(
        self: *const HostContractInterface,  // Changed
        instance: HostContractInstance,
    )>,
    pub dispatch: DispatchMechanisms,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `RuntimeContext` wrapper | Direct `*mut c_void` in interface | This phase | Simpler, clearer ownership |
| `polyplug_runtime_*` scattered FFI | `RuntimeInterface` struct | This phase | Single return value from create |
| `find_all_by_contract` out-params | `Array<T>` return | This phase | No capacity guessing |
| `call_method` placeholder | `call_guest_method` with contract_id | This phase | Enables guest-to-guest calls |
| Manual dependency lookup | `get_dependencies` ABI | This phase | Plugins can query their deps |

**Deprecated/outdated:**
- `RuntimeContext` — deleted, replaced by direct opaque pointer in interfaces
- `HostContext` — deleted, was an indirection layer without benefit
- `rt_ctx` parameter — replaced by self-passing pattern

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | GuestContractInstance changing from 8 to 16 bytes won't break existing callers | Instance-to-Contract | Binary layout change; test all SDKs |
| A2 | `Array<T>` with `align` field is sufficient for all use cases | Array/Vector | May need capacity for mutable arrays |

**Most claims are VERIFIED via codebase reading.**

## Open Questions

1. **Error handling for Array allocation failures**
   - What we know: Host allocator can fail, returning null
   - What's unclear: Should `list_bundles` return empty array or error?
   - Recommendation: Return `Array { items: null, len: 0, align: 0 }` for allocation failure; caller checks `items.is_null()`

2. **polyplug_init signature in existing plugins**
   - What we know: Old signature takes `RuntimeContext` as first param
   - What's unclear: How to handle existing plugins compiled against old SDK
   - Recommendation: Breaking changes are acceptable per project constraints; document migration path

## Environment Availability

> Step 2.6: Phase has no external dependencies beyond Rust toolchain.

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust 1.85+ | Build | ✓ | 1.85 | — |
| Cargo | Build | ✓ | workspace | — |
| Python 3.10+ | Python SDK tests | ✓ | — | Skip Python tests |
| .NET 10.0 | C# SDK tests | ✓ | — | Skip C# tests |

**Missing dependencies with no fallback:** None.

**Missing dependencies with fallback:** Python/.NET test dependencies — tests can be skipped.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `cargo test` |
| Config file | None — tests inline in source files |
| Quick run command | `cargo test -p polyplug_abi --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| D-01 | RuntimeAbi renamed to HostInterface | unit | `cargo test -p polyplug_abi layout_host_interface` | ❌ Wave 0 |
| D-02 | RuntimeInterface exists | unit | `cargo test -p polyplug_abi layout_runtime_interface` | ❌ Wave 0 |
| D-03 | RuntimeContext deleted | compile | `cargo build -p polyplug_abi` | ❌ Wave 0 |
| D-05 | Array<T> has align field | unit | `cargo test -p polyplug_abi layout_array` | ❌ Wave 0 |
| D-06 | GuestContractInstance has contract_id | unit | `cargo test -p polyplug_abi layout_guest_contract_instance` | ✅ Update existing |
| D-10 | DependencyInfo struct exists | unit | `cargo test -p polyplug_abi layout_dependency_info` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p polyplug_abi -p polyplug --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/polyplug_abi/src/host/runtime_interface.rs` — tests for RuntimeInterface layout
- [ ] `crates/polyplug_abi/src/types/array.rs` — tests for enhanced Array layout
- [ ] `crates/polyplug_abi/src/types/dependency_info.rs` — tests for DependencyInfo layout
- [ ] Update existing tests in `guest_contract_instance.rs` for new 16-byte size

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|------------------|
| V2 Authentication | no | N/A — plugin runtime, not auth system |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A |
| V5 Input Validation | yes | All FFI params validated at boundary |
| V6 Cryptography | no | N/A |

### Known Threat Patterns for FFI Boundary

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Null pointer dereference | Tampering, DoS | All FFI entry points check null before use |
| Buffer overrun | Tampering | Buffer/Array types track length |
| Use-after-free | Tampering | Caller-frees with explicit ownership docs |
| ABI version mismatch | Tampering | `POLYPLUG_ABI_VERSION` sentinel check |

## Sources

### Primary (HIGH confidence)
- `crates/polyplug_abi/src/host/runtime_abi.rs` — Current RuntimeAbi structure [VERIFIED]
- `crates/polyplug_abi/src/guest/guest_contract_instance.rs` — Current GuestContractInstance [VERIFIED]
- `crates/polyplug_abi/src/host/host_context.rs` — HostContext to delete [VERIFIED]
- `crates/polyplug_abi/src/host/runtime_context.rs` — RuntimeContext to delete [VERIFIED]
- `crates/polyplug/src/runtime.rs` — Runtime implementation, host functions [VERIFIED]
- `crates/polyplug/src/runtime_builder.rs` — RuntimeBuilder, host_abi creation [VERIFIED]

### Secondary (MEDIUM confidence)
- `crates/polyplug/src/ffi.rs` — Current FFI functions to restructure [VERIFIED]
- `crates/polyplug/src/registry/plugin_registry.rs` — Registry implementation [VERIFIED]
- `crates/polyplug/src/loader/manifest.rs` — ManifestData, ManifestDependency [VERIFIED]
- `crates/polyplugc/src/generators/rust.rs` — CodeGen patterns [VERIFIED]
- `sdks/python/host/polyplug/runtime.py` — Python SDK patterns [VERIFIED]

### Tertiary (LOW confidence)
- None — all critical claims verified from source code.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — All types verified from existing codebase
- Architecture: HIGH — Pattern follows existing Buffer/GuestContractInstance designs
- Pitfalls: HIGH — Based on common FFI mistakes and existing placeholder code

**Research date:** 2026-04-07
**Valid until:** 30 days — stable architecture, no fast-moving dependencies