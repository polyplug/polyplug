# Fix polyplug ABI, codegen, and guest Compilation Issues

## TL;DR

Fix compilation errors and warnings in the polyplug codebase:
1. Missing constant exports from `polyplug_abi`
2. Missing imports in `polyplug_guest`
3. Missing `parser` module in `polyplug_codegen` tests
4. Clippy warning in `csharp.rs`
5. Invalid syntax in C# SDK `Abi.cs`

## Context

The polyplug project has multiple crates that need to compile cleanly:
- `polyplug_abi` - ABI type definitions
- `polyplug_guest` - Guest library for plugin authors
- `polyplug_codegen` - Code generation library

Current issues identified from build logs:

### Issue 1: `polyplug_guest` - Missing `ABI_ERROR_GENERIC`
```
error[E0425]: cannot find value `ABI_ERROR_GENERIC` in this scope
   --> crates/polyplug_guest/src/lib.rs:291:19
```

The code uses `ABI_ERROR_GENERIC` but it doesn't exist. Should use `AbiErrorCode::Generic as u32`.

### Issue 2: Missing exports from `polyplug_abi`
```
error[E0432]: unresolved import `polyplug_abi::ABI_OK`
error[E0432]: unresolved import `polyplug_abi::contract_id`
error[E0432]: unresolved import `polyplug_abi::host_contract_id`
```

Tests are trying to import constants that don't exist. Need to add re-exports from `polyplug_utils` or define these in `polyplug_abi`.

### Issue 3: `polyplug_codegen` - Missing modules
```
error[E0432]: unresolved import `polyplug_codegen::parser`
error[E0432]: unresolved import `polyplug_codegen::generate`
```

Test files are trying to import `parser` and `generate` modules that don't exist in `polyplug_codegen`.

### Issue 4: `polyplug_codegen` - Clippy warning
```
error: stripping a prefix manually
  --> crates/polyplug_codegen/src/languages/csharp.rs:32:31
```

Need to use `strip_prefix` instead of manual slicing.

### Issue 5: C# SDK - Invalid syntax in `Abi.cs`
```
/mnt/data/Projects/Utils/polyplug/sdks/csharp/abi/Abi.cs(346,1): error CS8803: Top-level statements must precede namespace and type declarations.
```

Lines 346-378 have invalid C# syntax like:
- `public static StringView string_view_from_static('static[u8] bytes);` - invalid type syntax
- Multiple invalid function declarations outside a namespace/class

## Work Objectives

1. Fix all compilation errors in `polyplug_abi`, `polyplug_guest`, and `polyplug_codegen`
2. Fix all clippy warnings (`cargo clippy -- -D warnings`)
3. Ensure all tests pass (`cargo test`)

## Execution Strategy

### Wave 1: Fix Core ABI Issues (can run in parallel)

- [ ] 1. Fix `polyplug_abi` - Add missing re-exports
  **What to do**: Add `ABI_OK` constant and re-export hash functions from `polyplug_utils` in `polyplug_abi/src/lib.rs`
  **Changes needed**:
  - Add `pub const ABI_OK: u32 = 0;`
  - Add `pub use polyplug_utils::{contract_id, host_contract_id, bundle_id};`
  - Update existing imports to use these

- [ ] 2. Fix `polyplug_guest` - Replace undefined constant
  **What to do**: In `crates/polyplug_guest/src/lib.rs` line 291, replace `ABI_ERROR_GENERIC` with `AbiErrorCode::Generic as u32`
  **Must also**: Add import: `pub use polyplug_abi::AbiErrorCode;`

### Wave 2: Fix Codegen Issues (depends on Wave 1)

- [ ] 3. Fix `polyplug_codegen` - Clippy warning in csharp.rs
  **What to do**: In `crates/polyplug_codegen/src/languages/csharp.rs` line 31-32:
  ```rust
  // Change from:
  if rust_type.starts_with('&') {
      let inner: &str = &rust_type[1..];
  // To:
  if let Some(inner) = rust_type.strip_prefix('&') {
  ```

- [ ] 4. Fix `polyplug_codegen` - Add missing modules for tests
  **What to do**: The test files import `polyplug_codegen::parser` and `polyplug_codegen::generate` but these don't exist.
  **Options**:
  a) Add `pub mod parser;` and export the `generate` function in `lib.rs`
  b) Fix test imports to use correct module paths
  **Decision**: Check if these modules/functions exist under different names and update tests accordingly

### Wave 3: Fix C# SDK (independent)

- [ ] 5. Fix C# SDK `Abi.cs` syntax errors
  **What to do**: The file has lines 346-378 with invalid C# syntax. These appear to be:
  - Rust type signatures that weren't translated to C#
  - Function declarations outside a class
  **Fix**: Comment out or remove these invalid declarations, or wrap them in a proper class structure

### Wave 4: Verification

- [ ] 6. Verify `cargo build` passes
  **Command**: `cargo build`

- [ ] 7. Verify `cargo test` passes
  **Command**: `cargo test`

- [ ] 8. Verify `cargo clippy -- -D warnings` passes
  **Command**: `cargo clippy -- -D warnings`

## Commit Strategy

Single commit:
```
fix: resolve compilation errors and warnings in polyplug

- Add missing ABI_OK and hash function re-exports to polyplug_abi
- Fix undefined ABI_ERROR_GENERIC in polyplug_guest
- Fix manual_strip clippy warning in csharp.rs
- Fix invalid syntax in C# SDK Abi.cs
- Add missing module exports for polyplug_codegen tests
```

## Success Criteria

All commands must pass with zero errors/warnings:
```bash
cargo build
cargo test
cargo clippy -- -D warnings
```
