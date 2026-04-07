---
phase: 08
slug: retroactive-verification
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-06
---

# Phase 08 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Bash/grep verification + cargo test |
| **Config file** | none — uses existing tests |
| **Quick run command** | `ls .planning/phases/*/VERIFICATION.md` |
| **Full suite command** | `cargo test --workspace 2>&1 | tail -5` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Check VERIFICATION.md file exists
- **After every plan wave:** Verify all target files created
- **Before `/gsd-verify-work`:** All 4 VERIFICATION.md files must exist
- **Max feedback latency:** 5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 08-01-01 | 01 | 1 | REG-01..06 | — | N/A | file_check | `test -f .planning/phases/02-registry/02-VERIFICATION.md` | ❌ W0 | ⬜ pending |
| 08-02-01 | 02 | 1 | INST-01..06, HC-02..04, CG-02..05 | — | N/A | file_check | `test -f .planning/phases/03-instance-model/03-VERIFICATION.md` | ❌ W0 | ⬜ pending |
| 08-03-01 | 03 | 1 | HR-01..06 | — | N/A | file_check | `test -f .planning/phases/04-hot-reload/04-VERIFICATION.md` | ❌ W0 | ⬜ pending |
| 08-04-01 | 04 | 2 | TH-01..08 | — | N/A | file_check | `test -f .planning/phases/07-typed-handles/07-VERIFICATION.md` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `.planning/phases/08-retroactive-verification/08-RESEARCH.md` — research complete ✅
- [ ] Read access to existing VERIFICATION.md files in phases 01, 05, 06 for format reference
- [ ] Read access to all SUMMARY.md files for evidence extraction

*Existing infrastructure covers all phase requirements.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| VERIFICATION.md content accuracy | All 35 | Requires human judgment | Verify each requirement maps to actual code evidence |

*All phase behaviors have automated file existence checks; content verification requires review.*

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 5s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending