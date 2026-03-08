# Issues — codegen-implementation

## 2026-03-08T11:20:19Z — Known Issues Before Execution

### test_api.toml format bugs
- `[[contracts]]` → must be `[[contract]]`
- `[[contracts.functions.params]]` → must be inline `params = [{...}]`  
- `[[contracts.functions.returns]]` → must be `return = "u32"` scalar
- `[[types.fields]]` → must be inline `fields = [{...}]`
- NOTE: `RawType.fields` expects `Vec<RawField>` from inline array notation

### test_bundle.toml format bugs
- `[[plugins]]` → must be `[[plugin]]`

### Rust generator stub issues
- `generate_guest()` has `#[allow(dead_code)]` comment in `generators/mod.rs:36`
- CLI only calls `generate_host()` — never calls `generate_guest()`

### host-libs/cpp/polyplug/error.hpp
- Already has `PluginError` class (different from what plan wants)
- Plan wants `PolyplugException` class added WITH `check_abi_error()` function
- Must add both alongside existing content

### Guest lib missing re-exports
- `ABI_ERROR_GENERIC` and `ABI_ERROR_PANIC` not re-exported from guest-libs
- Generated vtables.rs needs access to these constants

### PluginVTable.functions type mismatch
- In `polyplug_runtime::abi`: `functions: *const *const ()` 
- In `test_plugin`: `functions: *const FnPtr` (local FnPtr type)
- Generated code must use `polyplug_guest::PluginVTable` type — which uses `*const *const ()`
- But `FnPtr` wrapper is needed for Sync — must define FnPtr IN generated vtables.rs
- Then cast: `TEST_FNS.as_ptr() as *const *const ()`
