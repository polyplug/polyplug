---
phase: 17
slug: refactor-contractregistry-to-unified-runtimestore
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-10
---

# Phase 17 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `#[test]` + cargo test |
| **Config file** | None — Cargo.toml workspace test config |
| **Quick run command** | `cargo test -p polyplug --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplug --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 17-01-01 | 01 | 1 | REQ-03 | — | N/A | unit | `cargo test -p polyplug --lib` | ✅ | ⬜ pending |
| 17-01-02 | 01 | 1 | REQ-04 | — | N/A | lint | `cargo clippy --workspace -- -D warnings` | ✅ | ⬜ pending |
| 17-02-01 | 02 | 2 | REQ-01 | — | N/A | unit | `cargo test -p polyplug --lib get_bundle_plugin_slots` | ❌ W0 | ⬜ pending |
| 17-02-02 | 02 | 2 | REQ-02 | — | N/A | unit | `cargo test -p polyplug --lib get_bundle_descriptor` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/test_runtime_store.rs` — stubs for REQ-01 (O(1) bundle slot lookup) and REQ-02 (bundle metadata)
- [ ] Update imports in existing tests: `registry_edge_cases.rs`, `stress_error.rs`, `stress_concurrent_registry.rs`
- [ ] No framework install needed — Rust `#[test]` built-in

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| AGENTS.md rule compliance | REQ-04 | Visual inspection of code patterns | Check no type aliases, explicit types, no deprecated code |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

---

*Validation strategy for Phase 17: RuntimeStore refactor*