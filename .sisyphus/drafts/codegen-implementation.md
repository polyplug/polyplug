# Draft: polyplugc Codegen Implementation

## Research Findings (confirmed)

### Current generator stub state
- `rust/mod.rs`: Has `generate_host` (emits struct + caller stubs with `todo!()`) and `generate_guest` (emits allocator hookup + TODO comments). Both are stubs — no real ABI generation.
- `cpp/mod.rs`: Same pattern — host emits type declarations and class stubs; guest emits operator new/delete override + TODO comments. Both stubs.
- `main.rs`: `generate` command calls `generate_host()` only — **`generate_guest()` is never called** by the CLI. `validate` command IS wired and working.
- `generators/mod.rs`: `generate_guest()` has `#[allow(dead_code)]` — confirms not called yet.

### Critical TOML schema mismatch discovered
- `test_api.toml` uses `[[types]]` and `[[contracts]]` (plural)
- `parser/mod.rs` expects `[[type]]` and `[[contract]]` (singular, per PRD sample)
- This is a fixture bug, not a parser bug. The REAL api.toml format is `[[type]]` and `[[contract]]` per PRD §10.
- The integration plan must use the CORRECT format in new fixture files.

### Runtime architecture (confirmed)
- `PluginVTable.functions` is `*const *const ()` — array of opaque fn ptrs indexed by `function_id`
- Each ABI function signature: `extern "C" fn(*const (), *mut ()) -> AbiError`
- `polyplug_init` symbol: `extern "C" fn(*mut PluginRegistrar) -> AbiError`
- `polyplug_abi_version` symbol: `extern "C" fn() -> u32`
- Non-primitive args passed as struct pointer (`*const MyStruct`) cast to `*const ()`
- Non-primitive returns via out-param (`*mut MyStruct`) cast to `*mut ()`
- Primitives returned directly — but wait: the ABI signature is ALWAYS `fn(*const (), *mut ()) -> AbiError`. So primitives are ALSO returned via out-param at the ABI level; the caller-provides-buffer is hidden by generated wrapper code that makes it look like a direct return.

### Naming convention (observed from existing code)
- `contract_name_to_struct("image.decode") → "ImageDecodeContract"` (Rust)
- `contract_name_to_class("image.decode") → "ImageDecodeContract"` (C++)
- Both already implemented in their respective generators

### CLI wire status
- `generate_host()` IS called. `generate_guest()` is NOT. The CLI does NOT distinguish `--api` (host-side) from `--bundle` (guest-side) for the generate_host/generate_guest dispatch.
- This needs fixing: `--api` → `generate_host()`, `--bundle` → `generate_guest()`

### C++ error handling
- PRD §15: "C++ — expected or exception (app developer chooses at codegen time)"
- Existing C++ generator: uses no error handling at all (stubs return `void`)
- guest-libs/cpp/guest.hpp: uses exceptions (`throw std::bad_alloc`)
- QUESTION: std::expected, exceptions, or error codes for the C++ host callers?

### Rust guest trait error type
- PRD §15: `fn compute(&self, image: &Image) -> Result<Stats, PluginError>`
- QUESTION: Is `PluginError` a generated type (per-contract) or a single type from polyplug_guest?

### Test infrastructure
- Existing tests: Rust only, hand-written (integration_dispatch, integration_load, integration_graph)
- `build.rs` compiles `test_plugin` as cdylib and exports `TEST_PLUGIN_SO` env var
- C++ tests: NO existing C++ test infrastructure at all (no CMakeLists.txt, no test harness)
- QUESTION: For C++ integration tests — use `cc` crate in build.rs to compile C++ plugin? Or separate CMake? Or skip C++ compile-and-load tests, only test codegen output?

### manifest.toml generation
- PRD §10 specifies exact format:
  ```toml
  name     = "image_bundle"
  version  = "1.0"
  file     = "image_bundle.so"
  provides = ["image.decode@1.0", "image.stats@1.0"]
  requires = ["image.decode@1.0"]
  ```
- No manifest generator exists yet. Must be added.

## Open Questions (to be resolved by user)

1. **Rust guest trait error type**: Should guest traits use `Result<T, PluginError>` where `PluginError` is a generated enum per contract, a single struct from polyplug_guest, or something else?

2. **C++ error handling**: std::expected<T,E>, exceptions (C++ throws, callers catch), or C-style error codes returned as AbiError directly?

3. **C++ integration test strategy**: (a) Use `cc` crate in Rust build.rs to compile a C++ test plugin, load it in Rust tests; (b) pure codegen validation only (no compile+load for C++); (c) separate shell-script based test?

4. **Multiple output files**: The PRD describes `init.rs`, `vtables.rs`, `contracts.rs`, `types.rs` as separate files. Should the generator produce multiple files, or one combined file?

5. **Panic isolation test**: Can we test panic isolation purely in Rust (compile a plugin with panic, catch it) or does this require a separate cdylib fixture?

## Scope Boundaries
- IN: Rust generator (host + guest), C++ generator (host + guest), CLI `--api`/`--bundle` dispatch fix, manifest.toml generation, integration tests
- OUT: C#, Python, Lua generators, hot reload, WASM, extension system implementation
