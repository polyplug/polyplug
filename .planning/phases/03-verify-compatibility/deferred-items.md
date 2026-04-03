# Deferred Items for Phase 03

## Pre-existing Compilation Issues in Core Crate

**Found during:** Plan 03-04 verification
**Issue:** `cargo check -p polyplug_dotnet` fails due to pre-existing errors in `crates/polyplug/src/`:
- `runtime_builder.rs:8:52` - duplicate RuntimeError import
- `runtime_builder.rs:7:21` - unresolved import CapabilityGraph (similar name exists)
- `runtime_builder.rs:11` - unresolved import ReloadCb
- `ffi.rs:11` - unresolved import VTableSlot
- `ffi.rs:56,60` - unresolved type StringViewC
- `ffi.rs:150,175,176` - unresolved type RuntimeConfigC
- `ffi.rs:551,561` - unresolved type HostContractVTable
- `registry/plugin_registry.rs:45,247` - unresolved type VTableSlot

**Scope:** These errors are in files NOT modified by this plan (03-04). The plan only modified test files in `crates/polyplug_dotnet/tests/` and `tests/integration/tests/`.

**Decision:** Out of scope per deviation rules. Deferred for resolution by the responsible agent/phase.

---
*Last updated: 2026-04-03 during 03-04 execution*