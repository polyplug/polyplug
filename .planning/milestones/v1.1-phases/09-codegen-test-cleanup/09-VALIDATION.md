---
phase: 09
slug: codegen-test-cleanup
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-04-06
updated: 2026-04-06
---

# Phase 09 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + cargo test |
| **Config file** | none — existing infrastructure |
| **Quick run command** | `cargo test -p polyplugc --test smoke` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplugc --test smoke`
- **After every plan wave:** Run `cargo test -p polyplugc`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 09-01-01 | 01 | 1 | CLN-01 | N/A | N/A | file search | `grep -r "vtables\." --include="*.rs" --include="*.hpp" crates/polyplugc/tests/` | ✅ W0 | ✅ green |
| 09-01-02 | 01 | 1 | CLN-04 | N/A | N/A | unit | `cargo test -p polyplugc --test smoke` | ✅ W0 | ✅ green |
| 09-02-01 | 02 | 1 | CLN-01 | N/A | N/A | integration | `cargo test -p polyplug --test integration_codegen_cpp` | ✅ W0 | ✅ green |
| 09-02-02 | 02 | 1 | SDK-05 | N/A | N/A | file delete | `test ! -f examples/guests/js/*/generated/guest/vtable.ts` | ✅ W0 | ✅ green |
| 09-03-01 | 03 | 2 | CLN-01 | N/A | N/A | e2e | `cargo test -p polyplugc --test smoke -- --test cpp_codegen_e2e` | ✅ W0 | ✅ green |
| 09-03-02 | 03 | 2 | CLN-04 | N/A | N/A | e2e | `cargo test -p polyplugc --test smoke -- --test rust_codegen_e2e` | ✅ W0 | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements:
- `crates/polyplugc/tests/smoke.rs` — covers Rust codegen E2E
- `crates/polyplug/tests/integration_codegen_cpp.rs` — covers C++ codegen E2E
- `crates/polyplugc/tests/generator_correctness.rs` — covers generator output validation
- `crates/polyplugc/tests/integration_codegen_rust.rs` — already has correct patterns (reference)

*No Wave 0 installation needed — all tests exist.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Stale file deletion confirmation | CLN-01 | File system operation | `ls examples/guests/*/generated/guest/vtables.*` should return nothing |

*All other phase behaviors have automated verification.*

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** ✅ COMPLETE — All tasks verified green