# Phase 04: Hot-Reload - Research

**Researched:** 2026-04-04
**Domain:** Hot-reload safety model, callback-based instance destruction, interface swap timing
**Confidence:** HIGH

## Summary

Phase 4 transforms the hot-reload model from Arc-based quiescence waiting to a callback-only model. The current implementation uses `wait_for_quiescence()` with `Arc::strong_count` to detect when all `PluginGuard` handles are dropped before swapping interfaces. Phase 3 replaced `PluginGuard` with RAII instance wrappers that call `create_instance`/`destroy_instance`. Phase 4 removes the quiescence wait entirely and relies on the host destroying all instances in the `ReloadPhase::Preparing` callback before the runtime swaps interfaces.

**Primary recommendation:** Remove `wait_for_quiescence()` from all loader reload implementations. Move interface swap to occur AFTER `Preparing` callback returns. Add warning callback if `Arc::strong_count > 1` after callback (indicates host didn't destroy instances - UB warning, not blocking).

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| HR-01 | Remove `wait_for_quiescence` with `Arc::strong_count` | See `crates/polyplug/src/reload.rs:75-109` - delete this function |
| HR-02 | Update hot-reload to use callback-only model | Modify loader reload methods to not wait for quiescence |
| HR-03 | `ReloadPhase::Preparing` fires before interface swap | Current: fires before loader.reload(). Change: fires, callback returns, then swap |
| HR-04 | Host destroys all instances in callback | Host responsibility - documented in HOT_RELOAD_DESIGN.md |
| HR-05 | Runtime swaps interfaces after callback returns | Swap happens after callback, not inside loader.reload() |
| HR-06 | Warning callback if instances remain (UB warning) | Check Arc::strong_count after callback, emit warning if > 1 |

</phase_requirements>

<user_constraints>

## User Constraints (from CONTEXT.md)

Not applicable - no CONTEXT.md file exists for this phase.

</user_constraints>

## Standard Stack

### Current Hot-Reload Components (to be modified)

| Component | Location | Purpose | Change Required |
|-----------|----------|---------|-----------------|
| `wait_for_quiescence()` | `reload.rs:75-109` | Arc::strong_count quiescence wait | DELETE entirely |
| `ReloadPhase` enum | `reload.rs:24-44` | Notification phases | Keep unchanged |
| `ReloadPhaseData` | `polyplug_abi/runtime/reload_phase_data.rs` | FFI-safe phase data | Keep unchanged |
| `get_interface_arc()` | `plugin_registry.rs:484-492` | Returns Arc for strong_count check | Keep for warning check (HR-06) |
| `NativeLoader::reload()` | `polyplug_native/src/loader.rs:173-310` | Native hot-reload | Remove wait_for_quiescence call, restructure |
| `Runtime::reload_bundle()` | `reload.rs:126-190` | Reload orchestration | Move interface swap timing |

### Supporting Types (from Phase 3)

| Type | Location | Purpose | Hot-Reload Role |
|------|----------|---------|-----------------|
| `GuestContractInstance` | `polyplug_abi/guest/guest_contract_instance.rs` | Opaque instance handle | Destroyed in callback |
| `GuestContractInterface` | `polyplug_abi/guest/guest_contract_interface.rs` | Contract with create/destroy | Swapped after callback |
| `GuestContractHandle` | `polyplug_abi/plugin/plugin_handle.rs` | Handle to interface | Used to find interfaces for swap |

## Architecture Patterns

### Current Pattern: Quiescence-Based Hot-Reload

```
┌─────────────────────────────────────────────────────────────────┐
│                     Current Hot-Reload Flow                      │
├─────────────────────────────────────────────────────────────────┤
│ Runtime.reload_bundle(path)                                     │
│   │                                                              │
│   ├─► Fire Preparing callback                                   │
│   │                                                              │
│   ├─► loader.reload(manifest, runtime)                          │
│   │     │                                                        │
│   │     ├─► Load new library                                     │
│   │     ├─► Call init on new library                             │
│   │     ├─► wait_for_quiescence(registry, bundle_id, timeout)  │
│   │     │     └─► Spin loop checking Arc::strong_count > 1     │
│   │     ├─► Drop old library (dlclose)                          │
│   │     └─► Store new library                                    │
│   │                                                              │
│   ├─► Fire Reloaded callback                                    │
│   │                                                              │
│   └─► Return Ok(())                                             │
└─────────────────────────────────────────────────────────────────┘

PROBLEM: wait_for_quiescence uses Arc::strong_count, but Phase 3 removed
PluginGuard. Instance wrappers don't hold Arc references to registry.
```

### Target Pattern: Callback-Based Hot-Reload

```
┌─────────────────────────────────────────────────────────────────┐
│                     Target Hot-Reload Flow (Phase 4)            │
├─────────────────────────────────────────────────────────────────┤
│ Runtime.reload_bundle(path)                                     │
│   │                                                              │
│   ├─► Fire Preparing callback                                   │
│   │     └─► HOST DESTROYS ALL INSTANCES HERE                    │
│   │                                                              │
│   ├─► Check Arc::strong_count (warning only, not blocking)      │
│   │     └─► If > 1: emit_warning("instances may still exist")   │
│   │                                                              │
│   ├─► loader.reload_backend(manifest, runtime)                  │
│   │     ├─► Load new library                                     │
│   │     ├─► Call init on new library (registers new interfaces) │
│   │     ├─► Drop old library                                     │
│   │     └─► Store new library                                    │
│   │                                                              │
│   ├─► Swap interfaces for bundle (registry.swap_interface)      │
│   │     └─► For each slot: get new interface from init, swap    │
│   │     └─► Atomic swap under RwLock write guard                │
│   │                                                              │
│   ├─► Fire Reloaded callback                                    │
│   │     └─► HOST CAN CREATE NEW INSTANCES NOW                   │
│   │                                                              │
│   └─► Return Ok(())                                             │
└─────────────────────────────────────────────────────────────────┘

KEY CHANGE: Interface swap happens AFTER init completes successfully,
not inside loader.reload(). Runtime calls swap_interface() for each slot.
```

### Interface Swap Pattern

```
┌─────────────────────────────────────────────────────────────────┐
│                     Interface Swap Implementation               │
├─────────────────────────────────────────────────────────────────┤
│ // In Runtime.reload_bundle() after loader.reload_backend():    │
│                                                                  │
│ // Step 1: Get slot indices for this bundle                     │
│ let slot_indices: Vec<u32> =                                    │
│     self.registry.find_slots_by_bundle(bundle_id);              │
│                                                                  │
│ // Step 2: For each slot, swap to the NEW interface             │
│ // The NEW interface was registered during init in step above   │
│ // Use find_by_contract to locate the newly registered interface│
│ for slot_idx in slot_indices {                                  │
│     // Get the contract_id this slot serves                     │
│     let contract_id = self.registry                             │
│         .get_slot_contract_id(slot_idx)?;                       │
│                                                                  │
│     // Find NEW interface by contract_id (registered during init)│
│     // Note: find_by_contract returns a handle with slot index  │
│     let new_interface = self.registry                           │
│         .get_interface_arc(                                     │
│             self.registry.find_by_contract(contract_id, 0)?     │
│                 .index                                          │
│         )?;                                                     │
│                                                                  │
│     // Atomic swap                                               │
│     self.registry.swap_interface(slot_idx, new_interface)?;     │
│ }                                                                │
│                                                                  │
│ // swap_interface is already implemented:                       │
│ pub fn swap_interface(&self, slot_index: u32,                   │
│                       new_interface: Arc<GuestContractInterface>)│
│                       -> Result<(), RegistryError>               │
└─────────────────────────────────────────────────────────────────┘
```

### Warning Callback Pattern (HR-06)

```
┌─────────────────────────────────────────────────────────────────┐
│                     Instance Leak Warning                        │
├─────────────────────────────────────────────────────────────────┤
│ // After Preparing callback returns, before loader.reload:      │
│                                                                  │
│ for slot_idx in slot_indices {                                  │
│     if let Some(arc) = registry.get_interface_arc(slot_idx) {   │
│         if Arc::strong_count(&arc) > 1 {                        │
│             // Someone still holds an Arc reference              │
│             // This is UB - but warn, don't block                │
│             runtime.emit_warning(                               │
│                 "Potential UB: Arc references still exist for   │
│                  bundle {} after Preparing callback.            │
│                  Host may not have destroyed all instances."    │
│             );                                                   │
│             break; // Only warn once per bundle                  │
│         }                                                        │
│     }                                                            │
│ }                                                                │
│                                                                  │
│ // PROCEED WITH RELOAD ANYWAY                                    │
│ // The host signed up for this responsibility                    │
└─────────────────────────────────────────────────────────────────┘
```

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Quiescence detection | New Arc tracking | Remove entirely - callback model | Phase 3 removed PluginGuard, Arc tracking no longer works for instances |
| Interface swap timing | Custom synchronization | Existing `swap_interface()` under RwLock | Already implemented in Phase 2, atomic swap |
| Instance lifecycle | Manual tracking in runtime | Host responsibility via callback | Simpler, cleaner separation of concerns |
| Warning emission | New callback type | Existing `emit_warning()` | Already has warning_cb or stderr fallback |

**Key insight:** The Arc::strong_count pattern no longer works because Phase 3's instance wrappers don't hold Arc references to the registry's interfaces. The registry stores `Arc<GuestContractInterface>` but instance wrappers only hold `*const GuestContractInterface` and `GuestContractInstance`. There's no Arc reference from wrapper to registry.

## Runtime State Inventory

> Phase involves hot-reload code changes - no external runtime state affected.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None - changes are to reload logic | None |
| Live service config | None - no external services | None |
| OS-registered state | None | None |
| Secrets/env vars | None | None |
| Build artifacts | None | None |

**All categories empty:** Changes are to reload flow and loader implementations only.

## Common Pitfalls

### Pitfall 1: Interface Swap Before Callback Returns

**What goes wrong:** Runtime swaps interfaces before host's Preparing callback destroys instances.

**Why it happens:** Current flow fires callback then immediately calls loader.reload() which does swap inside.

**How to avoid:** Move interface swap OUT of loader.reload(). Runtime should swap AFTER callback returns and AFTER init completes successfully.

**Warning signs:** Host sees stale instance pointers, crashes on dispatch after reload.

### Pitfall 2: Warning Check Blocks Reload

**What goes wrong:** Runtime blocks reload if Arc::strong_count > 1 after callback.

**Why it happens:** Treating warning as hard error like old quiescence timeout.

**How to avoid:** Warning is informational ONLY. Proceed with swap regardless. Host signed up for this responsibility.

**Warning signs:** Reload hangs waiting for instances that don't exist (instance wrappers don't hold Arc refs).

### Pitfall 3: Loader Still Calls wait_for_quiescence

**What goes wrong:** NativeLoader.reload() still calls wait_for_quiescence after Phase 4 changes.

**Why it happens:** Forgot to update loader implementations after removing function.

**How to avoid:** Delete wait_for_quiescence entirely. Update all loaders: NativeLoader, PythonLoader, JsLoader, LuaLoader, DotnetLoader.

**Warning signs:** Compilation error "wait_for_quiescence not found" or quiescence timeout errors.

### Pitfall 4: No Interface Registration from New Library

**What goes wrong:** New library loaded but interfaces not registered before swap.

**Why it happens:** Init must register interfaces BEFORE swap can get new interface pointers.

**How to avoid:** Init still happens inside loader.reload_backend(). Interfaces registered via host_vtable.register_plugin during init. Swap happens after, using registered interfaces via find_by_contract.

**Warning signs:** Swap fails with "InvalidHandle" or null interface.

### Pitfall 5: Tests Use Old Quiescence Pattern

**What goes wrong:** Tests call wait_for_quiescence or expect timeout behavior.

**Why it happens:** Test files weren't updated for callback model.

**How to avoid:** Delete/update integration_quiescence.rs, stress_quiescence_race.rs if they exist. Update other tests.

**Warning signs:** Test failures referencing quiescence.

### Pitfall 6: Swap Uses OLD Interface Instead of NEW

**What goes wrong:** Swap copies the old interface instead of the newly registered one.

**Why it happens:** After init registers new interfaces, need to find them via find_by_contract, not use the slot's current interface.

**How to avoid:** Use find_by_contract(contract_id, 0) to find the NEW interface handle, then get_interface_arc on that handle's index.

**Warning signs:** After reload, calls still use old code (old library not unmapped), crashes.

## Code Examples

### Current wait_for_quiescence (DELETE THIS)

```rust
// Source: crates/polyplug/src/reload.rs:75-109
// DELETE THIS ENTIRE FUNCTION

pub fn wait_for_quiescence(
    registry: &crate::registry::PluginRegistry,
    bundle_id: BundleId,
    timeout: Duration,
) -> Result<(), RuntimeError> {
    let slot_indices: Vec<u32> = registry.find_slots_by_bundle(bundle_id);

    let start: Instant = Instant::now();
    loop {
        let mut all_quiescent: bool = true;

        for &slot_idx in &slot_indices {
            if let Some(arc) = registry.get_interface_arc(slot_idx) {
                // Count == 1 means only registry holds it (no in-flight calls)
                if Arc::strong_count(&arc) > 1 {
                    all_quiescent = false;
                    break;
                }
            }
        }

        if all_quiescent {
            return Ok(());
        }

        if start.elapsed() > timeout {
            return Err(RuntimeError::QuiescenceTimeout {
                bundle: format!("bundle_id={}", bundle_id.id()),
            });
        }

        std::thread::sleep(Duration::from_millis(1));
        spin_loop();
    }
}
```

### Current NativeLoader.reload (Needs Refactoring)

```rust
// Source: crates/polyplug_native/src/loader.rs:173-310
// Lines 288-293 call wait_for_quiescence - REMOVE

fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
    // ... load new library, init ...

    // ─── REMOVE THIS BLOCK ───────────────────────────────────────────────────
    // wait_for_quiescence(
    //     runtime.registry(),
    //     bundle_id,
    //     std::time::Duration::from_secs(5),
    // )?;
    // ──────────────────────────────────────────────────────────────────────────

    // ─── Step 9: Remove and DROP old library ─────────────────────────────────────
    if let Some(old_library) = self.libraries.lock().unwrap().remove(&bundle_id) {
        drop(old_library); // dlclose() - unmaps code pages
    }

    // ─── Step 10: Store new library ───────────────────────────────────────────────────
    self.libraries.lock().unwrap().insert(bundle_id, new_library);

    Ok(())
}
```

### Target Runtime.reload_bundle Flow (HR-05 Implementation)

```rust
// Target implementation for reload.rs - Runtime.reload_bundle()
// After Plan 01 removes wait_for_quiescence, this implements the new flow

pub fn reload_bundle(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
    if !self.config().hot_reload_enabled {
        return Err(RuntimeError::HotReloadDisabled);
    }

    // Parse manifest
    let manifest: ManifestData = crate::loader::parse_manifest(bundle_dir)?;

    let bundle_id: BundleId = BundleId::new(&manifest.name);

    // ─── Step 1: Fire Preparing callback ──────────────────────────────────────────────
    // HOST DESTROYS ALL INSTANCES IN THIS CALLBACK
    if let Some(cb) = self.on_reload_cb() {
        cb(ReloadPhase::Preparing {
            bundle_id,
            bundle_name: manifest.name.clone(),
            retry_count: 0,
        });
    }

    // ─── Step 2: Warning check (informational only) ──────────────────────────────────────
    // Get slots BEFORE loader.reload_backend() (before new registration)
    let slot_indices: Vec<u32> = self.registry.find_slots_by_bundle(bundle_id);
    for slot_idx in &slot_indices {
        if let Some(arc) = self.registry.get_interface_arc(*slot_idx) {
            if Arc::strong_count(&arc) > 1 {
                self.emit_warning(&format!(
                    "Potential UB: Arc refs still exist for bundle '{}' after Preparing callback. \
                     Host may not have destroyed all instances. Proceeding with reload anyway.",
                    manifest.name
                ));
                break; // Only emit once per bundle
            }
        }
    }

    // ─── Step 3: Load new library and init (registers new interfaces) ──────────────────────
    let loader: &dyn BundleLoader = self.loaders.get(&manifest.runtime)...;
    let result: Result<(), RuntimeError> = loader.reload(&manifest, self);

    // ─── Step 4: Handle init failure ──────────────────────────────────────────────────────
    if let Err(e) = result {
        // Fire Failed callback - DO NOT SWAP if init failed
        if let Some(cb) = self.on_reload_cb() {
            cb(ReloadPhase::Failed {
                bundle_id,
                bundle_name: manifest.name.clone(),
                reason: e.to_string(),
            });
        }
        return Err(e);
    }

    // ─── Step 5: Swap interfaces (atomic under RwLock) ──────────────────────────────────────
    // New interfaces were registered during init in step 3
    // For each slot, find NEW interface by contract_id and swap
    for slot_idx in &slot_indices {
        // Get contract_id for this slot
        let contract_id: GuestContractId = self.registry
            .get_slot_contract_id(*slot_idx)
            .ok_or_else(|| RuntimeError::Registry(RegistryError::InvalidHandle {
                index: *slot_idx
            }))?;

        // Find NEW interface (registered during init) by contract_id
        // find_by_contract returns handle with slot index of NEW registration
        let new_handle: GuestContractHandle = self.registry
            .find_by_contract(contract_id, 0)
            .map_err(|e| RuntimeError::Registry(e))?;

        // Get Arc to NEW interface
        let new_interface: Arc<GuestContractInterface> = self.registry
            .get_interface_arc(new_handle.index)
            .ok_or_else(|| RuntimeError::Registry(RegistryError::InvalidHandle {
                index: new_handle.index
            }))?;

        // Atomic swap
        self.registry.swap_interface(*slot_idx, new_interface)?;
    }

    // ─── Step 6: Fire Reloaded callback ──────────────────────────────────────────────────────
    if let Some(cb) = self.on_reload_cb() {
        cb(ReloadPhase::Reloaded {
            bundle_id,
            bundle_name: manifest.name.clone(),
        });
    }

    Ok(())
}
```

### Registry Methods Used

```rust
// Source: crates/polyplug/src/registry/plugin_registry.rs

/// Already implemented in Phase 2 - keep unchanged
pub fn swap_interface(
    &self,
    slot_index: u32,
    new_interface: Arc<GuestContractInterface>,
) -> Result<(), RegistryError>

/// Already implemented - keep for warning check
pub fn find_slots_by_bundle(&self, bundle_id: BundleId) -> Vec<u32>

/// Already implemented - keep for warning check and swap logic
pub(crate) fn get_interface_arc(&self, slot_index: u32) -> Option<Arc<GuestContractInterface>>

/// Already implemented - keep for finding contract_id during swap
pub(crate) fn get_slot_contract_id(&self, slot_index: u32) -> Option<GuestContractId>

/// Already implemented - keep for finding new interface after init
pub fn find_by_contract(&self, contract_id: GuestContractId, min_version: u32)
    -> Result<GuestContractHandle, RegistryError>
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Arc quiescence tracking | Callback-based destruction | Phase 4 (this phase) | Removes wait, simpler flow |
| wait_for_quiescence() | Delete entirely | Phase 4 | No timeout errors |
| Interface swap in loader | Interface swap in Runtime | Phase 4 | Centralized swap logic |
| Blocking on strong_count | Warning only | Phase 4 | Non-blocking, host responsibility |
| PluginGuard RAII | Instance wrapper RAII | Phase 3 | Different ownership pattern |

**Deprecated/outdated:**
- `wait_for_quiescence()`: Deleted in this phase
- `QuiescenceTimeout` error: No longer raised
- Loader doing interface swap: Runtime does swap now

## Dependencies on Phase 3

Phase 4 depends on Phase 3 instance model completion:

| Phase 3 Change | Impact on Phase 4 |
|----------------|-------------------|
| Instance wrappers with create/destroy | Host must call destroy_instance in callback |
| Singleton host contracts | Singleton instances destroyed/recreated |
| RAII Drop impl calls destroy_instance | Drop wrapper = destroy instance |
| Removed PluginGuard | Arc::strong_count no longer tracks instances |

**Critical:** Phase 3's instance wrappers store `*const GuestContractInterface` (raw pointer) and `GuestContractInstance`, NOT `Arc<GuestContractInterface>`. This means Arc::strong_count will always be 1 for the registry's stored interface. The warning check (HR-06) will almost always show count=1, making it informational only.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Instance wrappers don't hold Arc references | Architecture Patterns | Warning check would be wrong, need different detection |
| A2 | Init registers interfaces before swap | Target Flow | Swap would fail to find new interfaces |
| A3 | All loaders need reload_backend refactoring | Loader Changes | Some loaders may have different patterns |
| A4 | Contract_id unchanged across reload | Swap Logic | Would need version matching |

**Verification needed:**
- A1: Check generated instance wrapper code from Phase 3 - confirmed stores raw pointer
- A2: Check loader init flow - confirmed register happens in init
- A3: Check all loader reload implementations
- A4: Check if reload preserves contract_id (version may change)

## Open Questions

1. **How does swap find the NEW interface after init? (RESOLVED)**
   - What we know: Init registers interfaces via host_vtable.register_plugin
   - What was unclear: How to get the NEW interface pointer for swap
   - Resolution: Use `find_by_contract(contract_id, 0)` to locate the newly registered interface. This returns a `GuestContractHandle` whose `index` field points to the NEW registration slot. Then call `get_interface_arc(handle.index)` to get the Arc for swap.
   - Implementation: In `Runtime.reload_bundle()`, after `loader.reload()` succeeds, for each old slot: get its `contract_id`, find new handle, get new interface Arc, call `swap_interface()`.

2. **Does the slot contract_id change across reload? (RESOLVED)**
   - What we know: Same contract, possibly new version
   - What was unclear: Whether contract_id hash is stable across reload
   - Resolution: `contract_id` (GuestContractId) is derived from the contract name string hash, which is stable across reloads. The version field in the interface may change, but the contract_id hash remains the same. `find_by_contract(contract_id, 0)` with min_version=0 will find any version.
   - Implementation: Use the old slot's `contract_id` to find the new registration - same hash, works correctly.

3. **What if init fails during reload? (RESOLVED)**
   - What we know: Current flow fires Failed callback
   - What was unclear: Should interfaces be swapped before or after init
   - Resolution: DO NOT SWAP if init fails. The flow is:
     1. Fire Preparing callback
     2. Warning check (optional)
     3. Call loader.reload() which does load+init
     4. If init FAILS: Fire Failed callback, return error, NO SWAP
     5. If init SUCCEEDS: Swap interfaces, fire Reloaded callback
   - Implementation: In `Runtime.reload_bundle()`, the swap logic is after `loader.reload()` returns `Ok(())`. If `Err(e)`, fire Failed callback and return error without touching interfaces.

4. **Should warning callback fire for each slot or once per bundle? (RESOLVED)**
   - What we know: Multiple slots per bundle possible
   - What was unclear: Granularity of warning
   - Resolution: One warning per bundle. Use `break` after first slot with strong_count > 1 to emit single message.
   - Implementation: Current task design uses `break` after first detection, correct behavior.

## Environment Availability

> SKIPPED - Phase has no external dependencies beyond Rust toolchain.

All changes are pure Rust code modifications. No external tools, services, or databases required.

## Validation Architecture

> SKIPPED - workflow.nyquist_validation is false in config.json (from Phase 3 research).

Existing test infrastructure:
- `crates/polyplug/tests/stress_hot_reload.rs` - stress tests for hot-reload (needs update)
- `tests/integration/tests/integration_reload.rs` - integration tests (needs update)
- `crates/polyplug/tests/hot_reload_safety.rs` - safety tests (needs update)

Wave 0 Gaps:
- Tests using `wait_for_quiescence` must be updated
- Tests expecting `QuiescenceTimeout` must be removed/updated
- Tests for warning callback behavior need to be added

## Security Domain

> SKIPPED - security_enforcement not set, this phase is about internal architecture.

Security-relevant considerations:
- Hot-reload safety: Host must destroy instances to avoid use-after-free
- Warning callback: Informs host of potential UB, doesn't prevent
- Interface swap atomicity: RwLock ensures no torn reads

## Sources

### Primary (HIGH confidence)
- `crates/polyplug/src/reload.rs` - Hot-reload implementation with wait_for_quiescence
- `crates/polyplug/src/registry/plugin_registry.rs` - Registry with swap_interface, get_interface_arc
- `crates/polyplug_native/src/loader.rs:173-310` - NativeLoader reload implementation
- `crates/polyplug_abi/src/runtime/reload_phase_data.rs` - FFI-safe ReloadPhaseData
- `docs/HOT_RELOAD_DESIGN.md` - Design documentation (needs update after Phase 4)
- `.planning/phases/03-instance-model/03-RESEARCH.md` - Phase 3 context
- `.planning/phases/03-instance-model/03-04-SUMMARY.md` - Instance wrapper codegen

### Secondary (MEDIUM confidence)
- `.planning/phases/02-registry/02-02-PLAN.md` - Registry changes (ArcSwap removal)
- `crates/polyplug/tests/stress_hot_reload.rs` - Hot-reload stress tests
- `tests/integration/tests/integration_reload.rs` - Integration tests

### Tertiary (LOW confidence)
- Other loader implementations (Python, JS, Lua, .NET) - need to check reload patterns

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All files verified, implementation clear
- Architecture: HIGH - Target pattern derived from requirements
- Pitfalls: HIGH - Identified from code analysis and requirements
- Dependencies: HIGH - Phase 3 research read, Phase 2 plans understood

**Research date:** 2026-04-04
**Valid until:** 30 days - depends on Phase 3 completion and Phase 2 registry state