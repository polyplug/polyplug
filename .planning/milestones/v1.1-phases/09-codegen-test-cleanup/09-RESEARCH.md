# Phase 09: Codegen Test Cleanup - Research

**Researched:** 2026-04-06
**Domain:** Test file naming alignment with codegen output
**Confidence:** HIGH

## Summary

The generators (`polyplugc`) correctly produce `interfaces.*` files (not `vtables.*`), but two key test files still expect the old naming convention. The smoke.rs and integration_codegen_cpp.rs tests reference `vtables.*` in their expected file lists and handwritten lib.rs template code, which does not match the current generator output. Additionally, stale generated `vtables.*` files exist in example directories that need cleanup.

**Primary recommendation:** Update smoke.rs and integration_codegen_cpp.rs to expect `interfaces.*` naming, delete stale vtables.* files from examples, and verify both Rust and C++ codegen E2E flows pass.

## User Constraints (from CONTEXT.md)

> No CONTEXT.md exists for this phase. Requirements from ROADMAP.md:

**Locked Decisions:**
- Requirements: CLN-01, CLN-04, SDK-05
- Success criteria defined:
  1. smoke.rs references interfaces.* not vtables.*
  2. Handwritten lib.rs imports guest::interfaces
  3. C++ codegen E2E flow passes
  4. Rust codegen E2E flow passes

**Claude's Discretion:** Research focus areas and implementation approach

**Deferred Ideas:** None identified for this phase

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CLN-01 | Remove all "vtable" naming from codebase | Stale files identified, test files need update |
| CLN-04 | Update tests to use new instance model | smoke.rs and integration_codegen_cpp.rs need vtable→interface rename |
| SDK-05 | Update JS SDK - use types from polyplug_abi | Stale JS vtable.ts files need deletion |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| polyplugc | workspace | Codegen CLI | Generates interfaces.* files correctly |

### Test Infrastructure
| Tool | Version | Purpose | Availability |
|------|---------|---------|--------------|
| cargo | 1.94.0 | Rust test runner | Available |
| g++ | 15.2.1 | C++ compilation test | Available |

## Files Requiring Modification

### Test Files with vtables.* References

| File | Line(s) | Current Pattern | Required Change |
|------|---------|-----------------|-----------------|
| `crates/polyplugc/tests/smoke.rs` | 110 | `pub mod vtables;` | `pub mod interfaces;` |
| `crates/polyplugc/tests/smoke.rs` | 125 | `use guest::vtables::TEST_ADDER_VTABLE;` | `use guest::interfaces::TEST_ADDER_VTABLE;` |
| `crates/polyplugc/tests/smoke.rs` | 126 | `use guest::vtables::set_test_adder_impl;` | `use guest::interfaces::set_test_adder_impl;` |
| `crates/polyplugc/tests/smoke.rs` | 463 | `"guest/vtables.hpp"` | `"guest/interfaces.hpp"` |
| `crates/polyplugc/tests/smoke.rs` | 490 | `vtables_hpp` variable | `interfaces_hpp` variable |
| `crates/polyplug/tests/integration_codegen_cpp.rs` | 222-225 | `"guest/vtables.hpp"` in expected_files | `"guest/interfaces.hpp"` |
| `crates/polyplug/tests/integration_codegen_cpp.rs` | 248-271 | `vtables_hpp` references | `interfaces_hpp` references |

### Stale Files to Delete

**C++ examples (5 files):**
```
examples/guests/cpp/decoder/generated/guest/vtables.hpp
examples/guests/cpp/encoder/generated/guest/vtables.hpp
examples/guests/cpp/reporter/generated/guest/vtables.hpp
examples/guests/cpp/transformer/generated/guest/vtables.hpp
examples/guests/cpp/validator/generated/guest/vtables.hpp
```

**JS examples (5 files):**
```
examples/guests/js/validator/generated/guest/vtable.ts
examples/guests/js/transformer/generated/guest/vtable.ts
examples/guests/js/reporter/generated/guest/vtable.ts
examples/guests/js/encoder/generated/guest/vtable.ts
examples/guests/js/decoder/generated/guest/vtable.ts
```

**Old host factories (may exist):**
```
examples/hosts/js/generated/host/vtable_factories.ts
examples/hosts/lua/generated/host/vtable_factories.lua
examples/hosts/csharp/generated/host/VTableFactories.cs
examples/hosts/cpp/generated/host/vtable_factories.hpp
examples/hosts/rust/generated/host/vtable_factories.rs
```

### Already Correct Files (No Changes Needed)

| File | Status | Evidence |
|------|--------|----------|
| `crates/polyplugc/tests/integration_codegen_rust.rs` | Correct | Uses `pub mod interfaces;` (line 107), imports from `guest::interfaces` |
| `crates/polyplugc/tests/generator_correctness.rs` | Correct | Helper `generate_guest_vtables` returns `interfaces.rs` |
| `examples/guests/rust/*/src/lib.rs` (5 files) | Correct | Import from `generated::interfaces::set_*_impl` |
| `examples/guests/rust/*/generated/guest/mod.rs` (10 files) | Correct | Contains `pub mod interfaces;` |
| `crates/polyplugc/src/generators/*.rs` | Correct | All generators output `interfaces.*` files |

## Generator Output Verification

### Rust Generator Output (VERIFIED)
From `crates/polyplugc/src/generators/rust.rs`:
- Line 273: `path: std::path::PathBuf::from("guest/interfaces.rs")`
- Line 2719: `out.push_str("pub mod interfaces;\n");`

### C++ Generator Output (VERIFIED)
From `crates/polyplugc/src/generators/cpp.rs`:
- Line 110: `path: std::path::PathBuf::from("guest/interfaces.hpp")`
- Line 635: `out.push_str("#include \"interfaces.hpp\"\n");`

## Architecture Patterns

### Current Correct Pattern (from examples)

Handwritten lib.rs should use:
```rust
// THIS IS THE CORRECT PATTERN (examples/guests/rust/validator/src/lib.rs)
#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineValidatorPlugin;
use generated::interfaces::set_validator_impl;
```

Generated mod.rs content (correct):
```rust
// examples/guests/rust/validator/src/generated/guest/mod.rs
pub mod contracts;
pub mod init;
pub mod types;
pub mod interfaces;
pub mod host_contract_callers;
```

### Incorrect Pattern (smoke.rs)

Current smoke.rs lib.rs template (WRONG):
```rust
mod guest {
    pub mod types;
    pub mod contracts;
    pub mod vtables;  // WRONG - should be interfaces
}

use guest::vtables::TEST_ADDER_VTABLE;  // WRONG
use guest::vtables::set_test_adder_impl;  // WRONG
```

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Test file content | Manual lib.rs template | Copy from integration_codegen_rust.rs | That test already has correct pattern |

## Common Pitfalls

### Pitfall 1: Incomplete Stale File Cleanup
**What goes wrong:** Deleting vtables.* in examples but leaving them in hosts
**Why it happens:** Separate directories with similar naming patterns
**How to avoid:** Grep for ALL vtables.* occurrences before cleanup
**Warning signs:** Build fails with "cannot find vtables.hpp" after changes

### Pitfall 2: Variable Name Not Updated
**What goes wrong:** Changing file reference but keeping `vtables_hpp` variable name
**Why it happens:** Mechanical rename without full review
**How to avoid:** Rename ALL occurrences including variable names like `vtables_hpp` -> `interfaces_hpp`
**Warning signs:** Test passes but confusing code remains

### Pitfall 3: Missing Expected File in Array
**What goes wrong:** Updating one expected file array but missing another test location
**Why it happens:** Multiple test files with similar patterns
**How to avoid:** Search for ALL `"guest/vtables.*"` occurrences in tests
**Warning signs:** Different test fails after fixing one

## Code Examples

### Correct Test lib.rs Template (from integration_codegen_rust.rs)
```rust
// Source: crates/polyplugc/tests/integration_codegen_rust.rs:102-108
mod guest {
    pub mod types;
    pub mod contracts;
    pub mod interfaces;
}

// Source: crates/polyplugc/tests/integration_codegen_rust.rs:123-124
use guest::interfaces::TEST_ADDER_VTABLE;
use guest::interfaces::set_test_adder_impl;
```

### Correct Expected Files Array (for C++)
```rust
// Source: generators produce these files
let expected_files: [&str; 5] = [
    "guest/types.hpp",
    "guest/contracts.hpp",
    "guest/interfaces.hpp",  // NOT vtables.hpp
    "guest/init.hpp",
    "manifest.toml",
];
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `vtables.rs` / `vtables.hpp` | `interfaces.rs` / `interfaces.hpp` | Phase 01-06 | Generator naming aligned with GuestContractInterface |

**Deprecated/outdated:**
- All `vtables.*` files in generated directories — generators now produce `interfaces.*`
- The term "vtable" in test file expectations — use "interface" terminology

## Runtime State Inventory

> This phase involves file deletion and test updates, not runtime state changes.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None | No data migration needed |
| Live service config | None | No external services affected |
| OS-registered state | None | No OS-level registrations |
| Secrets/env vars | None | No secrets affected |
| Build artifacts | Stale vtables.* in examples | Delete files, no rebuild needed |

**Nothing found requiring runtime migration:** All changes are file-level operations.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Generator output unchanged by this phase | Standard Stack | LOW - generators already correct |
| A2 | g++ available on all test environments | Environment | MEDIUM - tests gracefully skip if unavailable |

**Most claims verified:** Generator behavior confirmed by reading source code.

## Open Questions

1. **Should stale host vtable_factories.* be deleted?**
   - What we know: These files exist in examples/hosts/*/generated/host/
   - What's unclear: Whether they're still used by example host code
   - Recommendation: Check examples/hosts/*/src/main.* for imports before deletion

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo | Rust tests | Yes | 1.94.0 | None required |
| g++ | C++ compile tests | Yes | 15.2.1 | Skip gracefully (test design) |
| polyplugc | Codegen tests | Built-in | workspace | cargo install |

**Missing dependencies with no fallback:** None

**Missing dependencies with fallback:** None

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + cargo test |
| Config file | None (tests use inline assertions) |
| Quick run command | `cargo test -p polyplugc --test smoke` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CLN-01 | No vtable naming remains | file search | `grep -r "vtables\." --include="*.rs" --include="*.hpp"` | Yes (grep) |
| CLN-04 | Tests use interfaces naming | unit | `cargo test -p polyplugc --test smoke` | Yes (smoke.rs) |
| SDK-05 | JS SDK uses correct types | integration | `cargo test -p polyplug --test integration_codegen_cpp` | Yes (integration_codegen_cpp.rs) |

### Sampling Rate
- **Per task commit:** `cargo test -p polyplugc --test smoke` (smoke test only)
- **Per wave merge:** `cargo test -p polyplugc` (all polyplugc tests)
- **Phase gate:** `cargo test --workspace` (full workspace)

### Wave 0 Gaps
None - existing test infrastructure covers all phase requirements:
- smoke.rs exists for Rust codegen E2E
- integration_codegen_cpp.rs exists for C++ codegen E2E
- generator_correctness.rs covers generator output validation

## Security Domain

> No security implications for test file naming updates. This phase is purely structural cleanup.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | None needed |
| V3 Session Management | No | None needed |
| V4 Access Control | No | None needed |
| V5 Input Validation | No | None needed |
| V6 Cryptography | No | None needed |

## Sources

### Primary (HIGH confidence)
- `crates/polyplugc/src/generators/rust.rs` - verified interfaces.rs output
- `crates/polyplugc/src/generators/cpp.rs` - verified interfaces.hpp output
- `crates/polyplugc/tests/smoke.rs` - identified outdated vtables.* references
- `crates/polyplug/tests/integration_codegen_cpp.rs` - identified outdated vtables.* references

### Secondary (MEDIUM confidence)
- `examples/guests/rust/validator/src/lib.rs` - verified correct pattern usage
- `examples/guests/rust/validator/src/generated/guest/mod.rs` - verified interfaces module

### Tertiary (LOW confidence)
- File glob searches for stale vtables.* files

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - generators verified by code inspection
- Architecture: HIGH - patterns documented with line numbers
- Pitfalls: HIGH - based on observed issues in current tests

**Research date:** 2026-04-06
**Valid until:** 30 days (stable test infrastructure)