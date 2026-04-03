# Phase 1: ABI Types - Research

**Researched:** 2026-04-03
**Domain:** Rust FFI types, ABI boundary design, type renaming/migration
**Confidence:** HIGH

## Summary

Phase 1 consolidates all FFI types in `polyplug_abi` crate with renamed interfaces following Guest/Host terminology. The current architecture has types split across three crates (`polyplug_abi`, `polyplug`, `polyplug_utils`) with inconsistent naming (PluginContractId, PluginInterface, HostVTable). The HostContractVTable family of types is documented in `docs/HOST_CONTRACTS_API.md` but NOT yet implemented in code.

**Primary recommendation:** Create new types in `polyplug_abi` first, then update imports across dependent crates, ensuring all types are `#[repr(C)]` with stable ABI layouts.

## Current Architecture Analysis

### Crate Dependency Graph

```
polyplug_utils (standalone - ID types, FNV-1a hashing)
       |
       v
polyplug_abi (FFI types, depends on polyplug_utils for PluginContractId)
       |
       v
polyplug (runtime, depends on polyplug_abi + polyplug_utils)
       |
       v
sdks/rust/guest, sdks/rust/host (depend on polyplug_abi)
```

**Key insight:** `polyplug_abi` currently depends on `polyplug_utils` for `PluginContractId`. After rename to `GuestContractId`, this dependency remains.

### Current Type Locations

| Type | Current Location | Needs Action |
|------|------------------|--------------|
| `PluginInterface` | `polyplug_abi/src/plugin/plugin_interface.rs` | Rename to `GuestContractInterface` |
| `HostVTable` | `polyplug_abi/src/host/host_vtable.rs` | Rename to `RuntimeAbi` |
| `PluginHandle` | `polyplug_abi/src/plugin/plugin_handle.rs` | Keep (used for ContractHandle later) |
| `PluginDescriptor` | `polyplug_abi/src/plugin/plugin_descriptor.rs` | Keep |
| `PluginContext` | `polyplug_abi/src/plugin/plugin_context.rs` | Keep |
| `RuntimeConfig` | `polyplug/src/runtime_config.rs` | Move to `polyplug_abi` |
| `ReloadPhase` | `polyplug/src/reload.rs` | Move to `polyplug_abi` |
| `LoadOptions` | `polyplug/src/runtime.rs` | Keep in polyplug (internal) |
| `Compatibility` | `polyplug/src/compatibility/compatibility.rs` | Move to `polyplug_abi` (RuntimeConfig dependency) |
| `PluginContractId` | `polyplug_utils/src/plugin_contract_id.rs` | Rename to `GuestContractId` |
| `HostContractId` | `polyplug_utils/src/host_contract_id.rs` | Keep |

### Current Type Layouts

**PluginInterface** (40 bytes, align 8):
```rust
#[repr(C)]
pub struct PluginInterface {
    pub contract_id: PluginContractId,    // 8 bytes (offset 0)
    pub contract_version: Version,        // 6 bytes (offset 8)
    pub dispatch_type: DispatchType,      // 4 bytes (offset 14)
    pub dispatch: DispatchMechanisms,     // 16 bytes (offset 18)
}
```

**HostVTable** (64 bytes, align 8):
```rust
#[repr(C)]
pub struct HostVTable {
    pub register_plugin: extern "C" fn(...),      // 8 bytes (offset 0)
    pub alloc: extern "C" fn(...),                // 8 bytes (offset 8)
    pub free: extern "C" fn(...),                 // 8 bytes (offset 16)
    pub find_by_contract: extern "C" fn(...),     // 8 bytes (offset 24)
    pub find_by_bundle: extern "C" fn(...),       // 8 bytes (offset 32)
    pub find_all_by_contract: extern "C" fn(...), // 8 bytes (offset 40)
    pub resolve_plugin: extern "C" fn(...),       // 8 bytes (offset 48)
    pub get_host_contract: extern "C" fn(...),    // 8 bytes (offset 56) - ALREADY EXISTS
}
```

**RuntimeConfig** (24 bytes, align 8):
```rust
#[repr(C)]
pub struct RuntimeConfig {
    pub hot_reload_enabled: bool,                 // 1 byte (offset 0)
    pub hot_reload_max_retries: u32,              // 4 bytes (offset 4)
    pub hot_reload_retry_interval_ms: u64,        // 8 bytes (offset 8)
    pub hot_reload_abort_on_max_retries: bool,    // 1 byte (offset 16)
    pub compatibility: Compatibility,             // 4 bytes (offset 20)
}
```

**ReloadPhase** (NOT #[repr(C)] - enum with String fields):
```rust
pub enum ReloadPhase {
    Preparing { bundle_id: BundleId, bundle_name: String, retry_count: u32 },
    Reloaded { bundle_id: BundleId, bundle_name: String },
    Failed { bundle_id: BundleId, bundle_name: String, reason: String },
}
```

**VmDispatch** (16 bytes, align 8):
```rust
#[repr(C)]
pub struct VmDispatch {
    pub call: unsafe extern "C" fn(loader_data: *mut c_void, fn_id: u32, args: *const (), out: *mut ()) -> AbiError,
    pub loader_data: *mut c_void,
}
```

### Missing Types (Not Yet Implemented)

The following types are documented in `docs/HOST_CONTRACTS_API.md` but NOT present in code:

1. **HostContractVTableHeader** (48 bytes planned)
2. **NativeHostContractDispatch** (16 bytes planned)
3. **VmHostContractDispatch** (16 bytes planned)
4. **HostContractDispatch** (union, 16 bytes planned)
5. **HostContractVTable** (64 bytes planned)

These need to be created in `polyplug_abi`.

### ID Type Rename Analysis

**PluginContractId** uses prefix `"plugin_contract:"` for hashing:
```rust
pub fn new(name: &str, major_version: u32) -> Self {
    Self(contract_id("plugin_contract:", name, major_version))
}
```

After rename to `GuestContractId`, the prefix should remain `"plugin_contract:"` to maintain ABI stability OR change to `"guest_contract:"` (DECISION NEEDED).

**HostContractId** already exists with prefix `"host_contract:"`.

## Standard Stack

### Core Types (polyplug_abi)

| Type | Version | Purpose | Status |
|------|---------|---------|--------|
| `GuestContractInterface` | NEW | Renamed from PluginInterface | Rename existing |
| `HostContractInterface` | NEW | Host-side contract interface | Create new |
| `RuntimeAbi` | NEW | Renamed from HostVTable | Rename existing |
| `RuntimeConfig` | MOVE | Runtime configuration | Move from polyplug |
| `ReloadPhase` | MOVE | Hot-reload phases | Move from polyplug |
| `GuestContractInstance` | NEW | Opaque guest instance handle | Create new |
| `HostContractInstance` | NEW | Opaque host instance handle | Create new |

### Supporting Types (polyplug_utils)

| Type | Version | Purpose | Action |
|------|---------|---------|--------|
| `GuestContractId` | NEW | Renamed from PluginContractId | Rename existing |
| `HostContractId` | EXISTS | Host contract ID hash | Keep unchanged |
| `BundleId` | EXISTS | Bundle identifier | Keep unchanged |

## Architecture Patterns

### Recommended Project Structure After Phase 1

```
crates/polyplug_abi/src/
├── lib.rs                  # Barrel file exports
├── guest/                  # NEW: Guest-side types
│   ├── mod.rs
│   ├── guest_contract_interface.rs  # Renamed from plugin_interface.rs
│   ├── guest_contract_instance.rs   # NEW
│   ├── guest_contract_id.rs         # Could move from polyplug_utils
│   └── guest_descriptor.rs          # Renamed from plugin_descriptor.rs
├── host/                   # Host-side types
│   ├── mod.rs
│   ├── host_contract_interface.rs   # NEW
│   ├── host_contract_instance.rs    # NEW
│   ├── host_contract_vtable.rs      # NEW (from docs spec)
│   ├── host_context.rs              # Keep
│   └── host_vtable.rs               # Rename to runtime_abi.rs
├── runtime/                # NEW: Runtime config types
│   ├── mod.rs
│   ├── runtime_config.rs            # Moved from polyplug
│   ├── reload_phase.rs              # Moved from polyplug
│   ├── compatibility.rs             # Moved from polyplug
│   └── runtime_create_options.rs    # NEW (if needed)
├── dispatch/
│   ├── dispatch_type.rs             # Keep
│   ├── native_dispatch.rs           # Update for instance param
│   ├── vm_dispatch.rs               # Update for instance param
│   └── dispatch_mechanisms.rs       # Keep
├── types/
│   ├── string_view.rs               # Keep
│   ├── buffer.rs                    # Keep
│   ├── version.rs                   # Keep
│   ├── abi_error.rs                 # Keep
│   └── error_code.rs                # Keep
└── ffi.rs                           # Keep (allocator functions)
```

### Pattern 1: GuestContractInterface with Instance Factory

**What:** Interface struct with create_instance/destroy_instance function pointers

**Example:**
```rust
#[repr(C)]
pub struct GuestContractInterface {
    pub contract_id: GuestContractId,
    pub contract_version: Version,
    pub dispatch_type: DispatchType,
    pub create_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        args: *const (),
    ) -> GuestContractInstance,
    pub destroy_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        instance: GuestContractInstance,
    ),
    pub dispatch: DispatchMechanisms,
}
```

### Pattern 2: HostContractInterface

**What:** Interface for host-provided contracts with singleton support

**Example:**
```rust
#[repr(C)]
pub struct HostContractInterface {
    pub contract_id: HostContractId,
    pub contract_version: Version,
    pub singleton: bool,  // true = same instance for all callers
    pub create_instance: unsafe extern "C" fn(...) -> HostContractInstance,
    pub destroy_instance: unsafe extern "C" fn(...),
    pub dispatch: HostContractDispatch,
}
```

### Anti-Patterns to Avoid

- **Non-repr(C) enums in ABI:** `ReloadPhase` currently has `String` fields - must be redesigned for FFI compatibility
- **Bare `c_void` pointers:** Use typed opaque handles (`GuestContractInstance`, `HostContractInstance`) instead
- **Inconsistent prefixes:** Don't mix `"plugin_contract:"` and `"guest_contract:"` hashing prefixes without coordination

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Contract ID hashing | Custom hash function | `polyplug_utils::fnv1a_64` | ABI stability - same hash algorithm required |
| Instance handles | Raw `*mut c_void` | `GuestContractInstance` struct | Type safety, future typed handles (Phase 7) |
| Dispatch union | Separate structs per type | `DispatchMechanisms` union | ABI compatibility, single dispatch path |

## Common Pitfalls

### Pitfall 1: Breaking ABI Layouts

**What goes wrong:** Changing field order or adding fields without considering alignment breaks FFI compatibility

**Why it happens:** Developers treat Rust structs like regular data structures, not C ABI types

**How to avoid:**
1. Run layout tests after any struct change
2. Use `#[repr(C)]` on all public ABI types
3. Add padding fields explicitly for alignment

**Warning signs:** Test failures in `layout_*` tests, crash when calling across FFI boundary

### Pitfall 2: ReloadPhase FFI Incompatibility

**What goes wrong:** `ReloadPhase` enum has `String` fields, cannot pass across FFI

**Why it happens:** Current implementation is Rust-only, not designed for cross-language callbacks

**How to avoid:**
1. Create `ReloadPhaseData` struct with `StringView` for FFI
2. Keep Rust `ReloadPhase` enum for internal use
3. Convert between them at FFI boundary

### Pitfall 3: Circular Dependencies After Move

**What goes wrong:** Moving `RuntimeConfig` to `polyplug_abi` requires `Compatibility` enum, creating import chain issues

**Why it happens:** `RuntimeConfig` uses `Compatibility` which is in `polyplug`

**How to avoid:**
1. Move `Compatibility` enum to `polyplug_abi` FIRST
2. Then move `RuntimeConfig`
3. Update `polyplug` imports to use `polyplug_abi::Compatibility`

## Code Examples

### Current PluginInterface (to rename)

```rust
// Source: crates/polyplug_abi/src/plugin/plugin_interface.rs
#[repr(C)]
pub struct PluginInterface {
    pub contract_id: PluginContractId,
    pub contract_version: Version,
    pub dispatch_type: DispatchType,
    pub dispatch: DispatchMechanisms,
}
```

### Proposed GuestContractInterface

```rust
// Target: crates/polyplug_abi/src/guest/guest_contract_interface.rs
#[repr(C)]
pub struct GuestContractInterface {
    pub contract_id: GuestContractId,
    pub contract_version: Version,
    pub dispatch_type: DispatchType,
    pub create_instance: unsafe extern "C" fn(
        rt_ctx: RuntimeContext,
        args: *const (),
    ) -> GuestContractInstance,
    pub destroy_instance: unsafe extern "C" fn(
        rt_ctx: RuntimeContext,
        instance: GuestContractInstance,
    ),
    pub dispatch: DispatchMechanisms,
}
```

### VmDispatch Update (ABI-09)

```rust
// Current: crates/polyplug_abi/src/dispatch/vm_dispatch.rs
pub struct VmDispatch {
    pub call: unsafe extern "C" fn(
        loader_data: *mut c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    pub loader_data: *mut c_void,
}

// Target: Add instance parameter
pub struct VmDispatch {
    pub call: unsafe extern "C" fn(
        loader_data: *mut c_void,
        instance: GuestContractInstance,  // NEW
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    pub loader_data: *mut c_void,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `PluginInterface` | `GuestContractInterface` | Phase 1 | Clearer host/guest separation |
| `HostVTable` | `RuntimeAbi` | Phase 1 | Runtime != host, clearer naming |
| `PluginContractId` | `GuestContractId` | Phase 1 | Consistent with GuestContractInterface |
| Bare `c_void` instance pointers | `GuestContractInstance` handle | Phase 1 | Type safety at ABI boundary |
| `*mut c_void` rt_ctx | `RuntimeContext` handle | Phase 7 | Final typed handle replacement |

**Deprecated/outdated:**
- "vtable" naming: Use "interface" for contract definitions
- "plugin" prefix for guest-side types: Use "guest" for clarity
- `*C` suffix types: All types should be canonical in `polyplug_abi`

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust 1.85+ | All crates | ✓ | 1.85 | — |
| Cargo workspace | Build system | ✓ | — | — |

**Missing dependencies with no fallback:** None

**Missing dependencies with fallback:** None - this phase is pure code/config changes.

## Open Questions (RESOLVED)

1. **GuestContractId hash prefix** — RESOLVED: Use `"guest_contract:"` prefix for naming consistency.
   - Original: PluginContractId uses `"plugin_contract:"` prefix
   - Decision: Change to `"guest_contract:"` for consistent Guest/Host terminology
   - Impact: BREAKING CHANGE - existing contract IDs will have different hash values

2. **ReloadPhase FFI representation** — RESOLVED: Create `ReloadPhaseData` struct with `StringView` fields, keep `ReloadPhase` enum for Rust internal use.

3. **RuntimeCreateOptions type** — RESOLVED: Type does not exist in current codebase. ABI-07 deferred.

4. **HostContractVTable placement** — RESOLVED: Create in `polyplug_abi/src/host/host_contract_vtable.rs` alongside host_vtable.rs.

## Phase Requirements Map

| ID | Description | Research Support |
|----|-------------|------------------|
| ABI-01 | Rename PluginInterface to GuestContractInterface | Found current struct at `polyplug_abi/src/plugin/plugin_interface.rs` (40 bytes, repr(C)) |
| ABI-02 | Create HostContractInterface with singleton field | Spec in docs/HOST_CONTRACTS_API.md shows 48-byte header + dispatch |
| ABI-03 | Add create/destroy_instance to GuestContractInterface | Current PluginInterface has no instance methods - need to add function pointers |
| ABI-04 | Add create/destroy_instance to HostContractInterface | Same pattern as GuestContractInterface |
| ABI-05 | Move RuntimeConfig to polyplug_abi | Found at `polyplug/src/runtime_config.rs` (24 bytes, repr(C)) |
| ABI-06 | Move ReloadPhase to polyplug_abi | Found at `polyplug/src/reload.rs` - NOT repr(C), needs redesign |
| ABI-07 | Move RuntimeCreateOptions to polyplug_abi | Type not found - may be LoadOptions or needs creation |
| ABI-08 | Rename HostVTable to RuntimeAbi | Found at `polyplug_abi/src/host/host_vtable.rs` (64 bytes, repr(C)) |
| ABI-09 | Update VmDispatch with instance param | Found at `polyplug_abi/src/dispatch/vm_dispatch.rs` (16 bytes) - add instance param |
| ABI-10 | Add call_method to RuntimeAbi | HostVTable has 8 functions - need to add call_method as 9th |
| ABI-11 | Rename PluginContractId to GuestContractId | Found at `polyplug_utils/src/plugin_contract_id.rs` |
| ABI-12 | All public ABI structs #[repr(C)] | Verified: PluginInterface, HostVTable, RuntimeConfig are repr(C); ReloadPhase is NOT |
| RTABI-01 | Rename register_plugin (was register_contract) | HostVTable field name - rename in RuntimeAbi |
| RTABI-02 | find_contract returns ContractHandle | HostVTable.find_by_contract returns PluginHandle |
| RTABI-03 | resolve_contract returns interface pointer | HostVTable.resolve_plugin returns *const PluginInterface |
| RTABI-04 | get_host_contract returns HostContractInstance | Current returns *const HostContractVTable (bare pointer) |
| RTABI-05 | Remove find_by_bundle from ABI | HostVTable.find_by_bundle exists at offset 32 |

## Tests That Need Updating

| Test File | Location | Reason |
|-----------|----------|--------|
| `layout_plugin_interface` | `polyplug_abi/src/plugin/plugin_interface.rs` | Rename to layout_guest_contract_interface |
| `layout_host_vtable` | `polyplug_abi/src/host/host_vtable.rs` | Rename to layout_runtime_abi |
| `layout_runtime_config` | `polyplug/src/runtime_config.rs` | Move to polyplug_abi tests |
| `reload_phase_*` tests | `polyplug/src/reload.rs` | Move to polyplug_abi after type redesign |
| Integration tests | `tests/integration/tests/*.rs` | Update import paths after renames |
| Generated code tests | `polyplugc/src/generators/*.rs` | Update generated type names |

## Sources

### Primary (HIGH confidence)
- `crates/polyplug_abi/src/lib.rs` - ABI type exports
- `crates/polyplug_abi/src/plugin/plugin_interface.rs` - Current PluginInterface definition
- `crates/polyplug_abi/src/host/host_vtable.rs` - Current HostVTable definition
- `crates/polyplug/src/runtime_config.rs` - RuntimeConfig definition
- `crates/polyplug/src/reload.rs` - ReloadPhase definition

### Secondary (MEDIUM confidence)
- `docs/HOST_CONTRACTS_API.md` - HostContractVTable specification (documented, not implemented)
- `crates/polyplug_abi/build/extractor.rs` - ABI_TYPES list shows planned types
- `.planning/REQUIREMENTS.md` - Phase 1 requirements list

### Tertiary (LOW confidence)
- WebSearch not used - all information from codebase inspection

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Found all existing types in codebase
- Architecture: HIGH - Crate dependency graph verified via Cargo.toml
- Pitfalls: HIGH - Identified ReloadPhase FFI issue, circular dependency risk
- Missing types: MEDIUM - HostContractVTable family documented but not implemented

**Research date:** 2026-04-03
**Valid until:** 30 days - stable Rust FFI patterns

---

## RESEARCH COMPLETE

**Phase:** 1 - ABI Types
**Confidence:** HIGH

### Key Findings

1. **PluginInterface** (40 bytes) exists in `polyplug_abi/src/plugin/plugin_interface.rs` - needs rename to GuestContractInterface and addition of create_instance/destroy_instance fields

2. **HostVTable** (64 bytes) exists in `polyplug_abi/src/host/host_vtable.rs` with 8 function pointers including `get_host_contract` - needs rename to RuntimeAbi and addition of `call_method`

3. **RuntimeConfig** (24 bytes, repr(C)) exists in `polyplug/src/runtime_config.rs` - needs move to polyplug_abi along with `Compatibility` enum dependency

4. **ReloadPhase** is NOT repr(C) - has String fields making it FFI-incompatible - needs redesign with StringView or separate FFI struct

5. **HostContractVTable family** is documented in `docs/HOST_CONTRACTS_API.md` but NOT implemented in code - needs creation in polyplug_abi

6. **PluginContractId** uses `"plugin_contract:"` hash prefix - recommend keeping prefix after rename to avoid ABI breakage

### File Created

`.planning/phases/01-abi-types/01-RESEARCH.md`

### Confidence Assessment

| Area | Level | Reason |
|------|-------|--------|
| Standard Stack | HIGH | Found all current type definitions in codebase |
| Architecture | HIGH | Verified crate dependency graph via Cargo.toml files |
| Pitfalls | HIGH | Identified ReloadPhase FFI incompatibility, circular dependency chains |
| Missing Types | MEDIUM | HostContractVTable family documented but not yet implemented |

### Open Questions

1. GuestContractId hash prefix - keep `"plugin_contract:"` or change to `"guest_contract:"`?
2. ReloadPhase FFI representation - redesign enum or create separate FFI struct?
3. RuntimeCreateOptions definition - not found in current codebase

### Ready for Planning

Research complete. Planner can now create PLAN.md files addressing:
- Type renames (ABI-01, ABI-08, ABI-11)
- Type moves (ABI-05, ABI-06, ABI-07)
- New type creation (ABI-02, ABI-03, ABI-04, ABI-13, ABI-14)
- Function signature updates (ABI-09, ABI-10, RTABI-01 through RTABI-05)