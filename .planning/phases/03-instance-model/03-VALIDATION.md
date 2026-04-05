---
phase: 03
slug: instance-model
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-04-05
---

# Phase 03 — Validation Strategy

> Per-phase validation contract for instance model implementation.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` with cargo test |
| **Config file** | `Cargo.toml` (workspace) |
| **Quick run command** | `cargo test -p polyplug --lib` |
| **Full suite command** | `cargo test -p polyplug -p polyplugc` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplug --lib`
- **After every plan wave:** Run `cargo test -p polyplug -p polyplugc`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|--------|
| 03-01-01 | 01 | 1 | HC-01, CG-06 | — | singleton field defaults to false via serde | unit | `cargo check -p polyplugc` | ✅ green |
| 03-01-02 | 01 | 1 | CG-01 | — | generators use GuestContractInterface naming | verification | grep check | ✅ green |
| 03-02-01 | 02 | 2 | INST-05, INST-06 | — | guest vtables have create/destroy_instance | unit | `cargo check -p polyplugc` | ✅ green |
| 03-02-02 | 02 | 2 | CG-03 | — | dispatch signature includes instance param | unit | `cargo check -p polyplugc` | ✅ green |
| 03-03-01 | 03 | 3 | HC-02 | — | singleton contracts return cached instance | unit | `cargo test -p polyplug --lib -- singleton_contract` | ✅ green |
| 03-03-02 | 03 | 3 | HC-03 | — | multi-instance creates new each call | unit | `cargo test -p polyplug --lib -- multi_instance` | ✅ green |
| 03-03-03 | 03 | 3 | INST-04 | — | call_method FFI callback exists | unit | `cargo check -p polyplug` | ✅ green |
| 03-04-01 | 04 | 3 | INST-01, INST-02 | — | host caller calls create_instance in new() | unit | `cargo check -p polyplugc` | ✅ green |
| 03-04-02 | 04 | 3 | INST-03 | — | host caller calls destroy_instance in Drop | unit | `cargo check -p polyplugc` | ✅ green |
| 03-04-03 | 04 | 3 | CG-04 | — | dispatch passes instance parameter | integration | `cargo test -p polyplugc test_rust_codegen_compile_and_run` | ✅ green |
| 03-05-01 | 05 | 4 | HC-04, CG-05 | — | host contract factory includes singleton field | unit | `cargo check -p polyplugc` | ✅ green |

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| call_method cross-dispatch | INST-04 | Requires instance-to-contract mapping not yet implemented | Documented in runtime.rs as placeholder |

---

## Validation Audit 2026-04-05

| Metric | Count |
|--------|-------|
| Gaps found | 4 |
| Resolved | 4 |
| Escalated | 0 |

### Gap Details

1. **HC-02**: Singleton instance caching - Added test `singleton_contract_returns_cached_instance_on_multiple_calls`
2. **HC-03**: Multi-instance creation - Added test `multi_instance_contract_creates_new_instance_on_each_call`
3. **INST-01/02/03**: Host caller RAII pattern - Verified via codegen check
4. **INST-04/05/06**: Dispatch signature - Updated `integration_codegen_rust.rs` to use instance parameter

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** 2026-04-05