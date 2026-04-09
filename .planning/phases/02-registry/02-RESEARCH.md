# Phase 02: Registry - Research

**Researched:** 2026-04-04
**Domain:** Registry simplification - remove VTableSlot wrapper, PluginGuard, generation counters, ArcSwap
**Confidence:** HIGH

## Summary

Phase 2 simplifies the registry by removing layers of indirection that were designed for an Arc-based hot-reload safety model. The new instance-based model (Phase 3) makes these unnecessary because the host explicitly destroys instances before hot-reload via callback, eliminating the need for:
- **VTableSlot wrapper** - Was needed to wrap `*const GuestContractInterface` in an Arc for quiescence tracking
- **PluginGuard** - Was an RAII guard that held `Arc<VTableSlot>` to keep vtables alive during calls
- **Generation counters** - Were used to detect stale handles after hot-reload (now unnecessary since instances are destroyed before swap)
- **ArcSwap pattern** - Was used for atomic vtable swapping with quiescence wait

**Primary recommendation:** Remove components in order: VTableSlot wrapper first, then PluginGuard, then ArcSwap, then generation counter. This preserves compilation at each step.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REG-01 | Remove VTableSlot wrapper - store GuestContractInterface directly | Replace `ArcSwap<VTableSlot>` with `Option<Arc<GuestContractInterface>>` or direct pointer |
| REG-02 | Remove PluginGuard - replaced by instance model | Delete struct and `resolve_guard()` method; replace with direct interface access |
| REG-03 | Remove generation counter from handles (ContractHandle) | Modify `GuestContractHandle` to have only `index` field; update `StaleHandle` error handling |
| REG-04 | Remove ArcSwap pattern - hot-reload uses callback instead | Replace `ArcSwap` with direct swap under RwLock write guard |
| REG-05 | Simplify RegistrySlot to store interface directly | Remove `vtable: Option<ArcSwap<VTableSlot>>`, replace with direct interface storage |
| REG-06 | Update find_contract to return ContractHandle without generation | Remove generation loading from all find methods |
</phase_requirements>

## Standard Stack

### Core Registry Types (Current)

| Type | Location | Purpose | Changes Required |
|------|----------|---------|------------------|
| `VTableSlot` | `plugin_registry.rs:29` | Wrapper for `*const GuestContractInterface` | DELETE - store interface directly |
| `PluginGuard` | `plugin_registry.rs:36-54` | RAII guard holding `Arc<VTableSlot>` | DELETE - no guard needed |
| `RegistrySlot` | `plugin_registry.rs:70-78` | Slot with generation, entry, vtable | Simplify - remove generation, store interface directly |
| `GuestContractHandle` | `polyplug_abi/plugin_handle.rs` | Handle with index + generation | Simplify - keep only index |
| `PluginRegistry` | `plugin_registry.rs:112-115` | Main registry struct | Modify slot storage and methods |

### Dependencies to Remove

| Dependency | Current Use | After Phase 2 |
|------------|-------------|---------------|
| `arc_swap` 1.7 | Atomic vtable swapping for hot-reload | Remove - not needed in Cargo.toml |
| `Arc<VTableSlot>` | Reference counting for quiescence | Remove - no quiescence tracking |

## Architecture Patterns

### Current Pattern: Generational Index + ArcSwap

```
┌─────────────────────────────────────────────────────────────────┐
│                     Current Registry Architecture               │
├─────────────────────────────────────────────────────────────────┤
│ GuestContractHandle { index, generation }                              │
│         │                                                        │
│         ▼                                                        │
│ RegistrySlot {                                                   │
│     generation: AtomicU32,      // Detect stale handles         │
│     entry: Option<RegistryEntry>,                                │
│     vtable: Option<ArcSwap<VTableSlot>>,  // Atomic swap        │
│ }                                                                │
│         │                                                        │
│         ▼                                                        │
│ VTableSlot(*const GuestContractInterface)  // Wrapper           │
│         │                                                        │
│         ▼                                                        │
│ PluginGuard { slot: Arc<VTableSlot> }  // RAII guard            │
└─────────────────────────────────────────────────────────────────┘
```

### Target Pattern: Direct Storage

```
┌─────────────────────────────────────────────────────────────────┐
│                     Target Registry Architecture                │
├─────────────────────────────────────────────────────────────────┤
│ ContractHandle { index }  // No generation                      │
│         │                                                        │
│         ▼                                                        │
│ RegistrySlot {                                                   │
│     entry: Option<RegistryEntry>,                                │
│     interface: Option<Arc<GuestContractInterface>>,  // Direct  │
│ }                                                                │
│         │                                                        │
│         ▼                                                        │
│ GuestContractInterface directly accessible                       │
│ (no guard, no wrapper)                                           │
└─────────────────────────────────────────────────────────────────┘
```

### Key Methods to Modify

| Method | Current Implementation | Target Implementation |
|--------|------------------------|------------------------|
| `register()` | Returns `GuestContractHandle { index, generation }` | Returns `ContractHandle { index }` |
| `find_by_contract()` | Loads generation from slot | Returns handle with index only |
| `resolve_guard()` | Returns `PluginGuard` holding Arc | DELETE - replace with `resolve()` returning interface |
| `resolve()` | Calls `resolve_guard()` | Returns `*const GuestContractInterface` directly |
| `swap_vtable()` | Uses `ArcSwap::swap()`, bumps generation | Direct swap under write lock, no generation |
| `get_vtable_arc()` | Returns `Arc<VTableSlot>` for quiescence | DELETE - no quiescence tracking |

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Thread-safe slot access | Custom lock-free structure | `RwLock` as current | Simple, proven, adequate performance |
| Interface lifetime | Manual refcounting | `Arc<GuestContractInterface>` | Safe reference counting without VTableSlot wrapper |

**Key insight:** The `ArcSwap` and quiescence patterns were solving a specific hot-reload safety problem that is now addressed differently (callback-based instance destruction). The simpler pattern is actually safer because it's explicit.

## Runtime State Inventory

> Phase involves refactoring core registry - no external runtime state affected.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None - registry is in-memory only | None |
| Live service config | None - no external services | None |
| OS-registered state | None | None |
| Secrets/env vars | None | None |
| Build artifacts | None - no artifacts carry registry types | None |

**All categories empty:** Registry is pure in-memory Rust code with no external state.

## Common Pitfalls

### Pitfall 1: Removing Components in Wrong Order

**What goes wrong:** Removing PluginGuard first leaves `resolve()` with no return type, breaking compilation across many files.

**Why it happens:** Each component depends on others - VTableSlot is used by PluginGuard, ArcSwap stores VTableSlot.

**How to avoid:** Remove in dependency order:
1. VTableSlot wrapper first (replace with direct interface in Arc)
2. PluginGuard second (delete after VTableSlot is gone)
3. ArcSwap third (replace with direct swap)
4. Generation counter last (simplest, no cascading changes)

**Warning signs:** Compilation errors in unexpected files after a change.

### Pitfall 2: Forgetting FFI Handle Packing/Unpacking

**What goes wrong:** FFI layer packs handles as `(generation << 32) | index`. Removing generation breaks this.

**Why it happens:** `ffi.rs:117-128` has pack/unpack functions that assume generation exists.

**How to avoid:** Update pack/unpack to just return index (or remove packing entirely since it's now just a u32).

**Warning signs:** Test failures in FFI handle roundtrip tests.

### Pitfall 3: Not Updating Tests That Use PluginGuard

**What goes wrong:** Many tests call `resolve_guard()` and use `PluginGuard::vtable()`.

**Why it happens:** Tests in `registry_edge_cases.rs`, `hot_reload_safety.rs`, `stress_*.rs` all use the guard pattern.

**How to avoid:** Update all tests to use direct interface access. This is a large change.

**Warning signs:** ` unresolved import: PluginGuard` or `method resolve_guard not found`.

### Pitfall 4: Leaving ArcSwap Dependency

**What goes wrong:** Cargo.toml still has `arc-swap` dependency but it's unused.

**Why it happens:** Dependency cleanup is often forgotten after code changes.

**How to avoid:** Remove `arc-swap = "1.7"` from `Cargo.toml` after code changes are complete.

**Warning signs:** Cargo warnings about unused dependencies.

## Code Examples

### Current VTableSlot and PluginGuard (to remove)

```rust
// Source: crates/polyplug/src/registry/plugin_registry.rs:29-54
pub struct VTableSlot(pub *const GuestContractInterface);

pub struct PluginGuard {
    pub(crate) slot: Arc<VTableSlot>,
}

impl PluginGuard {
    pub fn vtable(&self) -> *const GuestContractInterface {
        self.slot.0
    }
}
```

### Current RegistrySlot with ArcSwap (to simplify)

```rust
// Source: crates/polyplug/src/registry/plugin_registry.rs:70-78
pub(crate) struct RegistrySlot {
    pub generation: AtomicU32,
    pub entry: Option<RegistryEntry>,
    pub vtable: Option<ArcSwap<VTableSlot>>,
}
```

### Target RegistrySlot (simplified)

```rust
// Target implementation
pub(crate) struct RegistrySlot {
    pub entry: Option<RegistryEntry>,
    pub interface: Option<Arc<GuestContractInterface>>,
}
```

### Current GuestContractHandle (to simplify)

```rust
// Source: crates/polyplug_abi/src/plugin/plugin_handle.rs:6-12
#[repr(C)]
pub struct GuestContractHandle {
    pub index: u32,
    pub generation: u32,
}
```

### Target ContractHandle (or rename GuestContractHandle)

```rust
// Target implementation - keep GuestContractHandle name for backward compat
#[repr(C)]
pub struct GuestContractHandle {
    pub index: u32,
}
```

### Current swap_vtable (to simplify)

```rust
// Source: crates/polyplug/src/registry/plugin_registry.rs:512-544
pub fn swap_vtable(
    &self,
    slot_index: u32,
    new_vtable: Arc<VTableSlot>,
) -> Result<Arc<VTableSlot>, RegistryError> {
    // Uses ArcSwap::swap() and bumps generation
}
```

### Target swap_interface (simplified)

```rust
// Target implementation
pub fn swap_interface(
    &self,
    slot_index: u32,
    new_interface: Arc<GuestContractInterface>,
) -> Result<(), RegistryError> {
    let mut data = self.data.write().unwrap();
    let slot = &mut data.slots[slot_index as usize];
    slot.interface = Some(new_interface);
    Ok(())
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Arc quiescence tracking | Callback-based instance destruction | Phase 1 decision | Removes need for PluginGuard/ArcSwap |
| Generation counters for stale detection | Explicit instance destruction before swap | Phase 1 decision | Generation unnecessary |
| VTableSlot wrapper | Direct interface storage | This phase | Simpler code, fewer types |

**Deprecated/outdated:**
- `VTableSlot`: Wrapper was only needed for Arc wrapping - now store interface directly
- `PluginGuard`: RAII guard was for quiescence - now caller gets interface directly
- `ArcSwap`: Atomic swap was for hot-reload safety - now swap under lock

## Open Questions

1. **Should GuestContractHandle be renamed to ContractHandle?**
   - What we know: Requirements use "ContractHandle" terminology
   - What's unclear: Whether rename happens in this phase or later
   - Recommendation: Keep `GuestContractHandle` name but simplify structure; rename in Phase 6 Cleanup

2. **Should we keep Arc<GuestContractInterface> or store raw pointer?**
   - What we know: Interface is `'static` from plugin library
   - What's unclear: Whether Arc provides any benefit without quiescence tracking
   - Recommendation: Keep Arc for safe lifetime tracking - library handles are never unloaded

3. **What happens to StaleHandle error?**
   - What we know: StaleHandle requires generation comparison
   - What's unclear: Whether we need any handle validation error
   - Recommendation: Replace with `InvalidHandle` error for out-of-bounds index only

## Environment Availability

> SKIPPED - Phase has no external dependencies beyond Rust toolchain.

All changes are pure Rust code modifications. No external tools, services, or databases required.

## Test Impact Analysis

### Tests That Must Be Modified

| Test File | Usage | Changes Required |
|-----------|-------|------------------|
| `registry_edge_cases.rs` | `PluginGuard`, `resolve_guard()` | Replace with direct interface access |
| `hot_reload_safety.rs` | `PluginGuard`, Arc quiescence tests | Remove quiescence tests, update for callback model |
| `stress_concurrent_registry.rs` | `PluginGuard`, `resolve_guard()` | Replace guard pattern |
| `stress_quiescence_race.rs` | Quiescence timing tests | DELETE or rewrite for callback model |
| `stress_hot_reload.rs` | `PluginGuard`, quiescence | Remove quiescence-related tests |
| `integration_quiescence.rs` | Quiescence timeout test | DELETE - no quiescence in new model |
| `benches/registry_resolve.rs` | Benchmarks `resolve_guard()` | Update to benchmark new resolve method |

### Tests in plugin_registry.rs Internal Tests

| Test | Current Check | Changes Required |
|------|---------------|------------------|
| `register_and_find` | Generation comparison | Remove generation check |
| `stale_handle_detection` | Wrong generation test | DELETE or change to out-of-bounds test |
| `duplicate_provider_allowed` | Generation comparison | Remove generation from assertions |
| `collision_detection` | Handle structure | Update handle assertions |

### FFI Tests

| Test | Usage | Changes Required |
|------|-------|------------------|
| `handle_roundtrip_zero` | Packs/unpacks generation | Simplify to index only |
| `handle_roundtrip_max` | Packs/unpacks generation | Simplify to index only |

## Sources

### Primary (HIGH confidence)
- `crates/polyplug/src/registry/plugin_registry.rs` - Complete registry implementation (lines 1-609)
- `crates/polyplug/src/registry/mod.rs` - Module exports
- `crates/polyplug/src/reload.rs` - Quiescence wait implementation
- `crates/polyplug_abi/src/plugin/plugin_handle.rs` - Handle definition
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` - Interface definition
- `crates/polyplug/src/ffi.rs` - Handle packing/unpacking

### Secondary (MEDIUM confidence)
- Test files in `crates/polyplug/tests/` - Verified test patterns
- Benchmark in `crates/polyplug/benches/` - Verified benchmark patterns
- SDK files: `sdks/csharp/host/PluginGuard.cs`, `sdks/python/host/polyplug/runtime.py`

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All types located and understood
- Architecture: HIGH - Current pattern documented, target pattern clear
- Pitfalls: HIGH - Identified from code analysis and test coverage
- Test impact: HIGH - All affected tests identified

**Research date:** 2026-04-04
**Valid until:** 30 days - stable Rust patterns, no external dependencies