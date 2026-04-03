# Roadmap: polyplug v1.1 Architecture Refactor

**Milestone:** v1.1 Architecture Refactor
**Created:** 2026-04-03
**Granularity:** Coarse (6 phases)
**Coverage:** 48/48 requirements mapped

## Core Value

The core runtime is loader-agnostic — the `polyplug` crate knows about the `BundleLoader` trait and `PluginRegistry`, but NOT about `libloading`, `dlopen`, or any specific loader implementation.

## Phases

- [ ] **Phase 1: ABI Types** - Foundation types moved to polyplug_abi with renamed interfaces
- [ ] **Phase 2: Registry** - Simplified registry with direct interface storage
- [ ] **Phase 3: Instance Model** - Factory-based instance lifecycle with codegen support
- [ ] **Phase 4: Hot-Reload** - Callback-based reload with instance safety contract
- [ ] **Phase 5: SDK Updates** - All five SDKs updated to use polyplug_abi types
- [ ] **Phase 6: Cleanup** - Naming consistency and documentation updates

## Phase Details

### Phase 1: ABI Types
**Goal:** All FFI types consolidated in polyplug_abi with renamed interfaces
**Depends on:** Nothing (foundation phase)
**Requirements:** ABI-01, ABI-02, ABI-03, ABI-04, ABI-05, ABI-06, ABI-07, ABI-08, ABI-09, ABI-10, ABI-11, ABI-12, ABI-13, ABI-14, RTABI-01, RTABI-02, RTABI-03, RTABI-04, RTABI-05
**Success Criteria** (what must be TRUE):
1. GuestContractInterface and HostContractInterface structs defined in polyplug_abi with create_instance/destroy_instance fields
2. RuntimeConfig, ReloadPhase, and RuntimeCreateOptions moved from polyplug crate to polyplug_abi
3. RuntimeAbi (renamed from HostVTable) contains all ABI functions including call_method
4. All ID types renamed: PluginContractId -> GuestContractId throughout codebase
5. All public ABI structs are #[repr(C)] and compile successfully
**Plans:** 4 plans

Plans:
- [x] 01-01-PLAN.md — Rename PluginContractId to GuestContractId in polyplug_utils
- [x] 01-02-PLAN.md — Rename/extend core ABI types: PluginInterface->GuestContractInterface, HostVTable->RuntimeAbi, add instance handles
- [x] 01-03-PLAN.md — Move RuntimeConfig, Compatibility to polyplug_abi; create ReloadPhaseData FFI struct
- [ ] 01-04-PLAN.md — Integration: update all imports across workspace, verify compilation

### Phase 2: Registry
**Goal:** Simplified registry stores GuestContractInterface directly without wrappers
**Depends on:** Phase 1
**Requirements:** REG-01, REG-02, REG-03, REG-04, REG-05, REG-06
**Success Criteria** (what must be TRUE):
1. RegistrySlot stores Arc<GuestContractInterface> directly (no VTableSlot wrapper)
2. PluginGuard removed from codebase (replaced by instance model in Phase 3)
3. ContractHandle has only index field (no generation counter)
4. find_contract returns ContractHandle without generation validation
5. Registry compiles and all existing tests pass
**Plans:** TBD

### Phase 3: Instance Model
**Goal:** Host creates and owns plugin instances via factory pattern with generated RAII wrappers
**Depends on:** Phase 2
**Requirements:** INST-01, INST-02, INST-03, INST-04, INST-05, INST-06, HC-01, HC-02, HC-03, HC-04, CG-01, CG-02, CG-03, CG-04, CG-05, CG-06
**Success Criteria** (what must be TRUE):
1. Generated *Instance wrappers call create_instance on construction and destroy_instance on drop
2. Instance pointer passed as first argument to all dispatch calls (native and VM)
3. HostContractInterface supports singleton field; get_host_contract returns same instance for singletons
4. Codegen generates instance wrappers for guest contracts and host contract implementations
5. Cross-dispatch call_method works for plugin-plugin calls across dispatch types
**Plans:** TBD

### Phase 4: Hot-Reload
**Goal:** Hot-reload uses callback-based model where host destroys instances before swap
**Depends on:** Phase 3
**Requirements:** HR-01, HR-02, HR-03, HR-04, HR-05, HR-06
**Success Criteria** (what must be TRUE):
1. ReloadPhase::Preparing callback fires before interface swap, giving host chance to destroy instances
2. Runtime atomically swaps interfaces after callback returns
3. ReloadPhase::Reloaded callback fires after swap for host to create new instances
4. Warning callback fires if any instances remain after Preparing callback (UB warning)
5. Arc::strong_count quiescence wait removed from hot-reload code
**Plans:** TBD

### Phase 5: SDK Updates
**Goal:** All five SDKs use types from polyplug_abi without duplicates
**Depends on:** Phase 4
**Requirements:** SDK-01, SDK-02, SDK-03, SDK-04, SDK-05, SDK-06, SDK-07
**Success Criteria** (what must be TRUE):
1. Rust host SDK imports RuntimeConfig, ReloadPhase from polyplug_abi (no duplicates)
2. Python SDK removes RuntimeConfigC duplicate, uses abi module types
3. C# SDK removes RuntimeConfigC duplicate, uses Abi namespace types
4. Lua SDK uses FFI cdef types from polyplug_abi
5. JS SDK uses TypeScript interfaces from polyplug_abi
6. PluginGuard removed from all SDKs (replaced by instance wrappers)
7. All SDKs generate instance-based wrappers via codegen
**Plans:** TBD

### Phase 6: Cleanup
**Goal:** Consistent Guest/Host naming throughout with no vtable terminology
**Depends on:** Phase 5
**Requirements:** CLN-01, CLN-02, CLN-03, CLN-04
**Success Criteria** (what must be TRUE):
1. No "vtable" naming remains in codebase (search: vtable, VTable, VTABLE)
2. No *C suffix types in FFI (all types from polyplug_abi are canonical)
3. Documentation uses Guest Contract / Host Contract terminology consistently
4. All tests pass with new instance model and naming
**Plans:** TBD

### Phase 7: Typed Handles
**Goal:** Replace all `*mut c_void` and `*const c_void` with meaningful typed handles
**Depends on:** Phase 6
**Requirements:** TH-01, TH-02, TH-03, TH-04, TH-05, TH-06, TH-07, TH-08
**Success Criteria** (what must be TRUE):
1. `RuntimeContext` typed handle replaces `*mut c_void` for rt_ctx parameter
2. `VmLoaderData` typed handle replaces `*mut c_void` for VM loader_data
3. All RuntimeAbi functions use `RuntimeContext` instead of bare pointer
4. All opaque handles are `#[repr(C)]` structs with single `data` field
5. No bare `c_void` pointers in public ABI (except in opaque handle internals)
**Plans:** TBD

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. ABI Types | 0/4 | Planned | - |
| 2. Registry | 0/0 | Not started | - |
| 3. Instance Model | 0/0 | Not started | - |
| 4. Hot-Reload | 0/0 | Not started | - |
| 5. SDK Updates | 0/0 | Not started | - |
| 6. Cleanup | 0/0 | Not started | - |
| 7. Typed Handles | 0/0 | Not started | - |

## Dependencies

```
Phase 1 (ABI Types)
    |
    v
Phase 2 (Registry)
    |
    v
Phase 3 (Instance Model)
    |
    v
Phase 4 (Hot-Reload)
    |
    v
Phase 5 (SDK Updates)
    |
    v
Phase 6 (Cleanup)
    |
    v
Phase 7 (Typed Handles)
```

---
*Roadmap created: 2026-04-03*
*Phase 1 plans added: 2026-04-03*