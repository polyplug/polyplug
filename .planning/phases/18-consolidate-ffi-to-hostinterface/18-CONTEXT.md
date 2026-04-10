# Phase 18: Consolidate FFI to HostInterface - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning
**Source:** User discussion on FFI architecture

<domain>
## Phase Boundary

This phase consolidates all runtime operations into the `HostInterface` struct, reducing FFI exports to only two functions: `polyplug_runtime_create` and `polyplug_runtime_destroy`.

**What this phase delivers:**
1. Single unified API: HostInterface struct contains ALL operations
2. Minimal FFI surface: Only create/destroy exported from library
3. Same API for host apps AND plugins: Both use HostInterface methods
4. No duplicate entry points: Remove redundant polyplug_runtime_* functions

**Current state (problem):**
- FFI exports 13 functions: create, destroy, load_bundle, reload_bundle, find_guest_contract, find_all_by_contract, resolve_guest_contract, etc.
- HostInterface has 12 fields for plugin callbacks
- Both do similar things (find_contract, resolve_contract) but with different names
- Host apps use FFI functions, plugins use HostInterface fields
- This creates confusion, redundancy, and maintenance burden

**Target state:**
- FFI exports 2 functions: `polyplug_runtime_create()` returns HostInterface*, `polyplug_runtime_destroy()` takes HostInterface*
- HostInterface contains ALL operations: load_bundle, find_contract, resolve_contract, etc.
- Host apps AND plugins both call HostInterface methods
- Same struct, same API, clear architecture

</domain>

<decisions>
## Locked Decisions (User Requirements)

### FFI Surface
- **D-18-01:** Only two FFI exports: `polyplug_runtime_create` and `polyplug_runtime_destroy`
- **D-18-02:** `polyplug_runtime_create` returns `HostInterface*` (not OpaqueRuntime*)
- **D-18-03:** All operations live in HostInterface struct fields
- **D-18-04:** No backward compatibility code (AGENTS.md rule 14)

### HostInterface Fields (Operations to Add)
- **D-18-05:** Add `load_bundle: unsafe extern "C" fn(this, path, path_len) -> u32`
- **D-18-06:** Add `reload_bundle: unsafe extern "C" fn(this, path, path_len) -> u32`
- **D-18-07:** Add `register_host_contract: unsafe extern "C" fn(this, contract_id, interface) -> u32`
- **D-18-08:** Add `register_loader: unsafe extern "C" fn(this, runtime_name, loader) -> u32`
- **D-18-09:** Rename `find_by_contract` to `find_guest_contract` (consistency with recent rename)
- **D-18-10:** Rename `find_all_by_contract` to `find_all_guest_contracts`
- **D-18-11:** Rename `resolve_contract` to `resolve_guest_contract`
- **D-18-12:** Keep `register_contract` as-is (plugins use this to register themselves)
- **D-18-13:** Keep `alloc` and `free` as-is (memory management)

### FFI Functions to Remove
- **D-18-14:** Delete `polyplug_runtime_load_bundle`
- **D-18-15:** Delete `polyplug_runtime_reload_bundle`
- **D-18-16:** Delete `polyplug_runtime_find_guest_contract`
- **D-18-17:** Delete `polyplug_runtime_find_guest_contract_by_bundle`
- **D-18-18:** Delete `polyplug_runtime_find_all_by_contract` → becomes `find_all_guest_contracts` in HostInterface
- **D-18-19:** Delete `polyplug_runtime_resolve_guest_contract`
- **D-18-20:** Delete `polyplug_runtime_register_host_contract`
- **D-18-21:** Delete `polyplug_runtime_register_loader`
- **D-18-22:** Delete `polyplug_runtime_last_error` → add `get_last_error` field to HostInterface
- **D-18-23:** Delete `polyplug_runtime_error_message_len` → add `get_error_len` field to HostInterface

### ABI Stability (Critical)
- **D-18-24:** HostInterface struct is #[repr(C)] - adding fields at end is ABI-safe for existing callers
- **D-18-25:** Field order MUST NOT change - existing fields stay in current positions
- **D-18-26:** New fields append at end of struct
- **D-18-27:** Field types use function pointer pattern (same as existing fields)

### SDK Updates
- **D-18-28:** Python SDK: Runtime class holds HostInterface pointer, calls methods through it
- **D-18-29:** C# SDK: Runtime class holds HostInterface pointer, calls methods through it
- **D-18-30:** Lua SDK: Runtime class holds HostInterface pointer, calls methods through it
- **D-18-31:** JS SDK: Runtime class holds HostInterface pointer, calls methods through it
- **D-18-32:** C++ SDK: Runtime class holds HostInterface pointer, calls methods through it

### Code Generator Updates
- **D-18-33:** polyplugc generates host_callers.rs that uses HostInterface methods (not FFI functions)
- **D-18-34:** All generators (Rust, Python, Lua, C#, C++, JS) updated to use HostInterface API

</decisions>

<canonical_refs>
## Canonical References

### ABI Definition
- `crates/polyplug_abi/src/host/host_interface.rs` — HostInterface struct definition (CRITICAL)
- `crates/polyplug/src/ffi.rs` — Current FFI exports (to be reduced)
- `crates/polyplug/src/runtime.rs` — Runtime struct implementation

### SDKs to Update
- `sdks/python/host/polyplug/runtime.py`
- `sdks/csharp/host/Runtime.cs`
- `sdks/csharp/host/NativeMethods.cs`
- `sdks/lua/host/polyplug/runtime.lua`
- `sdks/js/host/polyplug/runtime.ts`
- `sdks/cpp/host/polyplug/runtime.hpp`

### Code Generators to Update
- `crates/polyplugc/src/generators/rust.rs`
- `crates/polyplugc/src/generators/python.rs`
- `crates/polyplugc/src/generators/lua.rs`
- `crates/polyplugc/src/generators/csharp.rs`
- `crates/polyplugc/src/generators/cpp.rs`
- `crates/polyplugc/src/generators/js_deno.rs`
- `crates/polyplugc/src/generators/js_quickjs.rs`

### Test Files to Update
- `crates/polyplug/tests/ffi_edge_cases.rs`
- `crates/polyplug/tests/integration_ffi_null.rs`
- `crates/polyplug/tests/integration_ffi_robustness.rs`
- `crates/polyplug/benches/ffi_resolve.rs`
- `crates/polyplug/benches/ffi_find_all.rs`

### AGENTS.md Rules to Follow
- Rule 14: No backward compatibility code
- Rule 16: No type aliases
- Rule 7: ABI stability - add fields at end, don't reorder

</canonical_refs>

<open_questions>
## Claude's Discretion (Implementation Details)

### OpaqueRuntime vs HostInterface Return
- Should `polyplug_runtime_create` return `HostInterface*` directly, or return `OpaqueRuntime*` that contains HostInterface?
- **Recommendation:** Return `HostInterface*` directly. The HostInterface IS the runtime handle. No need for wrapper struct.

### Error Handling Pattern
- HostInterface methods return `u32` error codes (same as FFI functions)
- Should error messages go through HostInterface.get_last_error, or separate mechanism?
- **Recommendation:** Add `get_last_error` and `get_error_len` fields to HostInterface for consistency.

### Self-Passing Pattern
- Existing HostInterface fields use `this: *const HostInterface` as first parameter
- New fields should follow same pattern for consistency
- This allows extracting Runtime state from the HostInterface pointer

### Loaders Registration
- How do loaders get registered if register_loader is in HostInterface?
- Host app calls `host->register_loader(host, "rust", loader_ptr)` after create
- This works the same as current FFI approach, just through HostInterface

### Hot-Reload
- reload_bundle goes into HostInterface
- Callbacks (on_reload, on_warning) passed during create via options struct?
- Or registered separately via HostInterface fields?

### What About polyplug_runtime_create_with_options?
- Delete it? Or make it the primary create function with options struct?
- **Recommendation:** Single `polyplug_runtime_create` with options pointer parameter (null for defaults)

</open_questions>

<constraints>
## Technical Constraints

1. **ABI Field Order:** Existing HostInterface fields MUST stay in current order (ABI contract)
2. **Field Types:** All new fields are `unsafe extern "C" fn` pointers (same pattern)
3. **Self-Passing:** All methods take `this: *const HostInterface` first parameter
4. **No Breaking Plugin ABI:** Plugins compiled against old HostInterface still work (fields at same offsets)
5. **SDK Binary Compatibility:** SDKs must recompile against new HostInterface (no backward compat per AGENTS.md)

</constraints>