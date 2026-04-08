# Phase 13: C++ Codegen Modernization - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-08
**Phase:** 13-cpp-codegen-modernization
**Areas discussed:** Scope, Naming Convention, Testing, SDK Updates

---

## Scope Clarification

| Option | Description | Selected |
|--------|-------------|----------|
| C++ Only | Phase 13 is specifically for C++ codegen naming modernization | ✓ |
| All Codegens | Update all generators for consistency | |

**User's choice:** C++ Only
**Notes:** Phase 13 is specifically mapped to INST-01 through INST-06 and CG-02 through CG-05 in the roadmap. The instance model functionality was completed in Phase 12 for all SDKs. This phase is about C++ naming consistency with the modern ABI types.

---

## Naming Modernization Level

| Option | Description | Selected |
|--------|-------------|----------|
| Full modernization | Rename HostContractVTable → HostContractInterface, _VTABLE → _INTERFACE, vtable_ → interface_ | ✓ |
| Minimal: type names only | Only rename HostContractVTable → HostContractInterface | |
| Keep legacy naming | No naming changes | |

**User's choice:** Full modernization
**Notes:** All "vtable" terminology should be replaced with "interface" for consistency with other generators and the actual ABI types.

---

## Integration Tests

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, add integration test | Standard practice - ensures generated code is correct | ✓ |
| No new tests | Rely on existing polyplugc unit tests | |

**User's choice:** Yes, add integration test
**Notes:** Follow the pattern from integration_codegen_python.rs and integration_codegen_csharp.rs.

---

## SDK Updates

| Option | Description | Selected |
|--------|-------------|----------|
| No SDK changes needed | Generated code is self-contained, SDK files already use correct naming | |
| Update SDK for consistency | Ensure SDK aligns with generated patterns, run sdk_validator | ✓ |

**User's choice:** SDK must be real and working
**Notes:** User emphasized: "I don't want SDK hacks, I want SDK to be real and working correctly! Keep in mind I want you to understand our sdk_validator."

---

## C++ Standard

| Option | Description | Selected |
|--------|-------------|----------|
| C++17 | Existing standard, std::optional, std::string_view | ✓ |
| C++20 | Enable std::span, concepts, ranges | |
| C++14 | Maximum compatibility | |

**User's choice:** C++17
**Notes:** Existing code already uses C++17 features (std::optional, std::string_view).

---

## Claude's Discretion

- Exact test file structure matching other language integration tests
- Whether to add additional helper utilities to C++ SDK
- Specific line ranges to update in cpp.rs

## Deferred Ideas

None — discussion stayed within phase scope.