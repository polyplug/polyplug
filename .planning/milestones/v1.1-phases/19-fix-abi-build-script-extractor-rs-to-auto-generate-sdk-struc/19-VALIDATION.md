---
phase: 19
slug: fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-12
---

# Phase 19 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + per-language test frameworks (pytest, xUnit, Deno.test, assert) |
| **Config file** | `crates/polyplug_abi/Cargo.toml` (build-dependencies) |
| **Quick run command** | `cargo test -p polyplug_abi --lib` |
| **Full suite command** | `cargo test -p polyplug_abi && cargo build -p polyplug_abi` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p polyplug_abi --lib`
- **After every plan wave:** Run `cargo build -p polyplug_abi && grep -r 'sizeof\|static_assert\|ctypes.sizeof' sdks/*/abi/`
- **Before `/gsd-verify-work`:** Full build passes, all generated abi.* files contain valid code in each language, no PluginRegistrar references
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Decision | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|----------|------------|-----------------|-----------|-------------------|-------------|--------|
| 19-01-01 | 01 | 1 | D-01 | — | Module walk only follows `pub mod` declarations | unit | `cargo test -p polyplug_abi --lib test_walk_module_tree` | ⬜ W0 | ⬜ pending |
| 19-01-02 | 01 | 1 | D-02 | — | All #[repr(C)] types discovered | unit | `cargo test -p polyplug_abi --lib test_auto_discover_repr_c` | ⬜ W0 | ⬜ pending |
| 19-01-03 | 01 | 1 | D-03 | — | POLYPLUG_ constants discovered | unit | `cargo test -p polyplug_abi --lib test_auto_discover_constants` | ⬜ W0 | ⬜ pending |
| 19-02-01 | 02 | 2 | D-20 | — | Typed fn ptr signatures generated | unit | `grep 'CFUNCTYPE\|delegate\|fn.*->' sdks/*/abi/abi.*` | ⬜ W0 | ⬜ pending |
| 19-03-01 | 03 | 3 | D-22 | — | RuntimeConfig 16 bytes in all SDKs | layout | `pytest sdks/python/abi/test_layout.py` | ⬜ W0 | ⬜ pending |
| 19-03-02 | 03 | 3 | D-23 | — | GuestContractHandle 4 bytes, no generation field | layout | C++ static_assert in abi.hpp | ⬜ W0 | ⬜ pending |
| 19-03-03 | 03 | 3 | D-25 | — | HostContractInterface flat struct, 72 bytes | layout | `cargo test -p polyplug_abi --lib layout_host_contract_interface` | ✅ | ⬜ pending |
| 19-04-01 | 04 | 4 | D-26 | — | No hand-written structs in SDK host files | grep | `grep -c 'class.*Interface\|ctypes.Structure' sdks/*/host/` | ⬜ W0 | ⬜ pending |
| 19-05-01 | 05 | 5 | D-29/D-30 | — | No PluginRegistrar references remain | grep | `grep -r PluginRegistrar sdks/ docs/` | Manual | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Build script tests for module walking (unit tests in `build/extractor.rs` or separate test file)
- [ ] `sdks/python/abi/test_layout.py` — pytest layout assertions
- [ ] `sdks/csharp/abi/LayoutTests.cs` — xUnit layout tests
- [ ] `sdks/lua/abi/test_layout.lua` — assert-based layout checks
- [ ] `sdks/js/abi/test_layout.ts` — Deno.test layout checks
- [ ] `sdks/cpp/abi/test_layout.cpp` — static_assert layout checks

---

## Manual-Only Verifications

| Behavior | Decision | Why Manual | Test Instructions |
|----------|----------|------------|-------------------|
| No PluginRegistrar references in docs | D-30 | Requires reading prose context | `grep -r PluginRegistrar docs/ PRD.md README.md` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
