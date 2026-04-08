# Requirements: polyplug v1.1 Architecture Refactor

**Milestone:** v1.1 Architecture Refactor
**Created:** 2026-04-03
**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

## v1.1 Requirements

### Category: ABI Types

- [x] **ABI-01**: Rename `PluginInterface` to `GuestContractInterface`
- [x] **ABI-02**: Create `HostContractInterface` with `singleton` field
- [x] **ABI-03**: Add `create_instance` and `destroy_instance` to `GuestContractInterface` returning `GuestContractInstance`
- [x] **ABI-04**: Add `create_instance` and `destroy_instance` to `HostContractInterface` returning `HostContractInstance`
- [x] **ABI-05**: Move `RuntimeConfig` from `polyplug` crate to `polyplug_abi`
- [x] **ABI-06**: Create FFI-safe `ReloadPhaseData` struct in `polyplug_abi` (existing `ReloadPhase` enum with String fields stays in `polyplug` for internal Rust use)
- [ ] **ABI-07**: Move `RuntimeCreateOptions` to `polyplug_abi` — **DEFERRED**: Type does not exist in current codebase; may be addressed in later phase if needed
- [x] **ABI-08**: Rename `HostVTable` to `RuntimeAbi`
- [x] **ABI-09**: Update `VmDispatch` to include `GuestContractInstance` parameter
- [x] **ABI-10**: Add `call_method` to `RuntimeAbi` with `GuestContractInstance` param
- [x] **ABI-11**: Rename ID types: `PluginContractId` → `GuestContractId`
- [x] **ABI-12**: Ensure all public ABI structs are `#[repr(C)]`
- [x] **ABI-13**: Create `GuestContractInstance` opaque handle struct
- [x] **ABI-14**: Create `HostContractInstance` opaque handle struct

### Category: Registry

- [x] **REG-01**: Remove `VTableSlot` wrapper - store `GuestContractInterface` directly
- [x] **REG-02**: Remove `PluginGuard` - replaced by instance model
- [x] **REG-03**: Remove generation counter from handles (`ContractHandle`)
- [x] **REG-04**: Remove `ArcSwap` pattern - hot-reload uses callback instead
- [x] **REG-05**: Simplify `RegistrySlot` to store interface directly
- [x] **REG-06**: Update `find_contract` to return `ContractHandle` without generation

### Category: Instance Model

- [ ] **INST-01**: Update codegen to generate `*Instance` RAII wrappers
- [ ] **INST-02**: Generated wrapper calls `create_instance` on construction
- [ ] **INST-03**: Generated wrapper calls `destroy_instance` on drop
- [ ] **INST-04**: Instance passed as first argument to all dispatch calls
- [ ] **INST-05**: Native dispatch: `functions[fn_id](instance, args, out)`
- [ ] **INST-06**: VM dispatch: `call(loader_data, instance, fn_id, args, out)`

### Category: Hot-Reload

- [ ] **HR-01**: Remove `wait_for_quiescence` with `Arc::strong_count`
- [ ] **HR-02**: Update hot-reload to use callback-only model
- [ ] **HR-03**: `ReloadPhase::Preparing` fires before interface swap
- [ ] **HR-04**: Host destroys all instances in callback
- [ ] **HR-05**: Runtime swaps interfaces after callback returns
- [ ] **HR-06**: Warning callback if instances remain (UB warning)

### Category: Host Contracts

- [x] **HC-01**: `HostContractInterface` supports `singleton: bool` field
- [ ] **HC-02**: `get_host_contract` returns same instance for singleton
- [ ] **HC-03**: `get_host_contract` creates new instance for multi-instance
- [ ] **HC-04**: Update codegen for host contract implementations

### Category: RuntimeAbi

- [x] **RTABI-01**: Rename `register_plugin` (was `register_contract`)
- [x] **RTABI-02**: `find_contract` returns `ContractHandle`
- [x] **RTABI-03**: `resolve_contract` returns `*const GuestContractInterface`
- [x] **RTABI-04**: `get_host_contract` returns `HostContractInstance` (not bare pointer)
- [x] **RTABI-05**: Remove `find_by_bundle` from ABI (internal only)

### Category: SDK Updates

- [x] **SDK-01**: Update Rust host SDK to use `polyplug_abi` types
- [x] **SDK-02**: Update Python SDK - remove `RuntimeConfigC` duplicate
- [x] **SDK-03**: Update C# SDK - remove `RuntimeConfigC` duplicate
- [x] **SDK-04**: Update Lua SDK - use types from `polyplug_abi`
- [x] **SDK-05**: Update JS SDK - use types from `polyplug_abi`
- [x] **SDK-06**: Remove `PluginGuard` from all SDKs
- [x] **SDK-07**: Add instance-based wrappers to all SDKs (codegen)

### Category: Codegen

- [x] **CG-01**: Update codegen to use `GuestContractInterface` naming
- [ ] **CG-02**: Update codegen to generate instance wrappers
- [ ] **CG-03**: Generated instance wrappers hold `interface` + `instance` pointer
- [ ] **CG-04**: Generated wrappers call `create_instance`/`destroy_instance`
- [x] **CG-05**: Update host contract vtable generation for `HostContractInterface`
- [x] **CG-06**: Generate `singleton` support for host contracts

### Category: Cleanup

- [ ] **CLN-01**: Remove all "vtable" naming from codebase
- [x] **CLN-02**: Remove `*C` suffix types from FFI
- [ ] **CLN-03**: Update documentation to use Guest/Host terminology
- [ ] **CLN-04**: Update tests to use new instance model

### Category: Typed Handles (Phase 7)

- [x] **TH-01**: Replace `rt_ctx: *mut c_void` with `RuntimeContext` typed handle
- [x] **TH-02**: Replace `loader_data: *mut c_void` with `VmLoaderData` typed handle
- [x] **TH-03**: Replace `instance: *mut c_void` in native dispatch with `GuestContractInstance`
- [x] **TH-04**: Create `RuntimeContext` struct (opaque handle to Runtime)
- [x] **TH-05**: Create `VmLoaderData` struct (opaque handle to VM state)
- [x] **TH-06**: Update all RuntimeAbi functions to use `RuntimeContext` instead of `*mut c_void`
- [x] **TH-07**: Update PluginContext to use typed handles
- [x] **TH-08**: Ensure all opaque handles are `#[repr(C)]` with single `data` field

## v1 Requirements (Complete)

### Error Types

- [x] **ERR-01**: Remove Python-specific error variants from core `LoaderError`
- [x] **ERR-02**: Remove Lua-specific error variants from core `LoaderError`
- [x] **ERR-03**: Remove JS-specific error variants from core `LoaderError`
- [x] **ERR-04**: Remove .NET-specific error variants from core `LoaderError`
- [x] **ERR-05**: Ensure each loader crate exports its own error type
- [x] **ERR-06**: Update loaders to use `LoaderError::InitFailed` directly

### Compatibility

- [x] **COMP-01**: All existing tests pass after error type migration
- [x] **COMP-02**: No breaking changes to public FFI API

## Out of Scope

| Feature | Reason |
|---------|--------|
| WASM runtime support | Architectural decision — native plugins are the design |
| Plugin sandboxing | Host responsibility for trust |
| Manifest parsing move | Can be done later, not blocking |
| New loader implementations | Out of scope for this refactor |

## Traceability

| Requirement | Phase |
|-------------|-------|
| ABI-01 through ABI-14 | Phase 1: ABI Types |
| RTABI-01 through RTABI-05 | Phase 1: ABI Types |
| REG-01 through REG-06 | Phase 8: Retroactive Verification |
| INST-01 through INST-06 | Phase 13: C++ Codegen Modernization |
| HC-01 | Phase 3: Instance Model |
| HC-02 through HC-04 | Phase 8: Retroactive Verification |
| CG-01, CG-06 | Phase 3: Instance Model |
| CG-02 through CG-05 | Phase 13: C++ Codegen Modernization |
| HR-01 through HR-06 | Phase 14: Hot-Reload Documentation |
| SDK-01, SDK-07 | Phase 12: SDK Instance Model Completion |
| SDK-02 through SDK-04, SDK-06 | Phase 10: SDK Cleanup Completion |
| SDK-05 | Phase 12: SDK Instance Model Completion |
| CLN-01, CLN-04 | Phase 15: Final Cleanup |
| CLN-02 | Phase 10: SDK Cleanup Completion |
| CLN-03 | Phase 6: Cleanup |
| TH-01 through TH-08 | Phase 8: Retroactive Verification |

**Coverage:**
- v1.1 requirements: 58 total
- Mapped to phases: 58
- Unmapped: 0

---
*Requirements defined: 2026-04-03*
*Last updated: 2026-04-06 for gap closure phase assignments*