# Decisions — codegen-implementation

## 2026-03-08T11:20:19Z — Pre-execution decisions

### Task 1: TOML fixture format
- `test_api.toml`: params use inline array `params = [{ name = "args", type = "AddArgs" }]`
- `test_api.toml`: `return = "u32"` scalar (NOT `[[contracts.functions.returns]]`)
- The parser struct `RawParam` already has `ty` field renamed from `type`
- `[[types.fields]]` is already wrong too — parser expects `fields = [...]` inline array in `RawType`
- Must convert to rich test_api.toml with multiple functions per plan
- `test_bundle.toml`: change `[[plugins]]` → `[[plugin]]`

### Task 2: PluginError shape
- `struct PluginError { pub code: u32, pub message: String }`
- NOT repr(C), NOT ABI type — Rust-only error for plugin trait implementations
- Add Display impl
- Place at end of `guest-libs/rust/src/lib/mod.rs`

### Task 3: PolyplugException
- `host-libs/cpp/polyplug/error.hpp` already has `PluginError` class
- Plan wants `PolyplugException : public std::runtime_error` added
- AND `check_abi_error()` helper (different from existing `throw_if_error()`)
- Must add these ALONGSIDE existing content — DO NOT remove existing `PluginError` class

### Task 4: panicking_fn
- `polyplug_panicking_fn` in test_plugin NOT added to vtable
- C++ plugin compiled via `g++` in build.rs using `std::process::Command` — no cc crate
- `TEST_PLUGIN_CPP_SO` env var set, empty string if g++ unavailable

### Generator output files
- Rust host: `types.rs`, `host_callers.rs`, `manifest.toml` (3 files)
- Rust guest: `types.rs`, `contracts.rs`, `vtables.rs`, `init.rs`, `manifest.toml` (5 files)
- C++ host: `types.hpp`, `host_callers.hpp`, `manifest.toml` (3 files)
- C++ guest: `types.hpp`, `contracts.hpp`, `vtables.hpp`, `init.hpp`, `manifest.toml` (5 files)

### CLI dispatch
- `--api` flag → call BOTH generate_host() AND generate_guest()
- `--bundle` flag → call generate_guest() ONLY
- Track `from_api: bool = api.is_some()` BEFORE consuming api/bundle

### ABI errors in guest-libs re-exports
- Must add `ABI_ERROR_GENERIC` and `ABI_ERROR_PANIC` re-exports to guest-libs
- OR use numeric values 1 and 3 in generated code with comments
- Decision: re-export them from guest-libs (cleaner)
