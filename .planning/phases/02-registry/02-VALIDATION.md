---
phase: 02
slug: registry
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-04-05
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in #[test] + criterion benchmarks |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p polyplug --test registry_edge_cases --test hot_reload_safety` |
| **Full suite command** | `cargo test -p polyplug --lib --tests` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplug --test registry_edge_cases`
- **After every plan wave:** Run `cargo test -p polyplug --lib --tests`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-01 | 01 | 1 | REG-01 | — | Direct Arc<GuestContractInterface> storage | unit | `cargo test -p polyplug --test registry_edge_cases` | ✅ | ✅ green |
| 02-01-02 | 01 | 1 | REG-02 | — | No PluginGuard, resolve returns pointer | unit | `cargo test -p polyplug --test registry_edge_cases` | ✅ | ✅ green |
| 02-02-01 | 02 | 2 | REG-04 | — | Direct RwLock swap without ArcSwap | unit | `cargo test -p polyplug --test hot_reload_safety` | ✅ | ✅ green |
| 02-02-02 | 02 | 2 | REG-04 | — | Tests updated for new patterns | unit | `cargo test -p polyplug --test stress_concurrent_registry` | ✅ | ✅ green |
| 02-03-01 | 03 | 3 | REG-03 | — | PluginHandle has only index field | unit | `cargo test -p polyplug --test registry_edge_cases` | ✅ | ✅ green |
| 02-03-02 | 03 | 3 | REG-06 | — | find_by_contract returns handle without generation | unit | `cargo test -p polyplug --test registry_edge_cases` | ✅ | ✅ green |
| 02-03-03 | 03 | 3 | REG-05 | — | RegistrySlot simplified (no generation) | unit | `cargo test -p polyplug --test registry_edge_cases` | ✅ | ✅ green |

---

## Wave 0 Requirements

All tests existed from prior phases. Updates applied:
- ✅ `tests/registry_edge_cases.rs` — updated for BundleId, NativeDispatch.function_count
- ✅ `tests/hot_reload_safety.rs` — updated for new interface storage
- ✅ `tests/stress_concurrent_registry.rs` — updated for direct swap pattern

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| stress_hot_reload full integration | REG-04 | Requires build.rs infrastructure and polyplug_native dev-dependency | Run `cargo test -p polyplug --test stress_hot_reload` after adding build infrastructure |

---

## Validation Audit 2026-04-05

| Metric | Count |
|--------|-------|
| Gaps found | 4 |
| Resolved | 3 |
| Escalated | 1 (stress_hot_reload - infrastructure dependency) |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 5s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-04-05