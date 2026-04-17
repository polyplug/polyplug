---
phase: 15
slug: final-cleanup
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-08
---

# Phase 15 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Cargo test (Rust built-in) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test --workspace -q 2>&1 \| head -50` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** `cargo test -p polyplug -q` (affected crate only)
- **After every plan wave:** `cargo test --workspace -q`
- **Before `/gsd-verify-work`:** Full suite green + grep audit showing 0 occurrences (excluding planning artifacts)
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 15-01-01 | 01 | 1 | CLN-01 | — | N/A (naming cleanup) | grep | `grep -ri "vtable" crates/polyplugc/src/generators/*.rs \| wc -l` | ✅ W0 | ⬜ pending |
| 15-02-01 | 02 | 2 | CLN-01 | — | N/A (regeneration) | build | `cargo build --workspace` | ✅ W0 | ⬜ pending |
| 15-03-01 | 03 | 3 | CLN-01 | — | N/A (cleanup) | grep | `grep -ri "vtable" crates/polyplug/src/*.rs \| wc -l` | ✅ W0 | ⬜ pending |
| 15-04-01 | 04 | 4 | CLN-01 | — | N/A (cleanup) | grep | `grep -ri "vtable" sdks/ \| wc -l` | ✅ W0 | ⬜ pending |
| 15-05-01 | 05 | 5 | CLN-01 | — | N/A (cleanup) | grep | `grep -ri "vtable" tests/fixtures/ \| wc -l` | ✅ W0 | ⬜ pending |
| 15-06-01 | 06 | 6 | CLN-01 | — | N/A (docs) | grep | `grep -ri "vtable" docs/ \| wc -l` | ✅ W0 | ⬜ pending |
| 15-07-01 | 07 | 7 | CLN-01, CLN-04 | — | N/A (verification) | integration | `cargo test --workspace` | ✅ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Generator tests for vtable→interface naming in smoke.rs (uses TEST_ADDER_VTABLE)
- [ ] Integration tests for generated code naming patterns

*Existing infrastructure covers compilation and test runs. Wave 0 focuses on test expectations.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| grep audit excludes planning artifacts | CLN-01 | Requires exclusion filter | Run: `grep -ri "vtable" crates/ sdks/ docs/ tests/fixtures/ --include="*.rs" --include="*.py" --include="*.lua" --include="*.js" --include="*.ts" --include="*.cs" --include="*.hpp" --include="*.md" \| grep -v ".planning/" \| wc -l` |
| vtable_version field preserved | CLN-01 | ABI field name check | Verify `vtable_version` appears in polyplug_abi structs only |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending