# Phase 3: Verify Compatibility - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-03
**Phase:** 03-verify-compatibility
**Areas discussed:** Build scope, Test scope, FFI verification

---

## Build Scope

| Option | Description | Selected |
|--------|-------------|----------|
| Loader crates only | Verify loaders compile; acknowledge core has unrelated WIP issues | |
| Full workspace | Fix WIP issues first, then verify everything builds | |
| Loader crates + tests | Loader tests + integration tests, ignore SDK/example build issues | ✓ |

**User's choice:** Loader crates only (with note: "Core crate still has a lot of work to do, and mostly it will not be able to compile for a while.")
**Notes:** Core polyplug has ongoing WIP refactoring; don't block verification on fixing unrelated issues.

---

## Test Scope

| Option | Description | Selected |
|--------|-------------|----------|
| cargo test --workspace | Comprehensive verification across all functionality | |
| Loader tests only | Focus on loader-specific functionality only | |
| Loader + integration tests | Loader unit tests + cross-language integration tests | ✓ |

**User's choice:** Loader + integration tests
**Notes:** Focused verification on error handling changes.

---

## FFI Verification

| Option | Description | Selected |
|--------|-------------|----------|
| Trust tests | Error messages are strings at FFI boundary by design; tests passing is sufficient evidence | |
| Explicit check | Manually verify error string format at FFI boundary before/after | ✓ |

**User's choice:** Explicit check
**Notes:** User wants explicit verification of string format at boundary, not just trusting tests.

---

## Claude's Discretion

- Exact test commands and flags
- How to present verification results
- Whether to skip specific failing tests if they're unrelated to error handling

## Deferred Ideas

- Full workspace build verification (blocked by core WIP refactoring)
- SDK compilation verification
- Example host compilation verification