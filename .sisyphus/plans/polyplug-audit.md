# polyplug Codebase Audit Report

**Date:** 2026-03-12  
**Auditor:** Prometheus (planning agent)  
**Scope:** All `.rs` files in `crates/` (48 files across 7 crates)  
**References:** `AGENTS.md`, `TRUST_MODEL.md`, `polyplug_prd.md`

---

## Audit Scope

The following files were read in full before any finding was recorded:

### crates/polyplug/
- `src/lib.rs`
- `src/abi/mod.rs`
- `src/allocator/mod.rs`
- `src/allocator/tracking/mod.rs`
- `src/error/mod.rs`
- `src/registry/mod.rs`
- `src/loader/mod.rs`
- `src/loader/manifest/mod.rs`
- `src/loader/scanner/mod.rs`
- `src/graph/mod.rs`
- `src/runtime/mod.rs`
- `src/reload/mod.rs`
- `src/ffi/mod.rs`
- `src/version/mod.rs`
- `src/extensions/mod.rs`
- `src/extensions/trace/mod.rs`
- `build.rs`
- `benches/vtable_dispatch.rs`

### crates/polyplugc/
- `src/main.rs`
- `src/error/mod.rs`
- `src/ir/mod.rs`
- `src/parser/mod.rs`
- `src/pack/mod.rs`
- `src/generators/mod.rs`
- `src/generators/rust/mod.rs`
- `src/generators/cpp/mod.rs`
- `src/generators/csharp/mod.rs`
- `src/generators/python/mod.rs`
- `src/generators/lua/mod.rs`
- `src/generators/js_quickjs/mod.rs`
- `src/generators/js_deno/mod.rs`

### crates/polyplug-dotnet/
- `src/lib/mod.rs`
- `src/lib/config/mod.rs`
- `src/lib/context/mod.rs`
- `src/lib/version/mod.rs`

### crates/polyplug-python/
- `src/lib/mod.rs`
- `src/lib/config/mod.rs`
- `src/lib/context/mod.rs`

### crates/polyplug-lua/
- `src/lib/mod.rs`
- `src/lib/config/mod.rs`
- `src/lib/loader/mod.rs`
- `build.rs`

### crates/polyplug-js/
- `src/lib/mod.rs`
- `src/lib/config/mod.rs`
- `src/lib/loader/mod.rs`

### crates/polyplug-js-deno/
- `src/lib/mod.rs`
- `src/lib/config/mod.rs`
- `src/lib/loader/mod.rs`

---

## Category Index

1. [Category 1 — Unsafe Blocks Without Justification or Soundness Issues](#category-1)
2. [Category 2 — Unsafe impl Send/Sync](#category-2)
3. [Category 3 — PRD Correctness Deviations](#category-3)
4. [Category 4 — Redundancy](#category-4)
5. [Category 5 — Misaligned Concepts](#category-5)
6. [Category 6 — Error Handling](#category-6)
7. [Category 7 — Performance Regressions](#category-7)
8. [Category 8 — AGENTS.md Violations](#category-8)

---

## Category 1 — Unsafe Blocks Without Justification or Soundness Issues {#category-1}

### Finding 1.1 — TRUST_MODEL violation: `from_utf8_unchecked` on plugin-supplied data
**File:** `crates/polyplug/src/extensions/trace/mod.rs`  
**Function:** `trace_emit_thunk` (exact line depends on codebase snapshot; search for `from_utf8_unchecked`)  
**Problem:** `std::str::from_utf8_unchecked` is called at line 44 on `msg` — a `StringView` passed directly from a plugin across the ABI boundary. The SAFETY comment claims this is sound because "the ABI contract states that msg.ptr points to valid UTF-8 bytes." This reasoning is circular: TRUST_MODEL.md is unambiguous: "`from_utf8_unchecked` is never used on data originating from a plugin binary or passed from a plugin across the ABI boundary." The ABI contract cannot be enforced on a misbehaving plugin; the host must validate rather than assume. The existing SAFETY comment does not justify this against TRUST_MODEL — it merely restates the expectation.
**Fix:** Replace `core::str::from_utf8_unchecked(core::slice::from_raw_parts(msg.ptr, msg.len))` with `core::str::from_utf8(core::slice::from_raw_parts(msg.ptr, msg.len)).unwrap_or("<invalid utf-8>")`. Remove the SAFETY comment that claims the ABI contract makes this sound, because TRUST_MODEL explicitly forbids this pattern on plugin-provided data.

### Finding 1.2 — Intentional `Box::leak` without explanation in production path
**File:** `crates/polyplug/src/runtime/mod.rs`  
**Line:** 259 (`let host_vtable: &'static HostVTable = Box::leak(Box::new(HostVTable {`)
**Problem:** `Box::leak` is used to create a `'static` `HostVTable`. The comment at line 258 says only `// Build the static HostVTable. This must be 'static.` — it explains the constraint but does not constitute a `// SAFETY:` justification per AGENTS.md Rule 6. The fact that the leak is intentional (process-lifetime ownership) is not documented.
**Fix:** Add a comment directly above the line: `// SAFETY: HostVTable must have 'static lifetime because PluginRegistrar passes its pointer to all loaded plugins. The runtime owns this for the process lifetime; leaking is intentional.`

### Finding 1.3 — Unsafe thread spawn stores raw pointer across thread boundary without soundness guarantee
**File:** `crates/polyplug-js-deno/src/lib/loader/mod.rs`  
**Line:** ~492 (`let host_vtable_addr: usize = registrar.host as usize;`)  
**Problem:** `registrar.host` is cast to `usize` to make it `Send`, then reconstructed as `*const HostVTable` on the bundle thread. The `// SAFETY:` comment says it is "Box::leak'd by RuntimeBuilder — valid 'static." This is correct reasoning, but the comment appears at line ~516 inside the thread closure, not at line ~492 where the cast occurs. The cast site has no safety comment. Per AGENTS.md Rule 6: every unsafe operation must have a `// SAFETY:` comment explaining why it is sound.  
**Fix:** Add at line 492, before the cast: `// SAFETY: registrar.host is Box::leak'd by RuntimeBuilder (see runtime/mod.rs). It is valid 'static. We store it as usize to make it Send; the bundle thread reconstructs it as *const HostVTable only after thread_local assignment on that thread.`

---

## Category 2 — Unsafe impl Send/Sync {#category-2}

### Finding 2.1 — `unsafe impl Send/Sync for Registry` likely unnecessary
**File:** `crates/polyplug/src/registry/mod.rs`  
**Lines:** (search for `unsafe impl Send for Registry` and `unsafe impl Sync for Registry`)  
**Problem:** `Registry` contains fields of type `RwLock<...>` and `Mutex<...>`. These types auto-implement `Send + Sync` when their contained types are `Send`. Manually adding `unsafe impl Send + Sync` bypasses the compiler's verification. If a future developer adds a non-Send/non-Sync field, the compiler will no longer catch it.  
**Fix:** Remove `unsafe impl Send for Registry` and `unsafe impl Sync for Registry`. Run `cargo check` to confirm the auto-derived impls satisfy the trait bounds. If the compiler rejects this, document exactly which field is `!Send`/`!Sync` and why it is safe to override.

### Finding 2.2 — `unsafe impl Send/Sync for RegistryEntry` bypasses auto-derive
**File:** `crates/polyplug/src/registry/mod.rs`  
**Lines:** (search for `unsafe impl Send for RegistryEntry` and `unsafe impl Sync for RegistryEntry`)  
**Problem:** Same problem as Finding 2.1. `RegistryEntry` contains `Arc<Mutex<...>>` which is `Send + Sync`. The manual unsafe impls bypass auto-derive. If any contained raw pointer field is the reason, that field must be identified and commented.  
**Fix:** Same as 2.1. Remove the manual impls and let the compiler verify. If a raw pointer field requires the override, add a `// SAFETY:` comment naming that specific field and why it is safe to send.

### Finding 2.3 — `unsafe impl Send/Sync for Runtime` without per-field justification
**File:** `crates/polyplug/src/runtime/mod.rs`  
**Lines:** (search for `unsafe impl Send for Runtime`)  
**Problem:** `Runtime` has `unsafe impl Send + Sync` but does not document which specific fields are `!Send`/`!Sync` by default and why it is safe to override each one. The comment must be per-field, not per-struct.  
**Fix:** Add a `// SAFETY:` comment that enumerates each `!Send`/`!Sync` field and the specific invariant that makes Send/Sync sound. Example: `// SAFETY: Runtime contains raw ptr X which is !Send. X is only accessed while holding Mutex<Y>, therefore concurrent access is serialized.`

### Finding 2.4 — `unsafe impl Send/Sync for HostVtablePtr` in `polyplug-js`
**File:** `crates/polyplug-js/src/lib/loader/mod.rs`  
**Lines:** 59–62  
**Problem:** `unsafe impl Send for HostVtablePtr` and `unsafe impl Sync for HostVtablePtr` have comments that say "The data it points to is never mutated after construction." This is correct but incomplete. The comment does not confirm that the pointed-to `HostVTable` is not accessed concurrently from multiple threads in a way that races. Since `HostVTable` itself has `unsafe impl Send + Sync` in `abi/mod.rs`, and since `OnceLock::get_or_init` is the only write path, this is sound — but the comment must explicitly state: "HostVTable is immutable after construction (all fn-ptr fields set once by RuntimeBuilder) and has unsafe impl Send+Sync in abi/mod.rs."  
**Fix:** Update the SAFETY comment to: `// SAFETY: HostVtablePtr wraps a *const HostVTable that is 'static (Box::leak'd by RuntimeBuilder). HostVTable is immutable after construction — all function pointer fields are set once and never modified. HostVTable itself has unsafe impl Send+Sync declared in abi/mod.rs with equivalent justification.`

---

## Category 3 — PRD Correctness Deviations {#category-3}

### Finding 3.1 — CRITICAL: `find_all_by_contract` returns `Vec` — PRD requires caller-provides-buffer
**File:** `crates/polyplug/src/registry/mod.rs`  
**Function:** `find_all_by_contract` (~line 372)  
**Problem:** The function returns `Vec<PluginHandle>`. PRD §6 states: "The runtime MUST NOT allocate on behalf of the caller in the hot dispatch path." PRD §5 states the ABI uses a caller-provides-buffer pattern for all collection results. This function allocates a `Vec` on every call and returns it. The public ABI function in `ffi/mod.rs` wraps this and writes into a caller-provided buffer, but the internal `registry` layer still performs an allocation that is then discarded. The allocation is unnecessary and violates the no-allocation constraint in the hot path.  
**Fix:** Change the signature to `find_all_by_contract(&self, contract_id: u64, min_version: u32, out: *mut PluginHandle, capacity: usize) -> usize`. Write directly into `out` up to `capacity` items, return total count. This is a non-trivial change that touches registry internals, ffi, and all callers. Every caller must be updated.

### Finding 3.2 — CRITICAL: All capability versions hardcoded to `Version { major: 1, minor: 0 }`
**File:** `crates/polyplug/src/graph/mod.rs`  
**Function:** `ContractCapability::new` (~line 202 and ~218)  
**Problem:** Every `ContractCapability` constructed in the dependency graph sets `version: Version { major: 1, minor: 0 }` regardless of the actual version declared in the bundle manifest. PRD §8 (Dependency Graph) and §9 (Version Negotiation) require that version metadata from the manifest be used for compatibility checks. With this hardcode, a bundle declaring version `2.0` will be treated as `1.0`. Version negotiation is completely non-functional.  
**Fix:** Pass the `Version` from the manifest into `ContractCapability::new`. Specifically, in the caller that builds capabilities from `PluginDescriptor`, use `descriptor.version_major` and `descriptor.version_minor` to construct the `Version`. Remove the `Version { major: 1, minor: 0 }` literal.

### Finding 3.3 — FFI function names diverge from PRD §25 specification
**File:** `crates/polyplug/src/ffi/mod.rs`  
**Problem:** The functions in `ffi/mod.rs` use the `_rt_` infix (e.g., `polyplug_rt_find_by_contract`, `polyplug_rt_find_all_by_contract`, `polyplug_rt_resolve_plugin`). Meanwhile `lib.rs` correctly exports `polyplug_runtime_init`, `polyplug_find_all_by_contract`, etc. matching PRD §25. The split creates two public C API surfaces: `lib.rs` at correct names and `ffi/mod.rs` at non-PRD names. PRD §25 specifies a single unified C API without `_rt_` in the middle.  
**Fix:** Rename all `polyplug_rt_*` functions in `ffi/mod.rs` to their PRD §25 names. Verify no external consumer has already linked against the `_rt_` names. Since v1 has not been released, AGENTS.md Rule 7 does not prevent this rename.

### Finding 3.4 — `polyplug_load_bundle_opts` FFI function missing
**File:** `crates/polyplug/src/ffi/mod.rs`  
**Problem:** PRD §25 specifies `polyplug_load_bundle_opts(runtime, path, opts)` as a required C API function. It is not present. The `opts` parameter allows passing load-time options (e.g., trust level, timeout). Without this function, the C API cannot exercise the options path.  
**Fix:** Add `pub unsafe extern "C" fn polyplug_load_bundle_opts(runtime: *mut Runtime, path: *const c_char, opts: *const BundleLoadOpts) -> AbiError` to `ffi/mod.rs`. If `BundleLoadOpts` does not yet exist in `abi/mod.rs`, it must be added as a `#[repr(C)]` struct with at least the fields specified in PRD §25.

### Finding 3.5 — Contract ID computation uses hardcoded `major = 0` for dependencies
**File:** `crates/polyplugc/src/parser/mod.rs`  
**Line:** 541 (`let contract_id_val: u64 = compute_contract_id(&dep.contract, 0);`)  
**Problem:** `compute_contract_id(&dep.contract, 0)` hardcodes `major = 0` as the major version for dependency contract IDs. The actual minimum version is in `dep.min_version` (e.g. `"2"` or `"1"`). This means all dependency contract IDs in generated code are computed for major version 0. A bundle declaring a dependency on `my.contract` at major 2 will generate an ID for `my.contract@0`, which cannot match the provider's `my.contract@2`. PRD §4 (Contract Identity) requires contract IDs to include the major version.  
**Fix:** Replace `compute_contract_id(&dep.contract, 0)` at `parser/mod.rs:541` with `compute_contract_id(&dep.contract, dep.min_version.parse::<u32>().unwrap_or(1))`. Also check `ir/mod.rs` line 541 — that call also hardcodes `0` for the same reason.

### Finding 3.6 — CRITICAL: C# generator emits ABI stubs that never call the implementation
**File:** `crates/polyplugc/src/generators/csharp/mod.rs`  
**Line:** ~275 (search for `// call impl`)  
**Problem:** The generated `Vtables.cs` file contains ABI dispatch methods that emit `// call impl` as a comment followed by `return AbiError.Ok;`. The generated stub registers the vtable correctly but dispatches every function call to a no-op that always succeeds. A C# plugin's actual implementation method is never called. This is a critical correctness bug: C# plugins appear to load and function correctly but their business logic is never executed.  
**Fix:** The generator must emit code that actually dispatches to the concrete implementation method. The generated method must call `this.{methodName}({unpackedArgs})` and pack the result back into the out-pointer before returning. This requires the generator to emit proper marshalling code for each function signature.

### Finding 3.7 — JS loaders synthesize `contract_name` as hex string
**File:** `crates/polyplug-js/src/lib/loader/mod.rs` line 702; `crates/polyplug-js-deno/src/lib/loader/mod.rs` line 696  
**Problem:** Both JS loaders set `contract_name` in the `PluginDescriptor` to `format!("js_contract_{:#x}", contract_id_val)` and `format!("js_deno_contract_{:#x}", contract_id_val)` respectively. The `contract_name` field must match the canonical name from the bundle manifest (e.g., `"my.audio.decoder"`). Synthesizing it as a hex string means the registry cannot find this plugin by its declared contract name, breaking all contract-name-based lookups. This is the same class of bug as found previously in `loader/mod.rs` (previously catalogued finding C in the pre-audit notes).  
**Fix:** The JS bundle's `registerVtable` call must also pass the contract name string. The JS loaders must extract the contract name from the registered vtable metadata, not synthesize it from the ID. This requires changing the `registerVtable` JS function signature to accept a contract name string, and updating both generator and loader.

---

## Category 4 — Redundancy {#category-4}

### Finding 4.1 — `Registry::find()` is a dead alias for `find_by_contract()`
**File:** `crates/polyplug/src/registry/mod.rs`  
**Function:** `find` (~line 450)  
**Problem:** `find` is a method that calls `find_by_contract` with the same arguments. It adds no behaviour and is not used anywhere. It creates confusion about which method callers should use.  
**Fix:** Delete the `find` method entirely. All callers must use `find_by_contract` directly. (Verify with `lsp_find_references` that `find` has no callers before deleting.)

### Finding 4.2 — `Registry::resolve()` is a dead alias for `resolve_guard()`
**File:** `crates/polyplug/src/registry/mod.rs`  
**Function:** `resolve` (~line 458)  
**Problem:** Same as 4.1. `resolve` wraps `resolve_guard` without adding behaviour.  
**Fix:** Delete `resolve`. All callers must use `resolve_guard` directly.

### Finding 4.3 — `fnv1a_64` reimplemented in `polyplugc/src/main.rs` instead of imported
**File:** `crates/polyplugc/src/main.rs`  
**Problem:** The `fnv1a_64` function (line 178 in `main.rs`) is implemented locally and is used for **file change detection** (hashing generated file content to avoid spurious re-writes). The `compute_contract_id` function in `crates/polyplugc/src/ir/mod.rs` uses the same FNV-1a algorithm for contract ID generation. These are separate use cases, but both duplicate the FNV-1a implementation. If the hash seed or algorithm changes in one place for contract IDs, the file-change-detection hash in `main.rs` will silently diverge. More critically, a developer reading the code cannot tell whether these two FNV-1a implementations must produce identical results or may diverge.
**Fix:** Extract FNV-1a into a shared `fn fnv1a_64(data: &[u8]) -> u64` in `crates/polyplugc/src/ir/mod.rs` (where `compute_contract_id` already lives) and expose it as `pub(crate)`. Delete the local copy in `main.rs`. Import via `use crate::ir::fnv1a_64`. This makes the single implementation explicit without merging the two use cases.

### Finding 4.4 — `contract_name_to_struct` (and variants) duplicated across 5–6 generators
**Files:**  
- `crates/polyplugc/src/generators/rust/mod.rs`  
- `crates/polyplugc/src/generators/cpp/mod.rs`  
- `crates/polyplugc/src/generators/csharp/mod.rs`  
- `crates/polyplugc/src/generators/python/mod.rs`  
- `crates/polyplugc/src/generators/lua/mod.rs`  
- `crates/polyplugc/src/generators/js_quickjs/mod.rs`  
**Problem:** `contract_name_to_struct`, `arg_pack_struct_name`, and `needs_arg_pack` functions (or their equivalents) are copy-pasted into each generator. When the naming convention changes, every generator must be updated independently. The existing functions in the files are also annotated with `#[allow(dead_code)]` in some cases, indicating they are not even fully used.  
**Fix:** Move all shared naming/predicate utilities into `crates/polyplugc/src/generators/mod.rs` as `pub(crate)` free functions. Delete the per-generator copies. Each generator imports from `super::` (e.g., `use super::contract_name_to_struct`).

### Finding 4.5 — `to_snake_case` duplicated between `rust` and `python` generators
**Files:** `crates/polyplugc/src/generators/rust/mod.rs`, `crates/polyplugc/src/generators/python/mod.rs`  
**Problem:** Same function, same logic, two copies.  
**Fix:** Move to `generators/mod.rs` as `pub(crate) fn to_snake_case(s: &str) -> String`. Delete both copies.

### Finding 4.6 — `contract_to_class_name` duplicated between `js_quickjs` and `js_deno` generators
**Files:** `crates/polyplugc/src/generators/js_quickjs/mod.rs` line 408, `crates/polyplugc/src/generators/js_deno/mod.rs` line 424  
**Problem:** Identical function, identical implementation, two copies.  
**Fix:** Move to `generators/mod.rs`. Delete both copies.

### Finding 4.7 — `substitute_variant_refs_js` / `substitute_variant_refs_jsdeno` are identical logic
**Files:** `crates/polyplugc/src/generators/js_quickjs/mod.rs` line 130, `crates/polyplugc/src/generators/js_deno/mod.rs` line 142  
**Problem:** The two functions are character-for-character identical. They both scan a variant expression string, replace identifier references with their evaluated values, and return the result.  
**Fix:** Consolidate into one `pub(crate) fn substitute_variant_refs(declared_variants: &[EnumVariant], expr: &str) -> String` in `generators/mod.rs`. Delete both copies.

### Finding 4.8 — `build.rs` emits duplicate `rerun-if-changed` for the same file
**File:** `crates/polyplug/build.rs`  
**Line:** ~27 (search for duplicate `println!("cargo:rerun-if-changed=tests/fixtures/test_plugin/src/lib.rs")`)  
**Problem:** The same path is emitted twice. Cargo deduplicates these at the protocol level, so it is not a bug per se, but it is dead clutter that misleads readers.  
**Fix:** Remove the duplicate `println!` line.

---

## Category 5 — Misaligned Concepts {#category-5}

### Finding 5.1 — `lua_type_name` calls `p.cpp_name()` for Lua type names
**File:** `crates/polyplugc/src/generators/lua/mod.rs`  
**Line:** ~429  
**Problem:** `lua_type_name` dispatches through `p.cpp_name()` to determine the C-FFI type name for Lua. This works because LuaJIT uses C types via its FFI, but the name `cpp_name()` is semantically wrong — it is a C concept, not a C++ concept. A future developer adding actual C++ support will not know whether `cpp_name()` represents C ABI types or C++ name-mangled types.  
**Fix:** Rename `cpp_name()` on the `PrimitiveType` method to `c_ffi_name()`. Update all callers: the Lua generator's `lua_type_name`, the C++ generator's own callers, and anything else that calls `cpp_name()`. This is a rename, not a logic change.

### Finding 5.2 — `validate_bundle_compatibility` uses sentinel `expected: 0, found: 0` values
**File:** `crates/polyplug/src/runtime/mod.rs`  
**Function:** `validate_bundle_compatibility`  
**Problem:** The function constructs `FunctionCountMismatch { expected: 0, found: 0 }` as a placeholder or early-return sentinel when a contract is not found in the registry. Using 0/0 in an error variant named "FunctionCountMismatch" is misleading: the error looks like a count mismatch when the actual problem is a missing contract. Reading error logs from production will be confusing.  
**Fix:** Use the correct error variant for the missing-contract case (e.g., `PolyplugError::PluginNotFound` or a new `ContractNotRegistered` variant). Reserve `FunctionCountMismatch` exclusively for actual function count mismatches.

### Finding 5.3 — `INIT_BUNDLE_ID` thread-local is set without RAII guard (panic-unsafe)
**File:** `crates/polyplug/src/runtime/mod.rs`  
**Lines:** 550 (set) and 557 (clear)  
**Problem:** `INIT_BUNDLE_ID` is a `thread_local! { static ... Cell<u64> }`. It is set at line 550 before calling the loader's `load()` and cleared at line 557 after. If the loader panics (e.g., FFI plugin init raises a signal caught as a Rust panic), the `Cell` remains set for the rest of the thread's lifetime. The next `load_bundle_with` call on the same thread will see a stale non-zero `INIT_BUNDLE_ID`, which will incorrectly attribute all new plugin registrations to the previous bundle's ID. In a thread-pool that reuses threads this is a silent state-corruption bug.
**Fix:** Introduce a RAII guard: `struct BundleIdGuard; impl Drop for BundleIdGuard { fn drop(&mut self) { INIT_BUNDLE_ID.with(|c| c.set(0)); } }`. Remove the explicit clear at line 557 and replace with `let _guard = BundleIdGuard;` immediately after the set at line 550. The thread-local is then always cleared when the guard is dropped, even on panic.

### Finding 5.4 — `reload_v2` manifest fixture has `bundle_name = "reload_plugin_v1"` (copy-paste error)
**File:** `crates/polyplug/build.rs`  
**Line:** 364 (confirmed)  
**Problem:** The `reload_plugin_v2` bundle directory's `manifest.toml` is written at lines 361–375. At line 364, `bundle_name` is set to `"reload_plugin_v1"`. The bundle version is correctly `"2.0"` (line 365) and the file is correctly `"libreload_plugin_v2.so"` (line 367), but the `bundle_name` mismatch means the identity of the v2 bundle disagrees with its own manifest. Reload tests that check identity by bundle name will fail or produce false results.
**Fix:** Change line 364 from `"bundle_name                = \"reload_plugin_v1\"\n"` to `"bundle_name                = \"reload_plugin_v2\"\n"`.
---

## Category 6 — Error Handling {#category-6}

### Finding 6.1 — `polyplug_runtime_init` silently swallows init error
**File:** `crates/polyplug/src/lib.rs`  
**Line:** ~46 (`return core::ptr::null_mut()`)  
**Problem:** `polyplug_runtime_init` calls `Runtime::builder().build()` and, if it fails, returns `null_mut()` without recording the error anywhere. There is no `polyplug_last_error` path for this failure. The caller receives `null` and has no way to determine why initialization failed: was it a memory error? A config parse error? A loader registration failure? This makes debugging impossible in production.  
**Fix:** Store the error in the thread-local or process-global error slot used by `polyplug_last_error` before returning `null_mut()`. The call site should be: `match Runtime::builder().build() { Ok(r) => Box::into_raw(Box::new(r)), Err(e) => { set_last_error(e); core::ptr::null_mut() } }`.

### Finding 6.2 — `polyplug_runtime_init` ignores the `_config` parameter entirely
**File:** `crates/polyplug/src/lib.rs`  
**Function:** `polyplug_runtime_init`  
**Problem:** The function accepts `_config: *const abi::RuntimeConfig` but prefixes it with `_`, meaning it is intentionally unused. The C caller passes configuration options (log level, loader paths, etc.) that are silently discarded. This is a partial implementation that is not documented as such. Plugin-embedding hosts will pass a config and get a runtime that ignores it, with no warning.  
**Fix:** Either (a) actually read and apply the config, or (b) mark this as `// TODO(epic-N): config is not yet consumed` with a matching GitHub issue reference, and add a `polyplug_warning` call that tells the caller their config was ignored if the config pointer is non-null.

### Finding 6.3 (RETRACTED) — `loader/mod.rs:184` error handling is correct


> **Self-review correction:** Line 184 of `loader/mod.rs` reads:
> ```rust
> std::fs::read_to_string(&manifest_path).map_err(|_e: std::io::Error| {
>     LoaderError::ManifestParse { ... }
> })?;
> ```
> The `_e` is the closure parameter — the error IS consumed by `map_err` and propagated via `?`. The error is not silently swallowed. This finding was incorrect and is retracted.
> **No fix required.**
### Finding 6.4 — `RawManifestDependency::resolve()` silently skips ByBundle deps using `eprintln!`
**File:** `crates/polyplug/src/loader/manifest/mod.rs`  
**Function:** `RawManifestDependency::resolve()`  
**Lines:** 39–48  
**Problem:** When a `ByBundle` dependency has `bundle_id: None`, the code calls `eprintln!("[polyplug] warning: ByBundle dep '{}' has no bundle_id; skipping", ...)` and returns `None`. `eprintln!` is forbidden in library code — it bypasses the runtime's trace system and writes unconditionally to stderr in any embedding context. Returning `None` causes the dependency to be silently skipped, leading to an incomplete dependency graph with no error propagation to the caller.
**Fix:** Remove the `eprintln!` call. Change the return type to `Result<Option<ManifestDependency>, ManifestParseError>` and return `Err(ManifestParseError::MissingField { dep: self.bundle.clone().unwrap_or_default(), field: "bundle_id" })`. Update all callers to handle the `Result`.

### Finding 6.5 — C++ generator emits `{}U` as a literal placeholder in generated code
**File:** `crates/polyplugc/src/generators/cpp/mod.rs`  
**Line:** ~805  
**Problem:** The format string `"        if (!vtable_ || {}U >= vtable_->function_count) {{ polyplug::check_abi_error(AbiError{{4, {{nullptr, 0}}}}); }}\n"` contains `{}U` which is a Rust format placeholder that has no argument. The `{}` will be emitted verbatim into the generated C++ source, producing a C++ syntax error. Any project that uses the C++ generator will get uncompilable output.  
**Fix:** Either (a) provide the missing format argument (the function index as a `u32`), or (b) if the `{}U` is intentionally a C++ unsigned literal for a constant, escape it as `{{}}U` to prevent Rust from treating it as a format placeholder.

### Finding 6.6 — Python generator emits `polyplug.get_extension` without importing `polyplug`
**File:** `crates/polyplugc/src/generators/python/mod.rs`  
**Function:** `generate_init_py` (~line 362)  
**Problem:** The generated `init.py` references `polyplug.get_extension(EXT_TRACE_ID)` but the `polyplug` module is not imported in the generated file. Running the generated Python code will raise `NameError: name 'polyplug' is not defined`.  
**Fix:** In `generate_init_py`, prepend `import polyplug\n` to the generated output before any code that references `polyplug`.

### Finding 6.7 — `polyplugc/src/error/mod.rs` `BundleNameConflict` error message uses `bundle_name` for both fields
**File:** `crates/polyplugc/src/error/mod.rs`  
**Lines:** 29–33  
**Problem:** The `BundleNameConflict` error variant is defined as `BundleNameConflict { bundle_name: String }` (a single field). The error message template (line 29) reads: `"bundle name \"{bundle_name}\" conflicts with contract name \"{bundle_name}\" ..."`. Both format holes use the same `bundle_name` binding. The message claims to show a contract name but instead shows the bundle name twice. The user cannot determine from this error message which contract name caused the conflict.
**Fix:** Either (a) add a `contract_name: String` field to `BundleNameConflict` and update the message to use `{contract_name}` for the second hole, or (b) if the struct intentionally has one field, rewrite the message to accurately say `"bundle name \"{bundle_name}\" conflicts with an existing name — check for duplicate contract or bundle declarations"`. Option (a) is preferred for debuggability.
---

## Category 7 — Performance Regressions {#category-7}

### Finding 7.1 — `contract_id()` allocates a `String` via `format!` on every call
**File:** `crates/polyplug/src/abi/mod.rs`  
**Function:** `contract_id` (~line 356)  
**Problem:** `contract_id()` is called in the hot vtable dispatch path (e.g., during plugin lookup by contract ID). The function calls `format!("{name}@{major}")` to build an intermediate string before hashing it. This allocates on every call. PRD §2 (Performance) states "zero-allocation hot path." Even if contract ID lookups are cached after the first call, any code path that re-derives an ID will hit this allocation.  
**Fix:** Rewrite `contract_id` to hash the bytes of `name` and `major` without constructing an intermediate `String`. Use a stateless FNV-1a loop: hash the bytes of `name`, then hash `b'@'`, then hash the ASCII representation of `major` byte-by-byte. This eliminates the allocation entirely.

### Finding 7.2 — `find_all_by_contract` allocates a `Vec` in the hot call path
*(Already catalogued as Finding 3.1 — the PRD deviation. The performance angle is that this is also a regression.)*  
**Canonical location:** Finding 3.1.  
**Note:** Not duplicated here per audit rules.

### Finding 7.3 — `load_bundle_with` leaks `Box::leak(bundle_dir_str.into_boxed_str())` on every load call (dotnet, python, lua, js-quickjs, js-deno)
**Files:**  
- `crates/polyplug-dotnet/src/lib/mod.rs` line 123  
- `crates/polyplug-python/src/lib/mod.rs` line 191  
- `crates/polyplug-lua/src/lib/loader/mod.rs` line 379  
- `crates/polyplug-js/src/lib/loader/mod.rs` (bundlePath inject)  
- `crates/polyplug-js-deno/src/lib/loader/mod.rs` line 512  
**Problem:** Every `load()` call leaks a `String` containing the bundle directory path via `Box::leak(bundle_dir_str.into_boxed_str())` to create a `'static str` for `PluginContext::bundle_path`. This memory is never freed. If `load()` is called for 100 bundles, 100 strings are leaked. For a long-running process that hot-reloads bundles, this is an unbounded memory leak.  
**Fix:** `PluginContext` must not require `'static` data. Either (a) change `StringView` in `PluginContext` to borrow from the caller for the duration of the `load()` call (the recommended fix — the plugin init function runs synchronously), or (b) if `'static` is genuinely required, maintain a global `Vec<Box<str>>` per-process and push into it rather than leaking. Option (a) is correct: the bundle path only needs to be valid during `polyplug_init`.

---

## Category 8 — AGENTS.md Violations {#category-8}

### Finding 8.1 — `use` statements inside test functions in `abi/mod.rs`
**File:** `crates/polyplug/src/abi/mod.rs`  
**Lines:** ~431–433 (and other test functions; search for `use` inside `fn` blocks within `#[cfg(test)]`)  
**Problem:** AGENTS.md Rule 2: "`use` statements are ONLY allowed at the top of a file." The test module has `use` statements inside individual `#[test]` functions. This violates the rule absolutely. The rule makes no exception for test code.  
**Fix:** Move all `use` statements inside `#[cfg(test)] mod tests { ... }` to the top of the `mod tests` block, not inside individual functions. Example: change `fn test_foo() { use super::*; ... }` to `mod tests { use super::*; fn test_foo() { ... } }`.

### Finding 8.2 — `#[allow(dead_code)]` masking unused code throughout polyplugc
**Files and locations:**  
- `crates/polyplugc/src/error/mod.rs`: `UnsupportedType` variant (line 12)  
- `crates/polyplugc/src/ir/mod.rs`: multiple fields on `ResolvedContract`, `ResolvedPlugin`, `ResolvedBundle`, `ResolvedDependency`  
- `crates/polyplugc/src/parser/mod.rs`: `parse_bundle` (~line 165) and `parse_bundle_str` (~line 175)  
- `crates/polyplugc/src/generators/mod.rs`: `language_name()` method  
- `crates/polyplugc/src/generators/rust/mod.rs`: `GUEST_ALLOCATOR_TEMPLATE` constant  
**Problem:** AGENTS.md Rule 1 (implicitly via Rule 5 "No Implicit Behaviour") and general code quality: `#[allow(dead_code)]` is a suppression that hides the symptom rather than addressing the cause. Dead code either (a) should be deleted, or (b) is genuinely needed but not yet wired up, in which case a `// TODO` comment should replace the `#[allow]`.  
**Fix:** For each instance:
- `UnsupportedType`: either use it in the error-generation path or delete it.
- `ResolvedContract`/`ResolvedPlugin`/`ResolvedBundle`/`ResolvedDependency` dead fields: use them in the generators or delete them. Investigate whether these are planned fields for future epics; if so, mark with `// TODO(epic-N): used in code generation phase`.
- `parse_bundle` / `parse_bundle_str`: either wire these into the CLI or delete them.
- `language_name()`: use it in the CLI `--list-languages` subcommand or delete it from the trait.
- `GUEST_ALLOCATOR_TEMPLATE`: wire into the Rust generator output or delete.

### Finding 8.3 — `RegistryEntry` has `#[allow(dead_code)]`
**File:** `crates/polyplug/src/registry/mod.rs`  
**Line:** ~74  
**Problem:** Same as Finding 8.2 — suppressing dead code instead of fixing it.  
**Fix:** Determine which fields of `RegistryEntry` are unused. Delete unused fields or wire them into the registry's public API. Remove the `#[allow(dead_code)]` attribute.

### Finding 8.4 — `NativeBundleLoader` and `RegistrarState` have `#[allow(dead_code)]`
**File:** `crates/polyplug/src/loader/mod.rs`  
**Problem:** Same as Finding 8.2.  
**Fix:** Wire these types into the loader pipeline or delete them.

### Finding 8.5 — `loader/mod.rs` synthesizes `contract_name` as `"contract_{:#x}"` instead of using `descriptor.contract_name`
**File:** `crates/polyplug/src/loader/mod.rs`  
**Line:** ~448  
**Problem:** The native bundle loader synthesizes the contract name from the contract ID hex value. The `PluginDescriptor` contains a `contract_name` field that the plugin populated during `polyplug_init`. Using the ID-derived hex name instead of the actual name means all lookups by contract name will fail for native plugins. This is a correctness bug: the correct data is available and is ignored. **Note on categorisation:** This is the same class of bug as Finding 3.7 (JS loaders) but in a different file and with a different fix approach — 3.7 requires a `registerVtable` API change, while 8.5 only requires reading the already-available `descriptor.contract_name`. The finding is placed in Category 8 (AGENTS.md) because using a synthesised name where explicit data exists also violates Rule 5 (No Implicit Behaviour).
**Fix:** Use `descriptor.contract_name` (via `unsafe { descriptor.contract_name.as_str() }` with a SAFETY comment noting it comes from a trusted native plugin that signed its bundle per TRUST_MODEL). Replace the hex synthesis entirely.

---

## Fix Plan

The following numbered fixes are ordered by severity and dependency. Each fix is implementable independently unless a dependency is noted. A developer implementing these fixes does not need the audit report present — the file, function, and exact change are specified.

---

### P1 — CRITICAL: Fix C# generator dispatch (Finding 3.6)
**File:** `crates/polyplugc/src/generators/csharp/mod.rs`  
**Change:** Find the code that emits `// call impl\n return AbiError.Ok;` in generated vtable stubs. Replace with generation of actual dispatch code: emit `this.{methodName}({unpackedArgs});` where `{methodName}` is the contract function name and `{unpackedArgs}` is the unpacked argument struct fields. The generated method must also write the return value into the out-pointer before returning.

### P2 — CRITICAL: Fix TRUST_MODEL violation in trace extension (Finding 1.1)
**File:** `crates/polyplug/src/extensions/trace/mod.rs`  
**Change:** Find `from_utf8_unchecked` in `trace_emit_thunk`. Replace with `std::str::from_utf8(bytes).unwrap_or("<invalid utf-8>")`. Remove the invalid `// SAFETY:` comment.

### P3 — CRITICAL: Fix registry `find_all_by_contract` to use caller-provides-buffer (Finding 3.1)
**File:** `crates/polyplug/src/registry/mod.rs`  
**Change:** Change signature to `pub fn find_all_by_contract(&self, contract_id: u64, min_version: u32, out: *mut PluginHandle, capacity: usize) -> usize`. Write into `out`, return count. Update `ffi/mod.rs` callers and any other callers.  
**Depends-on:** Must be implemented before P13. P13 renames the FFI wrapper that calls this function; if P13 runs first, the renamed function will also need a signature update from P3.
### P4 — CRITICAL: Fix version hardcode in graph (Finding 3.2)
**File:** `crates/polyplug/src/graph/mod.rs`  
**Change:** In `ContractCapability::new` and its callers, pass actual version from `PluginDescriptor::version_major`/`version_minor` instead of `Version { major: 1, minor: 0 }`.

### P5 — HIGH: Fix JS loaders synthesizing contract name (Finding 3.7 + AGENTS 8.5)
**Files:** `crates/polyplug-js/src/lib/loader/mod.rs` line 702, `crates/polyplug-js-deno/src/lib/loader/mod.rs` line 696, `crates/polyplug/src/loader/mod.rs` line 448  
**Change:** For JS loaders: extend `registerVtable` to accept a contract name string parameter. For native loader: replace `format!("contract_{:#x}", ...)` with `descriptor.contract_name.as_str()` (with appropriate SAFETY comment per TRUST_MODEL).  
**Overlaps-with:** P17 also modifies `load()` in the JS adapter crates. Apply P5 and P17 together in the same editing pass for each adapter crate to avoid conflicting changes to the same function.
### P6 — HIGH: Fix `build.rs` reload_v2 manifest copy-paste (Finding 5.4)
**File:** `crates/polyplug/build.rs` line ~364  
**Change:** Replace `bundle_name = "reload_plugin_v1"` with `bundle_name = "reload_plugin_v2"`.

### P7 — HIGH: Fix IR dependency contract ID version (Finding 3.5)
**Files:**  
- `crates/polyplugc/src/parser/mod.rs` line 541 **(primary — this is the file named in Finding 3.5's header)**  
- `crates/polyplugc/src/ir/mod.rs` line ~541 **(secondary — also hardcodes major=0)**  
**Change:** In **both files**, replace `compute_contract_id(&dep.contract, 0)` with `compute_contract_id(&dep.contract, dep.min_version.split('.').next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1))`.

### P8 — HIGH: Fix C++ generator format string bug (Finding 6.5)
**File:** `crates/polyplugc/src/generators/cpp/mod.rs` line ~805  
**Change:** Provide the missing format argument for `{}U` (the function index `u32`), or escape as `{{}}U` if the literal `{}` is intentional.

### P9 — HIGH: Fix Python generator missing import (Finding 6.6)
**File:** `crates/polyplugc/src/generators/python/mod.rs` function `generate_init_py` line ~362  
**Change:** Add `out.push_str("import polyplug\n");` before any code that references `polyplug` in the generated output.

### P10 — HIGH: Fix `polyplug_runtime_init` swallowing error (Finding 6.1)
**File:** `crates/polyplug/src/lib.rs` line ~46  
**Change:** Replace `return core::ptr::null_mut()` with `{ set_last_error(e); return core::ptr::null_mut(); }`. Implement `set_last_error` using the same error slot as `polyplug_last_error`.

### P11 — HIGH: Fix `BundleNameConflict` error message template (Finding 6.7)
**File:** `crates/polyplugc/src/error/mod.rs` lines 29–31  
**Change:** Replace the second `{bundle_name}` in the `BundleNameConflict` message with `{contract_name}`. Add `contract_name: String` field to the variant if it does not exist.

### P12 — HIGH: Fix `validate_bundle_compatibility` sentinel values (Finding 5.2)
**File:** `crates/polyplug/src/runtime/mod.rs` function `validate_bundle_compatibility`  
**Change:** When a contract is not found in the registry, return `Err(PolyplugError::PluginNotFound { ... })` or appropriate variant. Do not construct `FunctionCountMismatch { expected: 0, found: 0 }`.

### P13 — MEDIUM: Fix FFI function names to match PRD §25 (Finding 3.3)
**File:** `crates/polyplug/src/ffi/mod.rs`  
**Change:** Rename `polyplug_rt_init` → `polyplug_runtime_init`, `polyplug_rt_load_bundle` → `polyplug_load_bundle`, and all other `polyplug_rt_*` functions to their PRD-specified names. Verify ABI freeze status (v1 not yet released = safe to rename).  
**Depends-on:** Apply after P3. P3 changes the signature of `find_all_by_contract`; P13 renames its FFI wrapper. Applying P13 before P3 requires an extra signature update.
### P14 — MEDIUM: Add missing `polyplug_load_bundle_opts` FFI function (Finding 3.4)
**File:** `crates/polyplug/src/ffi/mod.rs`  
**Change:** Add `#[no_mangle] pub unsafe extern "C" fn polyplug_load_bundle_opts(runtime: *mut Runtime, path: *const c_char, opts: *const BundleLoadOpts) -> AbiError`. Add `BundleLoadOpts` as a `#[repr(C)]` struct to `abi/mod.rs` with the fields from PRD §25.  
**Depends-on:** Apply after P13 so the new function uses the correct naming convention from the start.
### P15 — MEDIUM: Fix panic-unsafe `INIT_BUNDLE_ID` in `load_bundle_with` (Finding 5.3)
**File:** `crates/polyplug/src/runtime/mod.rs` function `load_bundle_with`  
**Change:** Add `struct BundleIdGuard; impl Drop for BundleIdGuard { fn drop(&mut self) { INIT_BUNDLE_ID.store(0, Ordering::Release); } }`. Replace the manual clear-on-return with `let _guard = BundleIdGuard;` after the set.

### P16 — MEDIUM: Fix `contract_id()` allocation (Finding 7.1)
**File:** `crates/polyplug/src/abi/mod.rs` function `contract_id`  
**Change:** Replace `format!("{name}@{major}")` and subsequent hash-from-string with a direct byte-loop that hashes `name.as_bytes()`, then `b'@'`, then the decimal digits of `major`. No allocation.

### P17 — MEDIUM: Fix `Box::leak` per-load memory leak (Finding 7.3)
**Files:** All five adapter crates' `load()` implementations  
**Change:** Change `PluginContext::bundle_path` from requiring `'static` data to borrowing from the caller for the duration of the `load()` call. The `StringView` should hold a pointer valid for the duration of `polyplug_init` execution, not for the process lifetime. Update the `PluginContext` documentation accordingly.  
**Overlaps-with:** P5 also modifies `load()` in the JS adapter crates. Apply P5 and P17 together in the same editing pass for each adapter crate.
### P18 — MEDIUM: Fix `use` statements inside test functions (Finding 8.1)
**File:** `crates/polyplug/src/abi/mod.rs`  
**Change:** In `mod tests { }`, move all `use` statements that are inside individual `#[test]` functions to the top of the `mod tests` block. Remove them from inside function bodies.

### P19 — *(DELETED — Finding 6.3 was retracted: the `_e` binding is a closure parameter consumed by `map_err`; the error is correctly propagated via `?`. No fix required.)*
### P20 — MEDIUM: Fix `RawManifestDependency::resolve()` silent skip (Finding 6.4)
**File:** `crates/polyplug/src/loader/manifest/mod.rs` function `resolve()`  
**Change:** Replace `eprintln!(...); continue;` with `return Err(ManifestError::MissingField { field: "bundle_id" });`.

### P21 — LOW: Fix `unsafe impl Send/Sync` on `Registry` and `RegistryEntry` (Findings 2.1, 2.2)
**File:** `crates/polyplug/src/registry/mod.rs`  
**Change:** Remove manual `unsafe impl Send/Sync` for both types. Verify auto-derive works with `cargo check`. If it does not, add a per-field SAFETY comment identifying exactly which field is `!Send`/`!Sync`.

### P22 — LOW: Fix `Runtime` unsafe impl SAFETY comment (Finding 2.3)
**File:** `crates/polyplug/src/runtime/mod.rs`  
**Change:** Replace the generic `// SAFETY:` comment with a per-field explanation identifying each `!Send`/`!Sync` field and the synchronisation mechanism that makes Send/Sync sound.

### P23 — LOW: Fix `HostVtablePtr` SAFETY comment (Finding 2.4)
**File:** `crates/polyplug-js/src/lib/loader/mod.rs` lines 59–62  
**Change:** Update the SAFETY comment as specified in Finding 2.4.

### P24 — LOW: Fix `Box::leak` missing comment in `runtime/mod.rs` (Finding 1.2)
**File:** `crates/polyplug/src/runtime/mod.rs` line ~259  
**Change:** Add `// SAFETY: HostVTable must have 'static lifetime ...` comment as specified in Finding 1.2.

### P25 — LOW: Fix unsafe cast site missing SAFETY comment in `js-deno` (Finding 1.3)
**File:** `crates/polyplug-js-deno/src/lib/loader/mod.rs` line ~492  
**Change:** Add the SAFETY comment at the cast site as specified in Finding 1.3.

### P26 — LOW: Consolidate duplicated generator utility functions (Finding 4.4, 4.5, 4.6, 4.7)
**Files:** All generator files under `crates/polyplugc/src/generators/`  
**Change:** Move `contract_name_to_struct`, `arg_pack_struct_name`, `needs_arg_pack`, `to_snake_case`, `contract_to_class_name`, `substitute_variant_refs` into `generators/mod.rs` as `pub(crate)` functions. Delete all per-generator copies. Update imports in each generator.  
**Overlaps-with:** P29 also modifies generator files (renames `cpp_name()` in `ir/mod.rs` and all callers in generators). Apply P26 and P29 in the same editing session to avoid conflicting generator file changes.
### P27 — LOW: Remove redundant `Registry::find` and `Registry::resolve` wrappers (Findings 4.1, 4.2)
**File:** `crates/polyplug/src/registry/mod.rs`  
**Change:** Verify with `lsp_find_references` that neither method has callers. Delete both methods.

### P28 — LOW: Deduplicate `fnv1a_64` in `polyplugc/src/main.rs` (Finding 4.3)
**File:** `crates/polyplugc/src/main.rs`  
**Change:** Remove the local `fnv1a_64` function from `main.rs`. Expose the existing implementation in `crates/polyplugc/src/ir/mod.rs` as `pub(crate) fn fnv1a_64(data: &[u8]) -> u64` (it is already used there for `compute_contract_id`). In `main.rs`, add `use crate::ir::fnv1a_64;`. Do **not** import from `polyplug::abi` — `polyplugc` is a separate binary crate; the intra-crate reference `crate::ir::fnv1a_64` is correct.
### P29 — LOW: Rename `cpp_name()` to `c_ffi_name()` (Finding 5.1)
**Files:** `crates/polyplugc/src/ir/mod.rs` (method definition), all callers in generators  
**Change:** Rename `cpp_name()` to `c_ffi_name()`. Update all callers.

### P30 — LOW: Resolve all `#[allow(dead_code)]` suppressions (Finding 8.2, 8.3, 8.4)
**Files:** Multiple (see Finding 8.2)  
**Change:** For each suppressed dead item: delete it if unused, or wire it into active code and remove the allow attribute. Do not leave `#[allow(dead_code)]` in the codebase.

### P31 — LOW: Remove duplicate `cargo:rerun-if-changed` in `build.rs` (Finding 4.8)
**File:** `crates/polyplug/build.rs` line ~27  
**Change:** Delete the duplicate `println!("cargo:rerun-if-changed=tests/fixtures/test_plugin/src/lib.rs")` line.

### P32 — LOW: Add `polyplug_load_bundle_opts` and document `_config` non-use (Finding 6.2)
**File:** `crates/polyplug/src/lib.rs`  
**Change:** Either consume `_config` or add a `polyplug_warning` call and a `// TODO(epic-N):` comment.

---

## Summary Table

| # | Category | Severity | File | Finding |
|---|----------|----------|------|---------|
| 1.1 | Unsafe | CRITICAL | `extensions/trace/mod.rs` | `from_utf8_unchecked` on plugin ABI data — TRUST_MODEL violation |
| 1.2 | Unsafe | LOW | `runtime/mod.rs` | `Box::leak` without SAFETY comment |
| 1.3 | Unsafe | LOW | `polyplug-js-deno/loader/mod.rs` | Raw pointer cast site missing SAFETY comment |
| 2.1 | Unsafe impl | LOW | `registry/mod.rs` | `unsafe impl Send/Sync for Registry` likely unnecessary |
| 2.2 | Unsafe impl | LOW | `registry/mod.rs` | `unsafe impl Send/Sync for RegistryEntry` bypasses auto-derive |
| 2.3 | Unsafe impl | MEDIUM | `runtime/mod.rs` | `Runtime` unsafe impl lacks per-field justification |
| 2.4 | Unsafe impl | LOW | `polyplug-js/loader/mod.rs` | `HostVtablePtr` SAFETY comment insufficient |
| 3.1 | PRD | CRITICAL | `registry/mod.rs` | `find_all_by_contract` returns `Vec` — violates no-allocation ABI |
| 3.2 | PRD | CRITICAL | `graph/mod.rs` | All capabilities hardcode `Version { major: 1, minor: 0 }` |
| 3.3 | PRD | MEDIUM | `ffi/mod.rs` | Function names use `_rt_` infix; PRD §25 specifies different names |
| 3.4 | PRD | MEDIUM | `ffi/mod.rs` | `polyplug_load_bundle_opts` missing from C API |
| 3.5 | PRD | HIGH | `polyplugc/parser/mod.rs` (primary), `polyplugc/ir/mod.rs` (secondary) | Dependency contract ID computed with hardcoded `major = 0` in both files |
| 3.6 | PRD | CRITICAL | `polyplugc/generators/csharp/mod.rs` | C# ABI stubs never call implementation — always return Ok |
| 3.7 | PRD | HIGH | `polyplug-js/loader/mod.rs`, `polyplug-js-deno/loader/mod.rs` | Contract name synthesized as hex, not from manifest |
| 4.1 | Redundancy | LOW | `registry/mod.rs` | `find()` is dead alias for `find_by_contract()` |
| 4.2 | Redundancy | LOW | `registry/mod.rs` | `resolve()` is dead alias for `resolve_guard()` |
| 4.3 | Redundancy | LOW | `polyplugc/main.rs` | `fnv1a_64` reimplemented; should use `crate::ir::fnv1a_64` (intra-crate, not `polyplug::abi`) |
| 4.4 | Redundancy | LOW | All generators | `contract_name_to_struct` and friends duplicated 5–6 times |
| 4.5 | Redundancy | LOW | `rust/mod.rs`, `python/mod.rs` | `to_snake_case` duplicated |
| 4.6 | Redundancy | LOW | `js_quickjs/mod.rs`, `js_deno/mod.rs` | `contract_to_class_name` duplicated |
| 4.7 | Redundancy | LOW | `js_quickjs/mod.rs`, `js_deno/mod.rs` | `substitute_variant_refs` duplicated |
| 4.8 | Redundancy | LOW | `polyplug/build.rs` | Duplicate `cargo:rerun-if-changed` emit |
| 5.1 | Concepts | LOW | `polyplugc/ir/mod.rs`, generators | `cpp_name()` used for C FFI types — misleading name |
| 5.2 | Concepts | MEDIUM | `runtime/mod.rs` | `FunctionCountMismatch { 0, 0 }` used as sentinel for missing contract |
| 5.3 | Concepts | MEDIUM | `runtime/mod.rs` | `INIT_BUNDLE_ID` set without RAII guard — panic-unsafe |
| 5.4 | Concepts | HIGH | `polyplug/build.rs:364` | `reload_v2` manifest has `bundle_name = "reload_plugin_v1"` |
| 6.1 | Error | HIGH | `lib.rs` | `polyplug_runtime_init` silently returns null without recording error |
| 6.2 | Error | MEDIUM | `lib.rs` | `_config` parameter completely ignored with no diagnostic |
| ~~6.3~~ | ~~Error~~ | ~~HIGH~~ | ~~`loader/mod.rs:~184`~~ | **RETRACTED** — `_e` is a closure parameter consumed by `map_err`; error is correctly propagated |
| 6.4 | Error | HIGH | `loader/manifest/mod.rs` | ByBundle dep with missing `bundle_id` uses `eprintln!` and continues |
| 6.5 | Error | HIGH | `polyplugc/generators/cpp/mod.rs:~805` | `{}U` format placeholder emitted verbatim in generated C++ |
| 6.6 | Error | HIGH | `polyplugc/generators/python/mod.rs:~362` | Generated `init.py` references `polyplug` without importing it |
| 6.7 | Error | HIGH | `polyplugc/error/mod.rs:29–31` | `BundleNameConflict` shows bundle name twice, never shows contract name |
| 7.1 | Perf | MEDIUM | `abi/mod.rs` | `contract_id()` allocates `String` via `format!` on every call |
| 7.3 | Perf | MEDIUM | All adapter crate `load()` impls | `Box::leak` per load call — unbounded memory leak on hot-reload |
| 8.1 | AGENTS | MEDIUM | `abi/mod.rs` | `use` inside test functions violates AGENTS.md Rule 2 |
| 8.2 | AGENTS | LOW | Multiple `polyplugc` files | `#[allow(dead_code)]` suppressing unused items instead of fixing them |
| 8.3 | AGENTS | LOW | `registry/mod.rs` | `#[allow(dead_code)]` on `RegistryEntry` |
| 8.4 | AGENTS | LOW | `loader/mod.rs` | `#[allow(dead_code)]` on `NativeBundleLoader` and `RegistrarState` |
| 8.5 | AGENTS | HIGH | `loader/mod.rs:~448` | Contract name synthesized as hex instead of using `descriptor.contract_name` |

---

## Finding Count by Severity

| Severity | Count |
|----------|-------|
| CRITICAL | 5 |
| HIGH | 10 *(was 11; Finding 6.3 retracted)* |
| MEDIUM | 9 *(was 10; P19 deleted as it referenced the retracted Finding 6.3)* |
| LOW | 16 |
| **Total active** | **40** *(41 numbered findings minus 1 retracted = 40 active)* |

---

I have read every file listed in Audit Scope in full.
