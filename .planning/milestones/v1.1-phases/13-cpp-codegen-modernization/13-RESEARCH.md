# Phase 13: C++ Codegen Modernization - Research

**Researched:** 2026-04-08
**Domain:** C++ code generation naming consistency, polyplugc generator
**Confidence:** HIGH

## Summary

This phase modernizes C++ codegen to align with the renamed ABI types (`HostContractInterface`, `GuestContractInterface`) instead of legacy `HostContractVTable`/`*_VTABLE` terminology. The instance model functionality (create_instance/destroy_instance) is already implemented in the C++ generator — this is purely a naming refactor.

**Primary recommendation:** Systematic string replacement in `crates/polyplugc/src/generators/cpp.rs` with test verification to ensure generated code compiles and SDK validator passes.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Implementation Decisions

### Naming Modernization
- **D-01:** Rename `HostContractVTable` → `HostContractInterface` in all generated code
- **D-02:** Rename `HostContractVTableHeader` → embed in `HostContractInterface` or use correct modern naming
- **D-03:** Rename `_VTABLE` suffix → `_INTERFACE` for static interface declarations
- **D-04:** Rename variable names `vtable_` → `interface_` in RAII wrappers
- **D-05:** Update all comments referencing "vtable" to use "interface" terminology

### C++ Standard
- **D-06:** Target C++17 for generated code (existing standard used)
- **D-07:** Continue using `std::optional`, `std::string_view` (C++17 features)

### Testing
- **D-08:** Create `integration_codegen_cpp.rs` test file
- **D-09:** Test should verify:
  - Generated files exist (host/types.hpp, guest/interfaces.hpp, etc.)
  - Generated code contains `HostContractInterface` not `HostContractVTable`
  - Generated code contains `_INTERFACE` not `_VTABLE`
  - Instance wrapper class exists with create/destroy lifecycle

### SDK Validation
- **D-10:** Run `sdk_validator` after changes to ensure C++ SDK consistency
- **D-11:** SDK files must be "real and working" — no hacks

### Claude's Discretion
- Exact test file structure matching other language integration tests
- Whether to add additional helper utilities to C++ SDK

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| INST-01 | Update codegen to generate `*Instance` RAII wrappers | C++ generator already has instance wrapper class (lines 1034-1165 in cpp.rs) — needs naming update only |
| INST-02 | Generated wrapper calls `create_instance` on construction | Already implemented — stubs exist at lines 338-358 |
| INST-03 | Generated wrapper calls `destroy_instance` on drop | Already implemented — stubs exist at lines 349-358 |
| INST-04 | Instance passed as first argument to all dispatch calls | Already implemented — dispatch passes instance parameter |
| INST-05 | Native dispatch: `functions[fn_id](instance, args, out)` | Already implemented — see NativeDispatch pattern |
| INST-06 | VM dispatch: `call(loader_data, instance, fn_id, args, out)` | Already implemented — see VmDispatch pattern |
| CG-02 | Update codegen to generate instance wrappers | Naming modernization only — functionality exists |
| CG-03 | Generated instance wrappers hold `interface` + `instance` pointer | Rename `vtable_` → `interface_` member variable |
| CG-04 | Generated wrappers call `create_instance`/`destroy_instance` | Already implemented — needs naming alignment |
| CG-05 | Update host contract vtable generation for `HostContractInterface` | Factory functions at lines 1895-2000 need renaming |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| polyplugc | workspace | Code generator | Generates type-safe bindings from api.toml/bundle.toml |
| polyplug_abi | workspace | ABI types | Defines `HostContractInterface`, `GuestContractInterface` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| sdk_validator | workspace | SDK consistency | Post-generation validation |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| String replacement | AST transformation | AST would be more robust but overkill for pure naming change |

## Architecture Patterns

### Current C++ Generator Structure
```
crates/polyplugc/src/generators/cpp.rs
├── Guest codegen (generate_guest)
│   ├── generate_cpp_guest_interfaces_file() — *_VTABLE static declarations
│   ├── generate_cpp_guest_contracts_file() — contract implementations
│   ├── generate_cpp_guest_host_contracts_file() — host contract callers
│   └── generate_cpp_guest_init_file() — polyplug_init entry point
├── Host codegen (generate_host)
│   ├── generate_cpp_host_types_file() — type definitions
│   ├── generate_cpp_host_callers_file() — caller wrappers
│   └── generate_cpp_host_interface_factories_file() — HostContractVTable factories
└── Unit tests (lines 2600-2940)
```

### Pattern: Static Interface Declaration (Guest-Side)
**What:** Static `GuestContractInterface` declaration with `*_VTABLE` naming
**Current (lines 361, 446):**
```cpp
static GuestContractInterface PLUGIN_VTABLE = {
    // fields...
};
```
**Target:**
```cpp
static GuestContractInterface PLUGIN_INTERFACE = {
    // fields...
};
```

### Pattern: Host Contract Caller RAII Wrapper
**What:** Guest-side class that wraps host contract interface pointer
**Current (lines 1548-1552):**
```cpp
explicit HostLoggerContract(const HostContractVTable* vtable) noexcept
    : vtable_(vtable) {}
const HostContractVTable* vtable_;
```
**Target:**
```cpp
explicit HostLoggerContract(const HostContractInterface* interface) noexcept
    : interface_(interface) {}
const HostContractInterface* interface_;
```

### Pattern: Host Contract Factory Functions
**What:** Create `HostContractInterface` for host implementations
**Current (lines 1907, 1934-1935, 1971, 1975-1976):**
```cpp
const HostContractVTable* create_host_logger_interface(...) noexcept {
    static HostContractVTable s_vtable = {
        HostContractVTableHeader{ ... },
        // dispatch
    };
}
```
**Target:**
```cpp
const HostContractInterface* create_host_logger_interface(...) noexcept {
    static HostContractInterface s_interface = {
        // contract_id, contract_version, singleton, dispatch_type, runtime
        // create_instance, destroy_instance
        // dispatch
    };
}
```

### Anti-Patterns to Avoid
- **Partial renaming:** Some occurrences use `HostContractInterface`, others use `HostContractVTable` — causes compilation errors
- **Comment drift:** Comments still reference "vtable" after renaming to "interface"
- **Test assertion mismatch:** Tests check for old naming patterns, will fail after rename

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Code generation | Manual string templates | polyplugc generator | Maintains consistency, handles edge cases |
| SDK validation | Manual inspection | sdk_validator | Automated consistency checks |

**Key insight:** This is a systematic rename — no new functionality needed.

## Runtime State Inventory

> This phase involves code generation changes only — no runtime state migration required.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None | — |
| Live service config | None | — |
| OS-registered state | None | — |
| Secrets/env vars | None | — |
| Build artifacts | None — generated code is output, not cached | — |

**Nothing found in any category:** This is purely a codegen naming change.

## Common Pitfalls

### Pitfall 1: Incomplete Rename
**What goes wrong:** Some instances of `HostContractVTable` or `_VTABLE` remain in generated code
**Why it happens:** String replacement misses edge cases (comments, factory function signatures)
**How to avoid:** Grep for all occurrences, verify line-by-line in cpp.rs
**Warning signs:** Generated code fails to compile against updated ABI types

### Pitfall 2: Test Assertion Drift
**What goes wrong:** Tests still check for `vtable_` after renaming to `interface_`
**Why it happens:** Tests at lines 2754, 3024 check for old naming patterns
**How to avoid:** Update all test assertions to use new naming
**Warning signs:** `cargo test` fails with "missing vtable member" errors

### Pitfall 3: Factory Function Type Mismatch
**What goes wrong:** Factory returns `HostContractVTable*` but ABI expects `HostContractInterface*`
**Why it happens:** Factory functions not updated consistently
**How to avoid:** Update return types in both NATIVE and VM factory functions
**Warning signs:** C++ compilation errors about type mismatch

### Pitfall 4: HostContractVTableHeader Removal
**What goes wrong:** Generated code references `HostContractVTableHeader` struct that doesn't exist in ABI
**Why it happens:** `HostContractInterface` in ABI doesn't have a separate header struct — fields are inline
**How to avoid:** Remove `HostContractVTableHeader` wrapper, emit fields directly into `HostContractInterface`
**Warning signs:** C++ compilation error "HostContractVTableHeader not defined"

## Code Examples

### Verified: HostContractInterface ABI Structure
```cpp
// Source: crates/polyplug_abi/src/host/host_contract_interface.rs
// C++ equivalent that generated code must match:
struct HostContractInterface {
    uint64_t contract_id;       // offset 0
    Version contract_version;   // offset 8 (12 bytes)
    bool singleton;             // offset 20
    // padding 3 bytes
    DispatchType dispatch_type; // offset 24
    // padding 4 bytes
    void* runtime;              // offset 32
    // create_instance, destroy_instance function pointers
    // dispatch union
};
// Total: 72 bytes, 8-byte aligned
```

### Verified: GuestContractInterface ABI Structure
```cpp
// Source: crates/polyplug_abi/src/guest/guest_contract_interface.rs
struct GuestContractInterface {
    uint64_t contract_id;
    Version contract_version;
    DispatchType dispatch_type;
    // create_instance, destroy_instance function pointers
    // dispatch union
};
// Total: 56 bytes, 8-byte aligned
```

### Current Factory Pattern (Needs Update)
```cpp
// Source: crates/polyplugc/src/generators/cpp.rs:1934-1952
static HostContractVTable s_vtable = {
    HostContractVTableHeader{
        1,  // vtable_version
        0x...ULL,  // contract_id
        major, minor, function_count,
        singleton,
        DispatchType::Native,
    },
    HostContractDispatch{
        NativeHostContractDispatch{ nullptr, FUNCTIONS },
    },
};
```

### Target Factory Pattern (After Rename)
```cpp
// Must match HostContractInterface fields directly:
static HostContractInterface s_interface = {
    0x...ULL,  // contract_id
    { major, minor, patch },  // contract_version
    singleton,  // singleton
    DispatchType::Native,  // dispatch_type
    nullptr,  // runtime (set during registration)
    create_instance_stub,  // create_instance
    destroy_instance_stub,  // destroy_instance
    { .native = { nullptr, FUNCTIONS } },  // dispatch
};
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `HostContractVTable` struct | `HostContractInterface` struct | Phase 1 (ABI Types) | C++ generator must align |
| `*_VTABLE` static naming | `*_INTERFACE` static naming | Phase 1 (ABI Types) | Generated declarations must rename |
| Separate `HostContractVTableHeader` | Inline fields in interface | Phase 1 (ABI Types) | Factory code must flatten struct |

**Deprecated/outdated:**
- `HostContractVTable` naming in generated C++ code
- `HostContractVTableHeader` struct wrapper
- `_VTABLE` suffix in static declarations
- `vtable_` member variable naming

## Assumptions Log

> All claims in this research are verified from source code — no assumptions.

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| — | None | — | — |

**If this table is empty:** All claims in this research were verified — no user confirmation needed.

## Open Questions

1. **HostContractVTableHeader removal details**
   - What we know: ABI has `HostContractInterface` with inline fields, no separate header struct
   - What's unclear: Exact field ordering in C++ generated code vs Rust ABI
   - Recommendation: Follow Rust ABI struct layout exactly (verified at lines 57-119 in host_contract_interface.rs)

## Environment Availability

> This phase has no external dependencies — pure code/config changes in Rust generator.

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust 1.85+ | polyplugc compilation | ✓ | — | — |
| cargo test | Test execution | ✓ | — | — |

**Missing dependencies with no fallback:**
- None

**Missing dependencies with fallback:**
- None

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + cargo test |
| Config file | Cargo.toml (workspace test profile) |
| Quick run command | `cargo test -p polyplugc --lib -- --test-threads=1` |
| Full suite command | `cargo test -p polyplugc` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CG-02 | Instance wrapper generation | unit | `cargo test -p polyplugc generate_cpp_guest_host_contract_caller` | ✅ existing (line 2729) |
| CG-03 | Interface pointer member | unit | `cargo test -p polyplugc generate_cpp_guest_host_contract_caller` | ✅ existing (line 2754) — needs update |
| CG-05 | Host contract factory | unit | `cargo test -p polyplugc generate_cpp_host_interface_factory` | ✅ existing (line 2772) |
| D-08 | Integration test for C++ | integration | `cargo test -p polyplugc integration_codegen_cpp` | ❌ Wave 0 — must create |

### Sampling Rate
- **Per task commit:** `cargo test -p polyplugc --lib`
- **Per wave merge:** `cargo test -p polyplugc`
- **Phase gate:** Full suite green + sdk_validator passes

### Wave 0 Gaps
- [ ] `integration_codegen_cpp.rs` — follows Python/C# pattern, verifies naming consistency
- [ ] Update existing test assertions at lines 2754, 3024 to check for `interface_` not `vtable_`

*(If no gaps: existing test infrastructure partially covers phase requirements — assertions need update)*

## Security Domain

> Security enforcement enabled — ASVS categories checked.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — (codegen only) |
| V3 Session Management | no | — (codegen only) |
| V4 Access Control | no | — (codegen only) |
| V5 Input Validation | no | — (codegen transforms, no external input) |
| V6 Cryptography | no | — (codegen only) |

### Known Threat Patterns for Rust Codegen

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| None applicable | — | — (pure code transformation, no runtime security concerns) |

**Security assessment:** This phase involves pure naming refactoring in code generation. No security-relevant changes — generated code behavior unchanged.

## Sources

### Primary (HIGH confidence)
- `crates/polyplugc/src/generators/cpp.rs` — C++ code generator (lines 361, 446, 1523, 1548, 1552, 1895-2000, 2754, 3024)
- `crates/polyplug_abi/src/host/host_contract_interface.rs` — HostContractInterface definition (verified layout)
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` — GuestContractInterface definition

### Secondary (MEDIUM confidence)
- `crates/polyplugc/tests/integration_codegen_python.rs` — Integration test pattern reference
- `crates/polyplugc/tests/integration_codegen_csharp.rs` — Integration test pattern reference
- `crates/polyplugc/tests/integration_host_contracts.rs` — Host contract test patterns

### Tertiary (LOW confidence)
- None — all findings verified from source

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — direct code inspection
- Architecture: HIGH — verified generator structure and ABI types
- Pitfalls: HIGH — identified specific line numbers needing change

**Research date:** 2026-04-08
**Valid until:** 30 days (stable codebase, naming refactor only)