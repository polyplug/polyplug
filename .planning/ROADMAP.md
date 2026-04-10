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
3. RuntimeAbi (renamed from HostInterface) contains all ABI functions including call_method
4. All ID types renamed: PluginContractId -> GuestContractId throughout codebase
5. All public ABI structs are #[repr(C)] and compile successfully
6. Workspace compiles (cargo build --workspace)
**Plans:** 12 plans (4 main + 8 gap closure)

Plans:
- [x] 01-01-PLAN.md — Rename PluginContractId to GuestContractId in polyplug_utils
- [x] 01-02-PLAN.md — Rename/extend core ABI types: GuestContractInterface->GuestContractInterface, HostInterface->RuntimeAbi, add instance handles
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
- [ ] 02-03-PLAN.md — Remove generation counter from GuestContractHandle, simplify error handling

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
3. C++ guest.hpp uses RuntimeAbi not HostInterface
4. All SDK naming consistent with polyplug_abi types
**Plans:** 2/2 plans complete

Plans:
- [x] 10-01-PLAN.md — Create VERIFICATION.md for SDK-02, SDK-03, SDK-04, SDK-06, CLN-02 (already satisfied)
- [x] 10-02-PLAN.md — Fix HostInterface → RuntimeAbi naming in C++ guest.hpp and C# AbiSizeTests.cs

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
| 11. Guest Calling Convention | 10/10 | Complete   | 2026-04-07 |
| 12. SDK Instance Model | 4/4 | Complete | 2026-04-08 |
| 13. C++ Codegen Modernization | 2/2 | Complete   | 2026-04-08 |
| 14. Hot-Reload Documentation | 1/1 | Complete   | 2026-04-08 |
| 15. Final Cleanup | 9/9 | Complete    | 2026-04-09 |
| 16. Milestone Gap Closure | 5/5 | Complete | 2026-04-09 |
| 17. RuntimeStore Refactor | 0/2 | Pending | — |
| 18. Consolidate FFI to HostInterface | 0/5 | Pending | — |

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
    |
    v
Phase 11 (Guest Calling Convention & Missing Introspection)
    |
    v
Phase 12 (SDK Instance Model Completion)
    |
    v
Phase 13 (C++ Codegen Modernization)
    |
    v
Phase 14 (Hot-Reload Documentation)
    |
    v
Phase 15 (Final Cleanup)
        |
        v
Phase 16 (Milestone Gap Closure)
        |
        v
Phase 17 (RuntimeStore Refactor)
        |
        v
Phase 18 (Consolidate FFI to HostInterface)
```

### Phase 12: SDK Instance Model Completion

**Goal:** Complete SDK updates to use polyplug_abi types and add instance-based wrappers
**Depends on:** Phase 11
**Requirements:** SDK-01, SDK-05, SDK-07
**Gap Closure:** Closes SDK gaps from audit
**Success Criteria** (what must be TRUE):
1. Rust host SDK imports types from polyplug_abi (no duplicates)
2. JS SDK uses TypeScript interfaces from polyplug_abi
3. All SDKs generate instance-based wrappers via codegen
**Plans:** 4/4 plans complete

Plans:
- [x] 12-01-PLAN.md — Verify Rust SDK polyplug_abi imports (SDK-01) [wave 1]
- [x] 12-02-PLAN.md — Update JS SDK TypeScript type naming (SDK-05) [wave 1]
- [x] 12-03a-PLAN.md — Instance wrappers for C++/Python generators (SDK-07 part 1) [wave 2]
- [x] 12-03b-PLAN.md — Instance wrappers for Lua/C#/JS generators + verification (SDK-07 part 2) [wave 3]

### Phase 13: C++ Codegen Modernization

**Goal:** Update C++ codegen to use modern HostInterface/instance patterns
**Depends on:** Phase 12
**Requirements:** INST-01, INST-02, INST-03, INST-04, INST-05, INST-06, CG-02, CG-03, CG-04, CG-05
**Gap Closure:** Closes instance model/codegen gaps from audit
**Success Criteria** (what must be TRUE):
1. C++ codegen generates *Instance RAII wrappers (not PluginGuard)
2. Generated wrappers call create_instance on construction
3. Generated wrappers call destroy_instance on drop
4. Instance passed as first argument to all dispatch calls
5. C++ SDK uses HostInterface terminology
**Plans:** 2/2 plans complete

Plans:
- [x] 13-01-PLAN.md — Rename vtable terminology to interface in C++ codegen (Wave 1)
- [x] 13-02-PLAN.md — Create integration test and verify SDK consistency (Wave 2)

### Phase 14: Hot-Reload Documentation

**Goal:** Create VERIFICATION.md for hot-reload callback model requirements
**Depends on:** Phase 13
**Requirements:** HR-01, HR-02, HR-03, HR-04, HR-05, HR-06
**Gap Closure:** Closes verification gaps from audit
**Success Criteria** (what must be TRUE):
1. Phase 04 VERIFICATION.md updated with HR-01 through HR-06 verified
2. Hot-reload callback model documented with evidence
**Plans:** 1/1 plans complete

### Phase 15: Final Cleanup

**Goal:** Complete remaining naming and test cleanup
**Depends on:** Phase 14
**Requirements:** CLN-01, CLN-04
**Gap Closure:** Closes cleanup gaps from audit
**Success Criteria** (what must be TRUE):
1. No "vtable" naming remains in codebase (excluding ABI fields and planning artifacts)
2. All tests use new instance model and naming
**Plans:** 9/9 plans complete

Wave Structure:
- Wave 1: Generator updates (Plan 01)
- Wave 2: Regenerate examples (Plan 02)
- Wave 3: Source, tests, SDKs, fixtures, polyplugc tests (Plans 03-06, 04b) - parallel
- Wave 4: Documentation (Plan 07)
- Wave 5: Verification (Plan 08)

Plans:
- [x] 15-01-PLAN.md — Update generator files to use interface terminology [Wave 1]
- [x] 15-02-PLAN.md — Regenerate all examples after generator updates [Wave 2]
- [x] 15-03-PLAN.md — Update runtime.rs test helper functions and variables [Wave 3]
- [x] 15-04-PLAN.md — Update polyplug test files with interface terminology [Wave 3]
- [x] 15-04b-PLAN.md — Update polyplugc test files with interface terminology [Wave 3]
- [x] 15-05-PLAN.md — Update SDK files with interface terminology [Wave 3]
- [x] 15-06-PLAN.md — Update test fixtures with interface terminology [Wave 3]
- [x] 15-07-PLAN.md — Update documentation files [Wave 4]
- [x] 15-08-PLAN.md — Final verification: grep audit + test suite [Wave 5]

### Phase 16: Milestone Gap Closure

**Goal:** Close all remaining audit gaps: verification reconciliation, generator comments, documentation, checkbox updates
**Depends on:** Phase 15
**Requirements:** CLN-03, TH-01, TH-04, TH-06, HC-02, HC-03, HC-04
**Gap Closure:** Closes all deferred/partial gaps from milestone audit
**Success Criteria** (what must be TRUE):
1. REQUIREMENTS.md checkboxes reflect actual state (HC-* marked complete)
2. Phase 07 VERIFICATION.md matches actual code (RuntimeContext not implemented, *mut c_void used)
3. 4 generator comments updated (no "VTable" in non-test code)
4. Documentation uses Guest/Host terminology consistently
5. All tests pass
**Plans:** 5 plans

Wave Structure:
- Wave 1: REQUIREMENTS.md checkbox updates (Plan 01)
- Wave 2: Phase 07 VERIFICATION.md reconciliation (Plan 02)
- Wave 3: Generator comment fixes (Plan 03)
- Wave 4: Documentation terminology (Plan 04)
- Wave 5: Final verification (Plan 05)

Plans:
- [ ] 16-01-PLAN.md — Update REQUIREMENTS.md checkboxes for HC-02/03/04 [Wave 1]
- [ ] 16-02-PLAN.md — Reconcile Phase 07 VERIFICATION.md with actual code state [Wave 2]
- [ ] 16-03-PLAN.md — Fix 4 generator comments with VTable terminology [Wave 3]
- [ ] 16-04-PLAN.md — Update documentation with Guest/Host terminology (CLN-03) [Wave 4]
- [ ] 16-05-PLAN.md — Final verification: grep audit + test suite [Wave 5]

### Phase 17: Refactor ContractRegistry to unified RuntimeStore

**Goal:** Rename ContractRegistry to RuntimeStore and add bundle-level indexing with BundleDescriptor for complete bundle/plugin management. Follow all AGENTS.md rules.
**Requirements:** STORE-01, STORE-02, STORE-03, STORE-04
**Depends on:** Phase 16
**Success Criteria** (what must be TRUE):
1. find_slots_by_bundle() becomes O(1) lookup instead of O(n) scan
2. Bundle metadata available through RuntimeStore, not split across Runtime
3. All tests pass with renamed types
4. All AGENTS.md rules followed (no type aliases, explicit types, no deprecated code)
**Plans:** 2 plans

Wave Structure:
- Wave 1: Pass 1 — Rename types, methods, fields (Plan 01)
- Wave 2: Pass 2 — Add BundleData, BundleDescriptor, bundle_name_index, new APIs (Plan 02)

Plans:
- [ ] 17-01-PLAN.md — Pass 1: Rename ContractRegistry to RuntimeStore and all methods/fields [Wave 1]
- [ ] 17-02-PLAN.md — Pass 2: Add BundleData, BundleDescriptor, bundle_name_index, new APIs [Wave 2]

### Phase 18: Consolidate FFI to HostInterface

**Goal:** Reduce FFI exports from 13 functions to 2 (create/destroy). All operations move into HostInterface struct fields. Host apps AND plugins use same HostInterface API.
**Requirements:** D-18-01 through D-18-34
**Depends on:** Phase 17
**Success Criteria** (what must be TRUE):
1. Only 2 FFI exports: polyplug_runtime_create and polyplug_runtime_destroy
2. polyplug_runtime_create returns HostInterface* (not OpaqueRuntime*)
3. HostInterface contains ALL operations (load_bundle, reload_bundle, find_guest_contract, etc.)
4. Host apps AND plugins both call HostInterface methods
5. All 5 SDKs updated to use HostInterface pointer
6. All 7 code generators updated for HostInterface API
7. All tests pass with unified API
**Plans:** 5 plans

Wave Structure:
- Wave 1: HostInterface struct changes (Plan 01) - add fields, rename fields
- Wave 2: FFI deletions + Runtime implementation (Plan 02)
- Wave 3: SDK updates - Python/C# (Plan 03) + Lua/JS/C++ (Plan 04)
- Wave 4: Code generators + tests + verification (Plan 05)

Plans:
- [ ] 18-01-PLAN.md — HostInterface struct changes: add 6 fields, rename 3 fields [Wave 1]
- [ ] 18-02-PLAN.md — FFI deletions + Runtime implementation + create returns HostInterface* [Wave 2]
- [ ] 18-03-PLAN.md — Python + C# SDK updates for HostInterface API [Wave 3]
- [ ] 18-04-PLAN.md — Lua + JS + C++ SDK updates for HostInterface API [Wave 3]
- [ ] 18-05-PLAN.md — Code generators + test updates + verification [Wave 4]

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
*Phase 11 plans added: 2026-04-07*
*Phase 12 plans added: 2026-04-08*
*Phase 12 plan 03 split into 03a/03b per checker feedback: 2026-04-08*
*Phase 13 plans added: 2026-04-08*
*Phase 15 plans added: 2026-04-08*
*Phase 15 plan 04b added for polyplugc tests: 2026-04-08*
*Phase 16 added for milestone gap closure: 2026-04-09*
*Phase 17 plans added: 2026-04-10*
*Phase 18 plans added: 2026-04-10*