# Phase 8: Retroactive Verification - Research

**Researched:** 2026-04-06
**Domain:** Documentation/Verification artifact creation
**Confidence:** HIGH

## Summary

Phase 8 is a documentation gap closure phase. The goal is to create VERIFICATION.md files for 4 already-completed phases (02, 03, 04, 07) that currently only have VALIDATION.md files. These phases were executed successfully (all SUMMARY.md files exist and all tests pass) but lack the formal VERIFICATION.md artifact that documents requirement satisfaction with evidence.

**Primary recommendation:** Create 4 VERIFICATION.md files by synthesizing evidence from SUMMARY.md files, VALIDATION.md test mappings, and code artifact verification.

## VERIFICATION.md Structure

### Required Frontmatter

```yaml
---
phase: XX-name
verified: YYYY-MM-DDTHH:MM:SSZ
status: passed | gaps_found | failed
score: N/M must-haves verified
gaps: []  # Only if status != passed
---
```

### Required Content Sections

| Section | Purpose | Source |
|---------|---------|--------|
| `## Goal Achievement` | Observable truths table with status/evidence | ROADMAP success criteria |
| `## Observable Truths` | Boolean truths that must be TRUE | ROADMAP + implementation |
| `## Required Artifacts` | Files that must exist with expected content | PLAN files |
| `## Key Link Verification` | Import/export wiring checks | Code grep |
| `## Data-Flow Trace` | Variable flow verification (Level 4) | Code analysis |
| `## Behavioral Spot-Checks` | Test execution commands | VALIDATION.md |
| `## Requirements Coverage` | REQ-ID -> status -> evidence mapping | REQUIREMENTS.md + SUMMARY.md |
| `## Anti-Patterns Found` | Naming, structure issues | Code grep |

### Example Format (from Phase 01)

```markdown
| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 | GuestContractInterface struct exists with create_instance/destroy_instance fields | VERIFIED | File exists at `crates/polyplug_abi/src/guest/guest_contract_interface.rs`, both fields present at lines 40-54 |
| 2 | RuntimeAbi struct renamed from HostVTable with call_method field | VERIFIED | File exists at `crates/polyplug_abi/src/host/runtime_abi.rs`, call_method at lines 76-81 |
```

### Requirements Coverage Table Format

```markdown
| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ---------- | ----------- | ------ | -------- |
| REG-01 | 02-01 | Remove VTableSlot wrapper - store GuestContractInterface directly | SATISFIED | plugin_registry.rs stores Arc<GuestContractInterface> directly, VTableSlot struct removed |
```

## VALIDATION vs VERIFICATION Relationship

### VALIDATION.md Purpose

VALIDATION.md is the **Nyquist validation contract** - it defines:
- Test infrastructure (framework, commands)
- Per-task verification map (Task ID -> Test Command)
- Sampling rate (commit-level, wave-level, phase-level)
- Wave 0 requirements (tests that must exist before execution)

VALIDATION.md answers: **"What tests exist and how do I run them?"**

### VERIFICATION.md Purpose

VERIFICATION.md is the **post-execution evidence document** - it proves:
- Requirements were satisfied (with specific file/line evidence)
- Observable truths are TRUE (with verification status)
- Key integration points are wired correctly
- Behavioral tests pass (with actual output)

VERIFICATION.md answers: **"Did the phase succeed and what proves it?"**

### Key Distinction

| Aspect | VALIDATION.md | VERIFICATION.md |
|--------|---------------|-----------------|
| Created | Before/during execution | After execution completes |
| Content | Test commands, sampling | Evidence, truths, status |
| Audience | Planner/Executor | Auditor/Verifier |
| Nyquist | Validation contract | Proof of compliance |

**Phase 8 transforms VALIDATION test mappings into VERIFICATION evidence statements.**

## Evidence Sources

### Phase 02: Registry (REG-01 through REG-06)

| Requirement | SUMMARY Source | Code Evidence | VALIDATION Test |
|-------------|----------------|---------------|-----------------|
| REG-01 | 02-01-SUMMARY.md (lines 37: `requirements-completed: [REG-01, REG-02, REG-05]`) | plugin_registry.rs stores Arc directly | `registry_edge_cases.rs` |
| REG-02 | 02-01-SUMMARY.md | PluginGuard deleted, resolve() returns pointer | `registry_edge_cases.rs` |
| REG-03 | 02-03-PLAN.md (no SUMMARY exists - gap) | PluginHandle has only index field | `registry_edge_cases.rs` |
| REG-04 | 02-02-SUMMARY.md | arc-swap removed, direct RwLock swap | `hot_reload_safety.rs` |
| REG-05 | 02-01-SUMMARY.md | RegistrySlot simplified | `registry_edge_cases.rs` |
| REG-06 | 02-03-PLAN.md (no SUMMARY exists - gap) | find_contract returns handle without generation | `registry_edge_cases.rs` |

**Gap: 02-03-SUMMARY.md does not exist.** The plan 02-03-PLAN.md exists but execution summary was not written. VERIFICATION.md must synthesize evidence from PLAN + VALIDATION tests + code grep.

### Phase 03: Instance Model (INST-01 through INST-06, HC-02 through HC-04, CG-02 through CG-05)

| Requirement | SUMMARY Source | Code Evidence | VALIDATION Test |
|-------------|----------------|---------------|-----------------|
| INST-01 | 03-04-SUMMARY.md (lines 40-41: `tags: [codegen, rust-generator, instance-wrapper, raii]`) | Generated wrappers call create_instance in new() | cargo check -p polyplugc |
| INST-02 | 03-04-SUMMARY.md | Generated wrappers call create_instance on construction | cargo check -p polyplugc |
| INST-03 | 03-04-SUMMARY.md | Generated wrappers call destroy_instance in Drop impl | cargo check -p polyplugc |
| INST-04 | 03-02-SUMMARY.md (lines 42: `dispatch signature includes instance param`) | Instance passed as first argument to dispatch | cargo check -p polyplugc |
| INST-05 | 03-02-SUMMARY.md | Native dispatch signature: fn(instance, args, out) | rust.rs generator |
| INST-06 | 03-02-SUMMARY.md | VM dispatch signature: fn(loader_data, instance, fn_id, args, out) | rust.rs generator |
| HC-02 | 03-03-SUMMARY.md (lines 46-50: `singleton-cache, double-check-locking`) | get_host_contract returns cached instance for singleton | singleton_contract_returns_cached_instance_on_multiple_calls test |
| HC-03 | 03-03-SUMMARY.md | get_host_contract creates new instance for multi-instance | multi_instance_contract_creates_new_instance_on_each_call test |
| HC-04 | 03-05-SUMMARY.md (lines 43: `Added singleton field extraction`) | Codegen generates host contract implementations with singleton field | cargo check -p polyplugc |
| CG-02 | 03-04-SUMMARY.md | Codegen generates instance wrappers holding interface + instance pointer | rust.rs: `interface: *const PluginInterface, instance: GuestContractInstance` |
| CG-03 | 03-04-SUMMARY.md | Generated wrappers hold interface + instance pointers | rust.rs struct fields |
| CG-04 | 03-04-SUMMARY.md | Generated wrappers call create/destroy_instance | Drop impl generation |
| CG-05 | 03-05-SUMMARY.md | Host contract vtable generation for HostContractInterface | All 6 generators updated |

### Phase 04: Hot-Reload (HR-01 through HR-06)

| Requirement | SUMMARY Source | Code Evidence | VALIDATION Test |
|-------------|----------------|---------------|-----------------|
| HR-01 | 04-01-SUMMARY.md (lines 7: `requirements: [HR-01, HR-02]`) | wait_for_quiescence removed from reload.rs | grep check |
| HR-02 | 04-01-SUMMARY.md | Callback-based model replaces Arc quiescence | cargo build -p polyplug |
| HR-03 | 04-02-SUMMARY.md (lines 7: `requirements: [HR-03, HR-05, HR-06]`) | Preparing fires before interface swap | reload.rs: emit callback before swap |
| HR-04 | 04-03-SUMMARY.md (lines 7: `requirements: [HR-04]`) | Host destroys instances in callback | hot_reload_safety.rs docs |
| HR-05 | 04-02-SUMMARY.md | Runtime swaps interfaces after callback returns | reload.rs: swap_interface after loader.reload() |
| HR-06 | 04-02-SUMMARY.md | Warning callback if instances remain | reload.rs: Arc::strong_count check, emit_warning |

### Phase 07: Typed Handles (TH-01 through TH-08)

| Requirement | SUMMARY Source | Code Evidence | VALIDATION Test |
|-------------|----------------|---------------|-----------------|
| TH-01 | 07-02-SUMMARY.md + 07-01-SUMMARY.md | RuntimeContext replaces *mut c_void for rt_ctx | `runtime_abi_uses_runtime_context` test |
| TH-02 | 07-01-SUMMARY.md | VmLoaderData replaces *mut c_void for loader_data | `vm_dispatch_uses_vm_loader_data` test |
| TH-03 | 07-03-SUMMARY.md | GuestContractInstance for instance in native dispatch | `vm_dispatch_instance_is_guest_contract_instance` test |
| TH-04 | 07-01-SUMMARY.md | RuntimeContext struct created (opaque handle) | `layout_runtime_context` test (size=8, align=8) |
| TH-05 | 07-01-SUMMARY.md | VmLoaderData struct created (opaque handle) | `layout_vm_loader_data` test (size=8, align=8) |
| TH-06 | 07-02-SUMMARY.md | All RuntimeAbi functions use RuntimeContext | `host_callbacks_use_runtime_context` test |
| TH-07 | 07-03-SUMMARY.md | PluginContext uses typed handles, no bare c_void | `plugin_context_no_bare_c_void` test |
| TH-08 | 07-01-SUMMARY.md + 07-04-SUMMARY.md | All opaque handles #[repr(C)] with single data field | 4 repr_c tests for all handles |

## Requirements Mapping

### Phase 02 Requirements (6 total)

| ID | Description | Plan | Evidence Status |
|----|-------------|------|-----------------|
| REG-01 | Remove VTableSlot wrapper | 02-01 | SUMMARY exists, tests green |
| REG-02 | Remove PluginGuard | 02-01 | SUMMARY exists, tests green |
| REG-03 | Remove generation counter | 02-03 | **SUMMARY MISSING** - use PLAN + tests |
| REG-04 | Remove ArcSwap pattern | 02-02 | SUMMARY exists, tests green |
| REG-05 | Simplify RegistrySlot | 02-01 | SUMMARY exists, tests green |
| REG-06 | find_contract returns handle without generation | 02-03 | **SUMMARY MISSING** - use PLAN + tests |

### Phase 03 Requirements (16 total)

| ID | Description | Plan | Evidence Status |
|----|-------------|------|-----------------|
| INST-01 | Codegen generates *Instance RAII wrappers | 03-04 | SUMMARY exists |
| INST-02 | Wrapper calls create_instance on construction | 03-04 | SUMMARY exists |
| INST-03 | Wrapper calls destroy_instance on drop | 03-04 | SUMMARY exists |
| INST-04 | Instance passed as first argument | 03-02 | SUMMARY exists |
| INST-05 | Native dispatch signature | 03-02 | SUMMARY exists |
| INST-06 | VM dispatch signature | 03-02 | SUMMARY exists |
| HC-02 | get_host_contract returns same instance for singleton | 03-03 | SUMMARY exists, VALIDATION test added |
| HC-03 | get_host_contract creates new instance for multi-instance | 03-03 | SUMMARY exists, VALIDATION test added |
| HC-04 | Update codegen for host contract implementations | 03-05 | SUMMARY exists |
| CG-02 | Codegen generates instance wrappers | 03-04 | SUMMARY exists |
| CG-03 | Instance wrappers hold interface + instance pointer | 03-04 | SUMMARY exists |
| CG-04 | Wrappers call create/destroy_instance | 03-04 | SUMMARY exists |
| CG-05 | Host contract vtable generation | 03-05 | SUMMARY exists |

### Phase 04 Requirements (6 total)

| ID | Description | Plan | Evidence Status |
|----|-------------|------|-----------------|
| HR-01 | Remove wait_for_quiescence | 04-01 | SUMMARY exists, grep verified |
| HR-02 | Update to callback-only model | 04-01 | SUMMARY exists |
| HR-03 | Preparing fires before interface swap | 04-02 | SUMMARY exists |
| HR-04 | Host destroys all instances in callback | 04-03 | SUMMARY exists (docs update) |
| HR-05 | Runtime swaps interfaces after callback | 04-02 | SUMMARY exists |
| HR-06 | Warning callback if instances remain | 04-02 | SUMMARY exists, VALIDATION test added |

### Phase 07 Requirements (8 total)

| ID | Description | Plan | Evidence Status |
|----|-------------|------|-----------------|
| TH-01 | RuntimeContext replaces *mut c_void for rt_ctx | 07-01, 07-02 | SUMMARY exists, VALIDATION test exists |
| TH-02 | VmLoaderData replaces *mut c_void for loader_data | 07-01 | SUMMARY exists, VALIDATION test exists |
| TH-03 | GuestContractInstance for instance parameter | 07-03 | SUMMARY exists, VALIDATION test exists |
| TH-04 | RuntimeContext struct created | 07-01 | SUMMARY exists, VALIDATION test exists |
| TH-05 | VmLoaderData struct created | 07-01 | SUMMARY exists, VALIDATION test exists |
| TH-06 | All RuntimeAbi functions use RuntimeContext | 07-02 | SUMMARY exists, VALIDATION test exists |
| TH-07 | PluginContext uses typed handles | 07-03 | SUMMARY exists, VALIDATION test exists |
| TH-08 | All opaque handles #[repr(C)] | 07-01, 07-04 | SUMMARY exists, VALIDATION tests exist |

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in #[test] + cargo test |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p polyplug --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REG-01 | Direct Arc storage | unit | `cargo test -p polyplug --test registry_edge_cases` | YES |
| REG-02 | No PluginGuard | unit | `cargo test -p polyplug --test registry_edge_cases` | YES |
| REG-03 | No generation counter | unit | `cargo test -p polyplug --test registry_edge_cases` | YES |
| REG-04 | No ArcSwap | unit | `cargo test -p polyplug --test hot_reload_safety` | YES |
| REG-05 | Simplified slot | unit | `cargo test -p polyplug --test registry_edge_cases` | YES |
| REG-06 | Handle without generation | unit | `cargo test -p polyplug --test registry_edge_cases` | YES |
| INST-01-03 | RAII wrapper lifecycle | compile | `cargo check -p polyplugc` | YES |
| INST-04-06 | Instance dispatch params | compile | `cargo check -p polyplugc` | YES |
| HC-02 | Singleton caching | unit | `cargo test -p polyplug -- singleton_contract` | YES |
| HC-03 | Multi-instance creation | unit | `cargo test -p polyplug -- multi_instance` | YES |
| HR-01 | No quiescence wait | grep | `grep -c "wait_for_quiescence" reload.rs` | YES |
| HR-06 | Warning emission | integration | `cargo test -p integration --test integration_hot_reload_warning` | YES |
| TH-01 | RuntimeAbi uses RuntimeContext | unit | `cargo test -p polyplug_abi runtime_abi_uses_runtime_context` | YES |
| TH-02 | VmDispatch uses VmLoaderData | unit | `cargo test -p polyplug_abi vm_dispatch_uses_vm_loader_data` | YES |
| TH-03 | VmDispatch instance param | unit | `cargo test -p polyplug_abi vm_dispatch_instance_is_guest_contract_instance` | YES |
| TH-04 | RuntimeContext layout | unit | `cargo test -p polyplug_abi layout_runtime_context` | YES |
| TH-05 | VmLoaderData layout | unit | `cargo test -p polyplug_abi layout_vm_loader_data` | YES |
| TH-06 | Host callbacks use RuntimeContext | unit | `cargo test -p polyplug --lib host_callbacks_use_runtime_context` | YES |
| TH-07 | PluginContext typed | unit | `cargo test -p polyplug_abi plugin_context_no_bare_c_void` | YES |
| TH-08 | repr(C) handles | unit | `cargo test -p polyplug_abi repr_c` | YES |

### Wave 0 Gaps

None - all phases have nyquist_compliant: true in VALIDATION.md frontmatter.

## Code Verification Commands

### Phase 02 Registry Verification

```bash
# REG-01: VTableSlot removed
grep -c "VTableSlot" crates/polyplug/src/registry/plugin_registry.rs
# Expected: 0

# REG-02: PluginGuard removed
grep -c "PluginGuard" crates/polyplug/src/registry/plugin_registry.rs
# Expected: 0

# REG-03: PluginHandle has only index
grep -c "generation" crates/polyplug_abi/src/plugin/plugin_handle.rs
# Expected: 0

# REG-04: ArcSwap removed
grep -c "arc_swap" crates/polyplug/Cargo.toml
# Expected: 0

# REG-05: RegistrySlot simplified
grep -A5 "pub(crate) struct RegistrySlot" crates/polyplug/src/registry/plugin_registry.rs
# Expected: struct with entry + interface fields only
```

### Phase 03 Instance Model Verification

```bash
# INST-01-03: Instance wrapper pattern
grep -c "GuestContractInstance" crates/polyplugc/src/generators/rust.rs
# Expected: >= 20

# HC-02/HC-03: Singleton tests
cargo test -p polyplug -- singleton_contract_returns_cached_instance
cargo test -p polyplug -- multi_instance_contract_creates_new_instance

# CG-04: create/destroy_instance in generators
grep -c "create_instance" crates/polyplugc/src/generators/rust.rs
# Expected: >= 10
```

### Phase 04 Hot-Reload Verification

```bash
# HR-01: Quiescence removed
grep -c "wait_for_quiescence" crates/polyplug/src/reload.rs
# Expected: 0

grep -c "QuiescenceTimeout" crates/polyplug/src/error.rs
# Expected: 0

# HR-05: swap_interface exists
grep -c "swap_interface" crates/polyplug/src/reload.rs
# Expected: >= 1

# HR-06: Warning emission
grep -c "emit_warning" crates/polyplug/src/reload.rs
# Expected: >= 1
```

### Phase 07 Typed Handles Verification

```bash
# TH-01-06: RuntimeContext usage
grep -c "rt_ctx: RuntimeContext" crates/polyplug_abi/src/host/runtime_abi.rs
# Expected: >= 8

# TH-08: repr(C) on all handles
grep -l "#\[repr(C)\]" crates/polyplug_abi/src/host/runtime_context.rs \
    crates/polyplug_abi/src/dispatch/vm_loader_data.rs \
    crates/polyplug_abi/src/guest/guest_contract_instance.rs \
    crates/polyplug_abi/src/host/host_contract_instance.rs
# Expected: 4 files
```

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| VERIFICATION.md | Copy VALIDATION.md content | Transform test mappings to evidence statements | Different purpose - evidence vs contract |
| Requirement evidence | List requirement names | Map to specific file/line code artifacts | Auditors need traceable proof |
| Truth verification | Assert "passed" | Show VERIFIED/FAILED with grep evidence | Objective verification |

## Common Pitfalls

### Pitfall 1: Copying VALIDATION.md as VERIFICATION.md

**What goes wrong:** VALIDATION.md defines test commands; VERIFICATION.md needs evidence of execution with results.
**Why it happens:** Similar YAML frontmatter and table structures look alike.
**How to avoid:** VERIFICATION.md has `## Goal Achievement` section with "Status | Evidence" columns showing actual results.
**Warning signs:** VERIFICATION.md contains "Automated Command" column instead of "Evidence".

### Pitfall 2: Missing SUMMARY.md Evidence

**What goes wrong:** Phase 02-03 has no SUMMARY.md but requirements need evidence.
**Why it happens:** Execution completed but summary not written.
**How to avoid:** Use PLAN.md must_haves section + VALIDATION tests + code grep to synthesize evidence.
**Warning signs:** Requirement table has no SUMMARY source.

### Pitfall 3: Asserting All Tests Pass Without Running

**What goes wrong:** VERIFICATION.md claims tests pass without actual execution output.
**Why it happens:** Trusting VALIDATION.md nyquist_compliant flag without verification.
**How to avoid:** Include Behavioral Spot-Checks section with actual test output.
**Warning signs:** No "cargo test: N passed" output in verification.

## Sources

### Primary (HIGH confidence)

- `.planning/ROADMAP.md` - Phase 8 requirements and success criteria [VERIFIED: file read]
- `.planning/REQUIREMENTS.md` - Full requirements list [VERIFIED: file read]
- `.planning/v1.1-MILESTONE-AUDIT.md` - Audit findings showing orphaned requirements [VERIFIED: file read]
- `.planning/phases/01-abi-types/01-VERIFICATION.md` - VERIFICATION.md format reference [VERIFIED: file read]
- `.planning/phases/05-sdk-updates/05-VERIFICATION.md` - VERIFICATION.md format reference [VERIFIED: file read]
- `.planning/phases/06-cleanup/06-VERIFICATION.md` - VERIFICATION.md format reference [VERIFIED: file read]

### Secondary (MEDIUM confidence)

- `.planning/phases/02-registry/02-VALIDATION.md` - Test mappings for Phase 02 [VERIFIED: file read]
- `.planning/phases/03-instance-model/03-VALIDATION.md` - Test mappings for Phase 03 [VERIFIED: file read]
- `.planning/phases/04-hot-reload/04-VALIDATION.md` - Test mappings for Phase 04 [VERIFIED: file read]
- `.planning/phases/07-typed-handles/VALIDATION.md` - Test mappings for Phase 07 [VERIFIED: file read]

### Tertiary (SUMMARY files)

- `.planning/phases/02-registry/02-01-SUMMARY.md` - Evidence for REG-01, REG-02, REG-05 [VERIFIED: file read]
- `.planning/phases/02-registry/02-02-SUMMARY.md` - Evidence for REG-04 [VERIFIED: file read]
- `.planning/phases/03-instance-model/03-01-SUMMARY.md` - Evidence for HC-01, CG-06, CG-01 [VERIFIED: file read]
- `.planning/phases/03-instance-model/03-02-SUMMARY.md` - Evidence for INST-04-06 [VERIFIED: file read]
- `.planning/phases/03-instance-model/03-03-SUMMARY.md` - Evidence for HC-02, HC-03, INST-04 [VERIFIED: file read]
- `.planning/phases/03-instance-model/03-04-SUMMARY.md` - Evidence for INST-01-03, CG-02-04 [VERIFIED: file read]
- `.planning/phases/03-instance-model/03-05-SUMMARY.md` - Evidence for HC-04, CG-05 [VERIFIED: file read]
- `.planning/phases/04-hot-reload/04-01-SUMMARY.md` - Evidence for HR-01, HR-02 [VERIFIED: file read]
- `.planning/phases/04-hot-reload/04-02-SUMMARY.md` - Evidence for HR-03, HR-05, HR-06 [VERIFIED: file read]
- `.planning/phases/04-hot-reload/04-03-SUMMARY.md` - Evidence for HR-04 [VERIFIED: file read]
- `.planning/phases/07-typed-handles/07-01-SUMMARY.md` - Evidence for TH-04, TH-05 [VERIFIED: file read]
- `.planning/phases/07-typed-handles/07-02-SUMMARY.md` - Evidence for TH-01, TH-06 [VERIFIED: file read]
- `.planning/phases/07-typed-handles/07-03-SUMMARY.md` - Evidence for TH-03, TH-07 [VERIFIED: file read]
- `.planning/phases/07-typed-handles/07-04-SUMMARY.md` - Evidence for TH-08 [VERIFIED: file read]

## Metadata

**Confidence breakdown:**
- VERIFICATION.md format: HIGH - 3 example files read
- Evidence sources: HIGH - All SUMMARY files exist except 02-03
- Requirements mapping: HIGH - Audit + REQUIREMENTS.md cross-reference
- Validation tests: HIGH - All VALIDATION.md files exist with nyquist_compliant: true

**Gaps identified:**
- `.planning/phases/02-registry/02-03-SUMMARY.md` does not exist - must use PLAN + tests for evidence

**Research date:** 2026-04-06
**Valid until:** 30 days (documentation phase, stable format)