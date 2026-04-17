---
phase: 11
slug: guest-calling-convention-missing-introspection
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-07
---

# Phase 11 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + `cargo test` |
| **Config file** | none — tests inline in source files |
| **Quick run command** | `cargo test -p polyplug_abi -p polyplug --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplug_abi -p polyplug --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 11-01-01 | 01 | 1 | D-01 | T-11-01 | HostInterface renamed from RuntimeAbi, null checks on interface ptr | compile | `cargo build -p polyplug_abi` | ❌ W0 | ⬜ pending |
| 11-01-02 | 01 | 1 | D-02 | T-11-01 | RuntimeInterface exists with self-passing pattern, null checks | unit | `cargo test -p polyplug_abi layout_runtime_interface` | ❌ W0 | ⬜ pending |
| 11-02-01 | 02 | 1 | D-03 | — | RuntimeContext deleted, no wrapper types | compile | `cargo build -p polyplug` | ❌ W0 | ⬜ pending |
| 11-03-01 | 03 | 2 | D-05 | T-11-02 | Array<T> has align field for proper freeing, length tracked | unit | `cargo test -p polyplug_abi layout_array` | ❌ W0 | ⬜ pending |
| 11-03-02 | 03 | 2 | D-06 | T-11-01 | GuestContractInstance has contract_id field, zero-overhead dispatch | unit | `cargo test -p polyplug_abi layout_guest_contract_instance` | ✅ update | ⬜ pending |
| 11-03-03 | 03 | 2 | D-10 | — | DependencyInfo struct exists, mirrors manifest structure | unit | `cargo test -p polyplug_abi layout_dependency_info` | ❌ W0 | ⬜ pending |
| 11-04-01 | 04 | 3 | D-07 | — | list_bundles returns Array<BundleId> | unit | `cargo test -p polyplug introspection_list_bundles` | ❌ W0 | ⬜ pending |
| 11-04-02 | 04 | 3 | D-08 | — | get_dependencies returns Array<DependencyInfo> | unit | `cargo test -p polyplug introspection_get_dependencies` | ❌ W0 | ⬜ pending |
| 11-05-01 | 05 | 3 | D-12 | — | GuestContractInterface create/destroy take HostInterface | compile | `cargo build -p polyplug_abi` | ❌ W0 | ⬜ pending |
| 11-05-02 | 05 | 3 | D-13 | — | HostContractInterface has runtime field, self-passing | compile | `cargo build -p polyplug_abi` | ❌ W0 | ⬜ pending |
| 11-06-01 | 06 | 4 | D-11 | — | find_all_by_contract returns Array<ContractHandle> | unit | `cargo test -p polyplug find_all_by_contract` | ❌ W0 | ⬜ pending |
| 11-07-01 | 07 | 4 | D-14 | — | First-class documentation on all interface types | manual | `cargo doc -p polyplug_abi --open` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/polyplug_abi/src/host/runtime_interface.rs` — tests for RuntimeInterface layout (16-byte check)
- [ ] `crates/polyplug_abi/src/types/array.rs` — tests for enhanced Array layout (24-byte check with align)
- [ ] `crates/polyplug_abi/src/types/dependency_info.rs` — tests for DependencyInfo layout (16-byte check)
- [ ] Update existing tests in `guest_contract_instance.rs` for new 16-byte size
- [ ] `crates/polyplug/src/registry/plugin_registry.rs` — tests for introspection APIs

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Documentation completeness | D-14 | Rustdoc quality requires human review | Run `cargo doc -p polyplug_abi --open`, verify each struct has purpose/provider/caller/ownership/lifetime sections |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending