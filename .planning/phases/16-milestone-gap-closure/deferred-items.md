# Deferred Items from Phase 16-05

## Pre-existing Test Infrastructure Issues

### 1. C++ SDK ABI Syntax Error

**File:** `sdks/cpp/abi/polyplug/abi.hpp`
**Issue:** Rust-style syntax (`&[u8]`, `&str`) used in C++ file
**Error:** Compilation fails with `'u8' was not declared in this scope`
**Origin:** Phase 02 (refactor(02-01,02-02): registry simplification with typed IDs - commit 7536fd1)
**Impact:** `smoke_cpp_codegen_dispatch` test fails
**Resolution needed:** Fix C++ syntax to use proper C++ types (e.g., `std::span<uint8_t>`, `std::string_view`)

### 2. Test Plugin Binaries Missing

**Files:** Test plugins in `crates/polyplug/tests/*.so`
**Issue:** Test plugins not built, environment variables `TEST_PLUGIN_DIR`, `RELOAD_PLUGIN_V1_DIR` not set
**Impact:** FFI edge case tests fail (`test_find_all_by_contract_overflow`, etc.)
**Resolution needed:** Build test plugins via justfile or cargo build step

### 3. polyplug_lua and polyplug_js Tests

**Issue:** Private struct `RuntimeBuilder` not accessible, unresolved imports
**Impact:** `lua_loader` and `quickjs_loader` tests fail compilation
**Resolution needed:** Fix imports or make RuntimeBuilder public

---

**Note:** These issues are pre-existing and NOT caused by Phase 16 documentation/comment changes.
Phase 16 was purely documentation updates (VTable → Interface terminology in comments, REQUIREMENTS.md checkbox fixes).

**Verified tests that pass:**
- polyplug_abi: 59 passed
- polyplug_codegen: 2 passed
- polyplugc smoke tests (partial): smoke_rust_codegen_dispatch passes