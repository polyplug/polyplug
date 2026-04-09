---
phase: 13
slug: cpp-codegen-modernization
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-08
---

# Phase 13 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + cargo test |
| **Config file** | Cargo.toml (workspace test profile) |
| **Quick run command** | `cargo test -p polyplugc --lib -- --test-threads=1` |
| **Full suite command** | `cargo test -p polyplugc` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplugc --lib`
- **After every plan wave:** Run `cargo test -p polyplugc`
- **Before `/gsd-verify-work`:** Full suite must be green + sdk_validator passes
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 13-01-01 | 01 | 1 | CG-02 | — | N/A (codegen) | unit | `cargo test -p polyplugc generate_cpp_guest_host_contract_caller` | ✅ W0 | ⬜ pending |
| 13-01-02 | 01 | 1 | CG-03 | — | N/A (codegen) | unit | `cargo test -p polyplugc generate_cpp_guest_host_contract_caller` | ⬜ update W0 | ⬜ pending |
| 13-02-01 | 02 | 1 | CG-05 | — | N/A (codegen) | unit | `cargo test -p polyplugc generate_cpp_host_interface_factory` | ✅ W0 | ⬜ pending |
| 13-03-01 | 03 | 1 | D-08 | — | N/A (codegen) | integration | `cargo test -p polyplugc integration_codegen_cpp` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/polyplugc/tests/integration_codegen_cpp.rs` — stubs for CG-02, CG-03, CG-05, D-08
- [ ] Update existing test assertions at lines 2754, 3024 in cpp.rs to check for `interface_` not `vtable_`

*Existing infrastructure partially covers phase requirements — assertions need update.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| sdk_validator C++ rules pass | CG-04 | Requires external tool run | Run `just validate-sdks` and verify C++ rules pass |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending