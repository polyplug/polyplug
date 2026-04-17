---
phase: 04
slug: hot-reload
status: validated
nyquist_compliant: true
wave_0_complete: true
created: 2026-04-06
---

# Phase 04 — Validation Strategy

> Per-phase validation contract for hot-reload callback-based model.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust cargo test (built-in) |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p polyplug reload -- --test-threads=1` |
| **Full suite command** | `cargo test --workspace -- --test-threads=1` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplug reload -- --test-threads=1`
- **After every plan wave:** Run `cargo test --workspace -- --test-threads=1`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 04-01-01 | 01 | 1 | HR-01 | T-04-01 | `wait_for_quiescence` removed from reload.rs | unit | `grep -c "wait_for_quiescence" crates/polyplug/src/reload.rs` | ✅ | ✅ green |
| 04-01-02 | 01 | 1 | HR-01 | T-04-01 | `QUIESCENCE_TIMEOUT` constant removed | unit | `grep -c "QUIESCENCE_TIMEOUT" crates/polyplug/src/reload.rs` | ✅ | ✅ green |
| 04-01-03 | 01 | 1 | HR-02 | T-04-02 | `QuiescenceTimeout` error removed | unit | `grep -c "QuiescenceTimeout" crates/polyplug/src/error.rs` | ✅ | ✅ green |
| 04-02-01 | 02 | 2 | HR-06 | T-04-04 | Warning emitted when Arc refs remain | integration | `cargo test -p integration --test integration_hot_reload_warning` | ✅ | ✅ green |
| 04-02-02 | 02 | 2 | HR-05 | T-04-07 | Interface swap after loader.reload() succeeds | integration | `cargo test -p polyplug --test hot_reload_safety` | ✅ | ✅ green |
| 04-02-03 | 02 | 2 | HR-03 | — | Preparing fires before interface swap | integration | `cargo test -p polyplug --test stress_hot_reload` | ✅ | ✅ green |
| 04-03-01 | 03 | 3 | HR-04 | — | Test docs updated for callback model | unit | `grep -c "callback-based" crates/polyplug/tests/hot_reload_safety.rs` | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

All requirements covered by existing infrastructure. No Wave 0 setup needed.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| None | — | — | — |

All phase behaviors have automated verification.

---

## Validation Audit 2026-04-06

| Metric | Count |
|--------|-------|
| Gaps found | 1 |
| Resolved | 1 |
| Escalated | 0 |

**Gap HR-06:** No test verifying warning emission when `Arc::strong_count > 1` after Preparing callback.

**Resolution:** Created `tests/integration/tests/integration_hot_reload_warning.rs` with 4 tests:
- `test_warning_callback_invoked_during_reload` — Verifies warning callback mechanism
- `test_warning_timing_after_preparing_before_reloaded` — Verifies event ordering
- `test_warning_message_content_structure` — Verifies warning message format
- `test_reload_works_without_warning_callback` — Verifies optional callback behavior

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 60s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** 2026-04-06