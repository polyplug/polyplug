# Learnings: Hot-Reload Notification FFI

## 2026-03-19: FFI Layer Implementation

### Pattern: C-compatible tagged union for enums with data
- Use `#[repr(u32)]` enum for type tags
- Create a `#[repr(C)]` struct with all possible fields
- Include a `phase_type` field to indicate which variant is active
- Document which fields are valid for which variant

### Pattern: String passing across FFI
- Use `StringViewC { ptr: *const u8, len: usize }` for borrowed strings
- Document memory safety: strings are borrowed, callback must NOT free
- Create helper `from_str()` for conversion

### Pattern: Global configuration storage
- Use `AtomicPtr<()>` for function pointers (null = not set)
- Use `OnceLock<T>` for config structs
- Allow overwriting by ignoring `OnceLock::set` error (matches `GLOBAL_WARNING_CB` pattern)

### Pattern: Callback wrapper
- Store C callback in global static
- Create Rust wrapper that reads global, transmutes, and invokes
- Use `// SAFETY:` comment to justify unsafe transmute

### Pattern: Pre-build configuration
- FFI functions store config/callback in globals
- `polyplug_runtime_create` reads globals and applies to builder
- This allows configuration before runtime exists

### Clippy: `to_*` methods on Copy types
- Use `into_*` naming and take `self` by value
- Or use different naming like `as_*` or `from_*`