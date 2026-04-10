# Phase 17: Refactor ContractRegistry to unified RuntimeStore - Research

**Researched:** 2026-04-10
**Domain:** Rust internal architecture refactor — registry → unified bundle/plugin store
**Confidence:** HIGH

## Summary

This phase refactors `ContractRegistry` to `RuntimeStore`, adding bundle-level indexing for O(1) slot lookup and bundle metadata management. The changes consolidate bundle metadata currently split between `Runtime` and the registry, enable bundle-level dependencies instead of contract-level, and standardize naming with "Guest Contract" prefix.

**Primary recommendation:** Execute in two passes per CONTEXT.md — Pass 1: rename types/methods/fields, Pass 2: add BundleData/BundleDescriptor/bundle_name_index and new APIs.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Type Renaming
- **D-01:** `ContractRegistry` → `RuntimeStore`
- **D-02:** `RegistryEntry` → `PluginEntry` — entry represents a plugin
- **D-03:** `RegistrySlot` → `PluginSlot` — slot holds a plugin
- **D-04:** `RegistryData` → `RuntimeStoreData` — match new type name

#### Method Renaming (Guest Contract prefix)
- **D-05:** `find_by_contract` → `find_guest_contract`
- **D-06:** `find_all_by_contract` → `find_all_guest_contracts`
- **D-07:** `count_by_contract` → `count_guest_contracts`
- **D-08:** `find_by_bundle` → `find_guest_contract_by_bundle`
- **D-09:** `find_slots_by_bundle` → `get_bundle_plugin_slots`
- **D-10:** `resolve` → `resolve_guest_contract`
- **D-11:** `swap_interface` → `swap_guest_contract_interface`
- **D-12:** `register` → `register_guest_contract`
- **D-13:** `declare_deps` → `declare_bundle_dependencies`
- **D-14:** `is_dependency_declared` → `is_bundle_dependency_declared`
- **D-15:** `get_slot_contract_id` → `get_slot_guest_contract_id`
- **D-16:** `get_interface_arc` → `get_guest_contract_interface_arc`

#### Internal Field Renaming
- **D-17:** `contract_index` → `guest_contract_index`
- **D-18:** `bundle_index` → `bundle_slots_index`
- **D-19:** `declared_deps` → `bundle_declared_deps`

#### Bundle Index Structure
- **D-20:** Create `BundleData` struct containing `plugin_slots: Vec<u32>` and `descriptor: BundleDescriptor`
- **D-21:** Change `bundle_index: HashMap<BundleId, u32>` to `bundle_data: HashMap<BundleId, BundleData>` — stores ALL slot indices (not just first) + bundle descriptor

#### Bundle Name Index
- **D-22:** Add `bundle_name_index: HashMap<String, Vec<BundleId>>` — maps bundle name → ALL loaded version BundleIds
- **D-23:** Multiple versions of same bundle name map to Vec of BundleIds

#### BundleDescriptor
- **D-24:** Create `BundleDescriptor` struct: `{ id: BundleId, name: String, version: Version, runtime: RuntimeLanguage, file_path: PathBuf, dependencies: Vec<BundleDependency> }`
- **D-25:** Create `BundleDependency` struct: `{ name: String, min_version: Option<Version> }`

#### Bundle-Level Dependencies
- **D-26:** Replace `[[dependency]]` table with bundle-level dependencies
- **D-27:** manifest.toml syntax: `dependencies = ["image-decoder@1.0", "audio-encoder"]`
- **D-28:** Dependency parsing: `"image-decoder"` → `{ name, min_version: None }`, `"image-decoder@1.0"` → `{ name, min_version: Some(Version::new(1, 0, 0)) }`
- **D-29:** Resolution via `bundle_name_index`

#### Multi-Version Handling
- **D-30:** Versionless dependency resolves to ALL versions
- **D-31:** `get_bundles_by_name("image-decoder")` returns all loaded version BundleIds
- **D-32:** Plugin can access contracts from any resolved version

#### New RuntimeStore APIs
- **D-33:** `list_bundles() -> Vec<BundleId>`
- **D-34:** `get_bundle_descriptor(BundleId) -> Option<&BundleDescriptor>`
- **D-35:** `get_bundle_plugin_slots(BundleId) -> Vec<u32>` — O(1) lookup
- **D-36:** `get_bundles_by_name(String) -> Vec<BundleId>`

#### Migration Order
- **D-37:** Pass 1: Rename types and methods
- **D-38:** Pass 2: Add BundleData, BundleDescriptor, bundle_name_index, new APIs

### Claude's Discretion
- Exact error types for dependency resolution failures
- Whether to add helper methods to BundleDescriptor
- Internal organization of RuntimeStoreData fields

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-01 | find_slots_by_bundle() becomes O(1) lookup | BundleData.plugin_slots Vec enables O(1) return via `bundle_data.get(bundle_id).map(|bd| bd.plugin_slots.clone())` |
| REQ-02 | Bundle metadata available through RuntimeStore | BundleDescriptor stored in BundleData within RuntimeStoreData |
| REQ-03 | All tests pass with renamed types | Test files identified: `registry_edge_cases.rs`, `stress_error.rs`, `stress_concurrent_registry.rs` — update imports and type names |
| REQ-04 | All AGENTS.md rules followed | AGENTS.md §3, §14, §16, §4, §5 documented below — no type aliases, explicit types, no deprecated code |
</phase_requirements>

## Standard Stack

### Core Types (Existing — rename only)

| Type | Current Location | Renamed To | Location After Rename |
|------|-----------------|------------|----------------------|
| ContractRegistry | `registry/contract_registry.rs` | RuntimeStore | `registry/runtime_store.rs` (file rename optional, or keep filename) |
| RegistryEntry | `registry/contract_registry.rs:23-30` | PluginEntry | same file |
| RegistrySlot | `registry/contract_registry.rs:33-39` | PluginSlot | same file |
| RegistryData | `registry/contract_registry.rs:45-54` | RuntimeStoreData | same file |

### New Types (Add in Pass 2)

| Type | Location | Purpose |
|------|----------|---------|
| BundleData | `registry/runtime_store.rs` | Holds `plugin_slots: Vec<u32>` + `descriptor: BundleDescriptor` for O(1) bundle lookup |
| BundleDescriptor | `registry/runtime_store.rs` or new `bundle.rs` | Bundle metadata: id, name, version, runtime, file_path, dependencies |
| BundleDependency | `registry/runtime_store.rs` or new `bundle.rs` | Dependency spec: name + optional min_version |

### Supporting Types (Reuse)

| Type | Location | Usage |
|------|----------|-------|
| BundleId | `polyplug_utils/src/bundle_id.rs` | As-is — FNV-1a hash of bundle name |
| Version | `polyplug_abi/src/types/version.rs` | As-is — semantic version struct |
| GuestContractId | `polyplug_abi/src/guest/guest_contract_id.rs` | As-is — contract identifier |
| GuestContractInterface | `polyplug_abi/src/guest/guest_contract_interface.rs` | As-is — plugin interface |

**No installation required** — internal Rust refactor, all types already in workspace.

## Architecture Patterns

### Recommended Structure After Refactor

```
crates/polyplug/src/
├── registry/
│   └── mod.rs               # pub use runtime_store::RuntimeStore
│   └── runtime_store.rs     # RuntimeStore, RuntimeStoreData, PluginEntry, PluginSlot
│   └── bundle.rs            # BundleData, BundleDescriptor, BundleDependency (optional submod)
├── runtime.rs               # Runtime owns Arc<RuntimeStore>
├── reload.rs                # Uses get_bundle_plugin_slots()
└── loader/
    └── manifest.rs          # ManifestData with new dependency format
```

### Pattern 1: O(1) Bundle Slot Lookup

**What:** Replace O(n) scan with pre-indexed Vec lookup.

**Current (O(n)):**
```rust
// Source: crates/polyplug/src/registry/contract_registry.rs:487-502
pub fn find_slots_by_bundle(&self, bundle_id: BundleId) -> Vec<u32> {
    let data: std::sync::RwLockReadGuard<'_, RegistryData> = self.data.read().unwrap_or_else(...);
    let mut result: Vec<u32> = Vec::new();
    for (i, slot) in data.slots.iter().enumerate() {
        if let Some(ref entry) = slot.entry && entry.bundle_id == bundle_id {
            result.push(i as u32);
        }
    }
    result
}
```

**After (O(1)):**
```rust
// Source: From CONTEXT.md D-21, D-35
pub fn get_bundle_plugin_slots(&self, bundle_id: BundleId) -> Vec<u32> {
    let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> = self.data.read().unwrap_or_else(...);
    data.bundle_data.get(&bundle_id)
        .map(|bd: &BundleData| bd.plugin_slots.clone())
        .unwrap_or_default()
}
```

### Pattern 2: Bundle Data Storage

**What:** Store complete bundle metadata in registry, not split across Runtime.

**Current (split):**
- `Runtime.bundle_manifests: Mutex<HashMap<String, ManifestData>>` — name → manifest
- `ContractRegistry.bundle_index: HashMap<BundleId, u32>` — only first slot index

**After (consolidated):**
```rust
// Source: From CONTEXT.md D-20, D-21
struct BundleData {
    pub plugin_slots: Vec<u32>,      // ALL slots, not just first
    pub descriptor: BundleDescriptor, // Bundle metadata
}

struct RuntimeStoreData {
    slots: Vec<PluginSlot>,
    guest_contract_index: HashMap<GuestContractId, Vec<u32>>,
    bundle_data: HashMap<BundleId, BundleData>,  // replaces bundle_index
    bundle_name_index: HashMap<String, Vec<BundleId>>, // new: name → IDs
    bundle_declared_deps: HashMap<BundleId, HashSet<GuestContractId>>,
}
```

### Pattern 3: Bundle-Level Dependencies

**What:** Dependencies reference bundles, not individual contracts.

**Current manifest.toml:**
```toml
[[dependency]]
kind = "contract"
contract = "image.decode"
min_version = "1.0"
```

**After manifest.toml:**
```toml
[bundle]
name = "my-plugin"
version = "1.0.0"
dependencies = ["image-decoder@1.0", "audio-encoder"]
```

**Dependency parsing logic:**
```rust
// Source: From CONTEXT.md D-28
fn parse_dependency_spec(spec: &str) -> BundleDependency {
    match spec.split_once('@') {
        Some((name, version_str)) => BundleDependency {
            name: name.to_string(),
            min_version: Some(version_str.parse::<Version>().expect("valid version")),
        },
        None => BundleDependency {
            name: spec.to_string(),
            min_version: None,
        },
    }
}
```

### Anti-Patterns to Avoid

- **Type aliases:** AGENTS.md §16 forbids `pub type OldName = NewName` — use canonical names everywhere
- **Deprecated code:** AGENTS.md §14 forbids backward compatibility shims — remove old names entirely
- **Implicit types:** AGENTS.md §3 requires explicit type annotations — annotate all local bindings

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Bundle ID hashing | Custom hash function | `BundleId::new(name)` via `fnv1a_64` | FNV-1a is stable, already in polyplug_utils |
| Version parsing | Custom split/parse | `Version::from_str()` | Already handles "major.minor.patch" format |
| Dependency resolution | Custom lookup logic | `bundle_name_index` HashMap | O(1) name → BundleIds |

**Key insight:** All required types exist in workspace. This is a pure internal refactor with no new external dependencies.

## Runtime State Inventory

> This is a rename/refactor phase affecting internal Rust types only.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — RuntimeStore is internal Rust struct, no external DB | Code edit only |
| Live service config | None — no external services | Code edit only |
| OS-registered state | None — pure Rust library | Code edit only |
| Secrets/env vars | None — no secrets involved | Code edit only |
| Build artifacts | Cargo compilation artifacts will rebuild automatically | `cargo build` after changes |

**Nothing found in any external category** — all changes are internal Rust code.

## Common Pitfalls

### Pitfall 1: Incomplete Rename Propagation

**What goes wrong:** Type renamed in one file but imports/tests still use old name, causing compile errors.

**Why it happens:** `ContractRegistry` is imported in 4+ files (runtime.rs, runtime_builder.rs, mod.rs, tests).

**How to avoid:** Use two-pass migration per D-37/D-38. Pass 1: search-and-replace all imports before adding new features.

**Warning signs:** `cargo check` fails with "cannot find type ContractRegistry in this scope".

### Pitfall 2: Bundle Index Not Updated on Register

**What goes wrong:** `bundle_data.plugin_slots` not populated when new plugin registers, breaking O(1) lookup.

**Why it happens:** Current `register()` only updates `bundle_index` with first slot index (line 169).

**How to avoid:** Update `register_guest_contract()` to push ALL slots into `bundle_data.entry(bundle_id).or_default().plugin_slots.push(slot_idx)`.

**Warning signs:** `get_bundle_plugin_slots()` returns empty Vec for bundles with plugins.

### Pitfall 3: Manifest Dependency Format Mismatch

**What goes wrong:** Manifest parsing expects old `[[dependency]]` format, new bundles use `dependencies = [...]` array.

**Why it happens:** `RawManifestDependency` struct expects `kind`, `contract`, `min_version` fields.

**How to avoid:** Replace `RawManifestDependency` with `BundleDependency` parsing per D-27/D-28. Keep backward compat? No — D-26 says replace entirely (AGENTS.md §14).

**Warning signs:** `toml::from_str` fails with "missing field `kind`".

### Pitfall 4: Bundle Name Index Missing Versions

**What goes wrong:** `bundle_name_index` only stores one BundleId per name, breaking multi-version lookup.

**Why it happens:** HashMap key collision overwrites previous version.

**How to avoid:** Use `HashMap<String, Vec<BundleId>>` per D-22. On registration: `bundle_name_index.entry(name).or_default().push(bundle_id)`.

**Warning signs:** `get_bundles_by_name()` returns only one BundleId when multiple versions loaded.

## Code Examples

### BundleDescriptor Struct

```rust
// Source: From CONTEXT.md D-24
use polyplug_utils::BundleId;
use polyplug_abi::types::Version;
use polyplug_abi::RuntimeLanguage;
use std::path::PathBuf;

pub struct BundleDescriptor {
    pub id: BundleId,
    pub name: String,
    pub version: Version,
    pub runtime: RuntimeLanguage,
    pub file_path: PathBuf,
    pub dependencies: Vec<BundleDependency>,
}
```

### BundleData and RuntimeStoreData

```rust
// Source: From CONTEXT.md D-20, D-21
pub struct BundleData {
    pub plugin_slots: Vec<u32>,
    pub descriptor: BundleDescriptor,
}

struct RuntimeStoreData {
    slots: Vec<PluginSlot>,
    guest_contract_index: HashMap<GuestContractId, Vec<u32>>,
    bundle_data: HashMap<BundleId, BundleData>,
    bundle_name_index: HashMap<String, Vec<BundleId>>,
    bundle_declared_deps: HashMap<BundleId, HashSet<GuestContractId>>,
}
```

### O(1) get_bundle_plugin_slots

```rust
// Source: From CONTEXT.md D-35
impl RuntimeStore {
    pub fn get_bundle_plugin_slots(&self, bundle_id: BundleId) -> Vec<u32> {
        let data: std::sync::RwLockReadGuard<'_, RuntimeStoreData> =
            self.data.read().unwrap_or_else(|e| {
                eprintln!("[polyplug] RwLock poisoned, recovering: {}", e);
                e.into_inner()
            });
        data.bundle_data
            .get(&bundle_id)
            .map(|bd: &BundleData| bd.plugin_slots.clone())
            .unwrap_or_default()
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Contract-level dependencies | Bundle-level dependencies | This phase | Simpler manifest syntax, multi-version support |
| `find_slots_by_bundle()` O(n) scan | `get_bundle_plugin_slots()` O(1) lookup | This phase | Hot-reload slot discovery faster for bundles with many plugins |
| Bundle metadata in Runtime | Bundle metadata in RuntimeStore | This phase | Single source of truth, no split across Runtime |

**Deprecated/outdated:**
- `RawManifestDependency` with `kind`, `contract`, `bundle`, `contract_id`, `bundle_id` fields → Replace with `BundleDependency`
- `ManifestDependency` enum (ByContract/ByBundle) → Replace with bundle-level only per D-26

## Assumptions Log

> All claims in this research were verified against source files.

| # | Claim | Section | Source |
|---|-------|---------|--------|
| A1 | ContractRegistry uses RwLock<RegistryData> pattern | Architecture | [VERIFIED: contract_registry.rs:75-76] |
| A2 | Version struct supports "major.minor.patch" parsing | Standard Stack | [VERIFIED: version.rs:40-73] |
| A3 | BundleId uses FNV-1a hash | Standard Stack | [VERIFIED: bundle_id.rs:23-25] |
| A4 | find_slots_by_bundle is O(n) scan | Architecture | [VERIFIED: contract_registry.rs:494-500] |
| A5 | Tests use ContractRegistry directly | Validation | [VERIFIED: registry_edge_cases.rs, stress_error.rs] |

**No user confirmation needed** — all claims verified against codebase.

## Open Questions

1. **Should BundleDescriptor/BundleDependency be in a separate `bundle.rs` submodule?**
   - What we know: AGENTS.md §1 prefers `filename.rs` for single-file modules, `filename/mod.rs` only for multi-file modules.
   - What's unclear: If adding `bundle.rs` creates a submodule or just a separate file in registry/.
   - Recommendation: Keep in `runtime_store.rs` initially; split to `bundle.rs` if complexity grows (AGENTS.md §1).

2. **Should `bundle.rs` use `mod.rs` pattern?**
   - What we know: AGENTS.md §1 forbids `folder/mod.rs` with no submodules.
   - Recommendation: Use `registry/bundle.rs` as flat file, not `registry/bundle/mod.rs`.

## Environment Availability

> Step 2.6: SKIPPED — no external dependencies. Pure Rust internal refactor.

## Validation Architecture

> nyquist_validation enabled per .planning/config.json workflow.nyquist_validation = true

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust `#[test]` + cargo test |
| Config file | None — Cargo.toml workspace test config |
| Quick run command | `cargo test -p polyplug --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-01 | O(1) bundle slot lookup | unit | `cargo test -p polyplug --lib get_bundle_plugin_slots` | ❌ Wave 0 — new test needed |
| REQ-02 | Bundle metadata in RuntimeStore | unit | `cargo test -p polyplug --lib get_bundle_descriptor` | ❌ Wave 0 — new test needed |
| REQ-03 | Tests pass with renamed types | integration | `cargo test -p polyplug` | ✅ Existing tests need import updates |
| REQ-04 | AGENTS.md rules followed | lint | `cargo clippy --workspace -- -D warnings` | ✅ CI validates |

### Wave 0 Gaps

- [ ] `tests/test_runtime_store.rs` — covers REQ-01, REQ-02 (new test file for O(1) lookup and bundle metadata)
- [ ] Update imports in `registry_edge_cases.rs`, `stress_error.rs`, `stress_concurrent_registry.rs`
- [ ] No framework install needed — Rust `#[test]` built-in

## Security Domain

> No security implications — internal Rust refactor, no FFI boundary changes, no authentication/access control.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes (manifest parsing) | `toml::from_str` with error handling |
| V6 Cryptography | no | — |

### Known Threat Patterns for Rust Registry Refactor

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Manifest injection | Tampering | Validate manifest fields, reject malformed TOML |
| Dependency confusion | Tampering | Bundle name resolution via `bundle_name_index` — exact match required |

## Sources

### Primary (HIGH confidence)
- `crates/polyplug/src/registry/contract_registry.rs` — ContractRegistry implementation
- `crates/polyplug/src/runtime.rs` — Runtime integration
- `crates/polyplug/src/loader/manifest.rs` — ManifestData, RawManifestDependency
- `crates/polyplug_abi/src/types/version.rs` — Version struct
- `crates/polyplug_utils/src/bundle_id.rs` — BundleId struct
- `AGENTS.md` — Project rules (§3, §4, §5, §14, §16)

### Secondary (MEDIUM confidence)
- `.planning/phases/17-CONTEXT.md` — User decisions (locked)
- `.planning/REQUIREMENTS.md` — v1.1 requirements context
- `.planning/STATE.md` — Project state

### Tertiary (LOW confidence)
- None — all claims verified against codebase

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all types verified in existing codebase
- Architecture: HIGH — patterns extracted from actual implementation
- Pitfalls: HIGH — based on codebase analysis and AGENTS.md rules

**Research date:** 2026-04-10
**Valid until:** 30 days — stable Rust codebase, no external dependency changes

---

*Research complete. Ready for planning.*