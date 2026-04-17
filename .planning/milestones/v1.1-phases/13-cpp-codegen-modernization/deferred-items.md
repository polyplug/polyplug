# Deferred Items - Phase 13-02

## Out-of-Scope Issues Found During Execution

### 1. C++ SDK abi.hpp Contains Invalid Placeholder Syntax

**File:** `sdks/cpp/abi/polyplug/abi.hpp`
**Issue:** Lines 6-14 contain placeholder Rust-like syntax (`&[u8]`, `&str`) instead of valid C++.
**Impact:** smoke_cpp_codegen_dispatch test fails when attempting to compile generated C++ code.
**Recommendation:** Implement actual C++ constexpr functions for:
  - `fnv1a_64(std::span<const uint8_t> data)`
  - `contract_id(std::string_view name, uint32_t major)`
  - `bundle_id(std::string_view name)`
  - `host_contract_id(std::string_view name, uint32_t major)`
  - `plugin_contract_id(std::string_view name, uint32_t major)`
**Not Fixed:** Out of scope - pre-existing stub, not caused by this plan's changes.

---
*Documented: 2026-04-08*