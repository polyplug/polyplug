# Roadmap: polyplug v1.1 Architecture Refactor

**Milestone:** v1.1 Architecture Refactor
**Created:** 2026-04-03
**Granularity:** Coarse (6 phases)
**Coverage:** 48/48 requirements mapped

## Core Value

The core runtime is loader-agnostic — the `polyplug` crate knows about the `BundleLoader` trait and `PluginRegistry`, but NOT about `libloading`, `dlopen`, or any specific loader implementation.

## Phases

- [x] **Phase 1: ABI Types** - Foundation types moved to polyplug_abi with renamed interfaces
- [x] **Phase 2: Registry** - Simplified registry with direct interface storage
- [x] **Phase 3: Instance Model** - Factory-based instance lifecycle with codegen support
- [x] **Phase 4: Hot-Reload** - Callback-based reload with instance safety contract
- [x] **Phase 5: SDK Updates** - All five SDKs updated to use polyplug_abi types
- [x] **Phase 6: Cleanup** - Naming consistency and documentation updates
- [x] **Phase 7: Typed Handles** - Replace opaque c_void pointers with typed handles
- [x] **Phase 8: Retroactive Verification** - VERIFICATION.md files for orphaned requirements
- [x] **Phase 9: Codegen Test Cleanup** - Fix smoke.rs vtable→interface test mismatches
- [x] **Phase 10: SDK Cleanup Completion** - Remaining SDK naming and cleanup items (completed 2026-04-06)

## Phase Details

### Phase 1: ABI Types
**Goal:** All FFI types consolidated in polyplug_abi with renamed interfaces, workspace compiles
**Depends on:** Nothing (foundation phase)
**Requirements:** ABI-01, ABI-02, ABI-03, ABI-04, ABI-05, ABI-06, ABI-07, ABI-08, ABI-09, ABI-10, ABI-11, ABI-12, ABI-13, ABI-14, RTABI-01, RTABI-02, RTABI-03, RTABI-04, RTABI-05
**Success Criteria** (what must be TRUE):
1. GuestContractInterface and HostContractInterface structs defined in polyplug_abi with create_instance/destroy_instance fields
2. RuntimeConfig, Compatibility, and ReloadPhaseData moved from polyplug crate to polyplug_abi
3. RuntimeAbi (renamed from HostVTable) contains all ABI functions including call_method
4. All ID types renamed: PluginContractId -> GuestContractId throughout codebase
5. All public ABI structs are #[repr(C)] and compile successfully
6. Workspace compiles (cargo build --workspace)
**Plans:** 12 plans (4 main + 8 gap closure)

Plans:
- [x] 01-01-PLAN.md — Rename PluginContractId to GuestContractId in polyplug_utils
- [x] 01-02-PLAN.md — Rename/extend core ABI types: PluginInterface->GuestContractInterface, HostVTable->RuntimeAbi, add instance handles
- [x] 01-03-PLAN.md — Move RuntimeConfig, Compatibility to polyplug_abi; create ReloadPhaseData FFI struct
- [x] 01-04-PLAN.md — Integration: update all imports across workspace, verify compilation
- [x] 01-05-PLAN.md — Gap closure: Export AbiErrorCode and helper functions from polyplug_abi root
- [x] 01-06-PLAN.md — Gap closure: Fix deprecated PluginContractId usage in compatibility files
- [x] 01-07-PLAN.md — Gap closure: Fix ffi.rs bundle_id type mismatch, verify polyplug compiles
- [x] 01-08-PLAN.md — Gap closure: Remove ABI_* constant imports from SDK guest library
- [x] 01-09-PLAN.md — Gap closure: Fix fixture AbiError.code usage, verify workspace compiles
- [x] 01-10-PLAN.md — Gap closure: Fix plugin_interface.rs PluginContractId usage
- [x] 01-11-PLAN.md — Gap closure: Fix compatibility/mod.rs test PluginContractId usage
- [x] 01-12-PLAN.md — Gap closure: Add serde traits to GuestContractId and BundleId

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
**Plans:** 3 plans

Plans:
- [ ] 02-01-PLAN.md — Remove VTableSlot wrapper and PluginGuard, store interface directly
- [ ] 02-02-PLAN.md — Remove ArcSwap pattern, update tests, remove quiescence tests
- [ ] 02-03-PLAN.md — Remove generation counter from PluginHandle, simplify error handling

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
**Plans:** 5 plans

Plans:
- [ ] 03-01-PLAN.md — Parser singleton field support and GuestContractInterface naming verification
- [ ] 03-02-PLAN.md — Guest vtable create/destroy_instance stubs and dispatch signature updates
- [ ] 03-03-PLAN.md — Runtime get_host_contract and call_method implementation
- [ ] 03-04-PLAN.md — Host instance wrapper codegen (Rust generator)
- [ ] 03-05-PLAN.md — Host contract factory codegen (all generators)

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
**Plans:** 3 plans

Plans:
- [x] 04-01-PLAN.md — Remove wait_for_quiescence from reload.rs, QuiescenceTimeout from RuntimeError, update NativeLoader
- [x] 04-02-PLAN.md — Add Arc::strong_count warning check after Preparing callback, update documentation
- [x] 04-03-PLAN.md — Update hot-reload tests and documentation for callback-based model

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
**Plans:** 8 plans (6 main + 2 gap closure)

Plans:
- [x] 05-01-PLAN.md — Rust SDK: verify imports from polyplug_abi, no duplicate types
- [x] 05-02-PLAN.md — Python SDK: remove runtime_config.py, update RuntimeConfigC, remove PluginGuard
- [x] 05-03-PLAN.md — C# SDK: remove HostRuntimeConfig.cs, update RuntimeConfigC, remove PluginGuard.cs
- [x] 05-04-PLAN.md — Lua SDK: remove runtime_config.lua, update ffi.cdef RuntimeConfigC, remove Guard
- [x] 05-05-PLAN.md — JS SDK: remove runtime_config.js, update config buffer, remove Guard class
- [x] 05-06-PLAN.md — Codegen: verify instance wrappers generated for all languages
- [x] 05-07-PLAN.md — Gap closure: Update C++ SDK (remove PluginGuard, add RuntimeConfig 24 bytes)
- [x] 05-08-PLAN.md — Gap closure: Rename RuntimeConfigC to RuntimeConfig in all SDKs

### Phase 6: Cleanup
**Goal:** Consistent Guest/Host naming throughout with no vtable terminology
**Depends on:** Phase 5
**Requirements:** CLN-01, CLN-02, CLN-03, CLN-04
**Success Criteria** (what must be TRUE):
1. No "vtable" naming remains in codebase (search: vtable, VTable, VTABLE)
2. No *C suffix types in FFI (all types from polyplug_abi are canonical)
3. Documentation uses Guest Contract / Host Contract terminology consistently
4. All tests pass with new instance model and naming
**Plans:** 9 plans (4 main + 5 gap closure)

Plans:
- [ ] 06-01-PLAN.md — Remove all "vtable" naming from codebase
- [ ] 06-02-PLAN.md — Verify no *C suffix types in FFI (covered by 05-08)
- [ ] 06-03-PLAN.md — Update documentation to use Guest/Host terminology
- [ ] 06-04-PLAN.md — Update tests to use new instance model and naming
- [ ] 06-05-PLAN.md — Gap closure: Rename generator file/function names (vtable_factories -> interface_factories)
- [ ] 06-06-PLAN.md — Gap closure: Fix generator ABI structure templates (version, error codes, register_contract)
- [ ] 06-07-PLAN.md — Gap closure: Update SDK host files to use HostContractInterface terminology
- [ ] 06-08-PLAN.md — Gap closure: Rename test file and update all test imports
- [ ] 06-09-PLAN.md — Gap closure: Regenerate example code and final verification

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
**Plans:** 4 plans

Plans:
- [x] 07-01-PLAN.md — Create RuntimeContext and VmLoaderData opaque handle structs following GuestContractInstance pattern
- [x] 07-02-PLAN.md — Update RuntimeAbi function signatures and host callbacks to use RuntimeContext
- [x] 07-03-PLAN.md — Update GuestContractInterface, HostContractInterface, VmDispatch to use typed handles
- [x] 07-04-PLAN.md — Update codegen and loaders for typed handles, final verification

### Phase 8: Retroactive Verification
**Goal:** Create VERIFICATION.md files for phases 02, 03, 04, 07 to close orphaned requirement gaps
**Depends on:** Phase 7
**Requirements:** REG-01, REG-02, REG-03, REG-04, REG-05, REG-06, INST-01, INST-02, INST-03, INST-04, INST-05, INST-06, HC-02, HC-03, HC-04, CG-02, CG-03, CG-04, CG-05, HR-01, HR-02, HR-03, HR-04, HR-05, HR-06, TH-01, TH-02, TH-03, TH-04, TH-05, TH-06, TH-07, TH-08
**Gap Closure:** Closes 35 orphaned requirements from audit
**Success Criteria** (what must be TRUE):
1. Phase 02 VERIFICATION.md exists with REG-01 through REG-06 verified
2. Phase 03 VERIFICATION.md exists with INST, HC, CG requirements verified
3. Phase 04 VERIFICATION.md exists with HR-01 through HR-06 verified
4. Phase 07 VERIFICATION.md exists with TH-01 through TH-08 verified
5. All 35 orphaned requirements have VERIFICATION.md evidence
**Plans:** 4 plans

Plans:
- [x] 08-01-PLAN.md — Create Phase 02 VERIFICATION.md for REG-01 through REG-06
- [x] 08-02-PLAN.md — Create Phase 03 VERIFICATION.md for INST, HC, CG requirements
- [x] 08-03-PLAN.md — Create Phase 04 VERIFICATION.md for HR-01 through HR-06
- [x] 08-04-PLAN.md — Create Phase 07 VERIFICATION.md for TH-01 through TH-08

### Phase 9: Codegen Test Cleanup
**Goal:** Fix smoke.rs test expectations for vtable→interface naming transition
**Depends on:** Phase 8
**Requirements:** CLN-01, CLN-04, SDK-05
**Gap Closure:** Closes test/integration/flow gaps from audit
**Success Criteria** (what must be TRUE):
1. smoke.rs references interfaces.* not vtables.*
2. Handwritten lib.rs imports guest::interfaces
3. C++ codegen E2E flow passes
4. Rust codegen E2E flow passes
**Plans:** 3 plans

Plans:
- [x] 09-01-PLAN.md — Update smoke.rs lib.rs template and C++ expected files to use interfaces naming
- [x] 09-02-PLAN.md — Update integration_codegen_cpp.rs expected files and variable names
- [x] 09-03-PLAN.md — Delete stale vtables.* files from examples (C++, JS, host factories)

### Phase 10: SDK Cleanup Completion
**Goal:** Complete remaining SDK naming and cleanup items
**Depends on:** Phase 9
**Requirements:** SDK-02, SDK-03, SDK-04, SDK-06, CLN-02
**Gap Closure:** Closes partial/unsatisfied SDK requirements from audit
**Success Criteria** (what must be TRUE):
1. RuntimeConfigC renamed to RuntimeConfig in Python, C#, Lua, C++ SDKs
2. C++ SDK PluginGuard removed
3. C++ guest.hpp uses RuntimeAbi not HostVTable
4. All SDK naming consistent with polyplug_abi types
**Plans:** 2/2 plans complete

Plans:
- [x] 10-01-PLAN.md — Create VERIFICATION.md for SDK-02, SDK-03, SDK-04, SDK-06, CLN-02 (already satisfied)
- [x] 10-02-PLAN.md — Fix HostVTable → RuntimeAbi naming in C++ guest.hpp and C# AbiSizeTests.cs

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. ABI Types | 12/12 | Complete | 2026-04-04 |
| 2. Registry | 3/3 | Complete | 2026-04-04 |
| 3. Instance Model | 5/5 | Complete | 2026-04-04 |
| 4. Hot-Reload | 3/3 | Complete | 2026-04-04 |
| 5. SDK Updates | 8/8 | Complete | 2026-04-04 |
| 6. Cleanup | 13/13 | Complete | 2026-04-05 |
| 7. Typed Handles | 4/4 | Complete | 2026-04-05 |
| 8. Retroactive Verification | 4/4 | Complete | 2026-04-06 |
| 9. Codegen Test Cleanup | 3/3 | Complete | 2026-04-06 |
| 10. SDK Cleanup Completion | 2/2 | Complete    | 2026-04-06 |

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
    |
    v
Phase 8 (Retroactive Verification)
    |
    v
Phase 9 (Codegen Test Cleanup)
    |
    v
Phase 10 (SDK Cleanup Completion)
```

### Phase 11: Guest Calling Convention & Missing Introspection

**Goal:** Rename `RuntimeAbi` → `HostInterface`, create `RuntimeInterface` for symmetric API, delete `RuntimeContext`/`HostContext` wrappers, rename `call_method` → `call_guest_method`, implement guest-to-guest calls, add introspection ABIs, create `Array<T>`/`Vector<T>` types, update all SDKs and codegen.
**Requirements**: 
- Rename `RuntimeAbi` → `HostInterface` (consistent naming)
- Create `RuntimeInterface` struct returned from `polyplug_runtime_create()`
- Delete `RuntimeContext` and `HostContext` wrapper types
- Functions take `self: *const Interface` instead of `rt_ctx`
- Rename `call_method` → `call_guest_method` in HostInterface
- Add `contract_id` field to `GuestContractInstance` for dispatch
- Add `list_bundles` and `get_dependencies` introspection ABIs
- Create `Array<T>` and `Vector<T>` ABI types
- Update codegen to support Array/Vector in contract signatures
- Update all 5 SDKs with new interfaces and types
**Depends on:** Phase 10
**Plans:** 0 plans

Plans:
- [ ] TBD (run /gsd-plan-phase 11 to break down)

---
*Roadmap created: 2026-04-03*
*Phase 1 plans added: 2026-04-03*
*Gap closure plans added: 2026-04-03*
*Plans split per checker feedback: 2026-04-03*
*Additional gap closure plans added: 2026-04-04*
*Phase 2 plans added: 2026-04-04*
*Phase 3 plans added: 2026-04-04*
*Phase 4 plans added: 2026-04-04*
*Phase 5 gap closure plans added: 2026-04-04*
*Phase 6 gap closure plans added: 2026-04-04*
*Phase 7 plans added: 2026-04-05*
*Phase 8 plans added: 2026-04-06*
*Phase 10 plans added: 2026-04-06*