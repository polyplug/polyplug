# Phase 17: Refactor ContractRegistry to unified RuntimeStore - Context

**Gathered:** 2026-04-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Rename `ContractRegistry` to `RuntimeStore`, add `BundleDescriptor` for bundle metadata, change bundle-level dependencies from contract-level, and optimize `find_slots_by_bundle()` from O(n) scan to O(1) lookup.

**Hierarchy:** Bundle → contains Plugins → each plugin implements a Guest Contract

</domain>

<decisions>
## Implementation Decisions

### Type Renaming

- **D-01:** `ContractRegistry` → `RuntimeStore`
- **D-02:** `RegistryEntry` → `PluginEntry` — entry represents a plugin
- **D-03:** `RegistrySlot` → `PluginSlot` — slot holds a plugin
- **D-04:** `RegistryData` → `RuntimeStoreData` — match new type name

### Method Renaming (Guest Contract prefix)

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

### Internal Field Renaming

- **D-17:** `contract_index` → `guest_contract_index`
- **D-18:** `bundle_index` → `bundle_slots_index`
- **D-19:** `declared_deps` → `bundle_declared_deps`

### Bundle Index Structure

- **D-20:** Create `BundleData` struct containing:
  ```rust
  pub struct BundleData {
      /// All slot indices for plugins from this bundle.
      pub plugin_slots: Vec<u32>,
      /// Bundle metadata.
      pub descriptor: BundleDescriptor,
  }
  ```
- **D-21:** Change `bundle_index: HashMap<BundleId, u32>` to `bundle_data: HashMap<BundleId, BundleData>`
  - Stores ALL slot indices (not just first) — enables O(1) `get_bundle_plugin_slots()`
  - Also stores bundle descriptor — bundle metadata now in RuntimeStore

### Bundle Name Index

- **D-22:** Add `bundle_name_index: HashMap<String, Vec<BundleId>>`
  - Maps bundle name → ALL loaded version BundleIds
  - Enables O(1) name → ID resolution for dependencies
- **D-23:** Multiple versions of same bundle name map to Vec of BundleIds

### BundleDescriptor

- **D-24:** Create `BundleDescriptor` struct:
  ```rust
  pub struct BundleDescriptor {
      pub id: BundleId,
      pub name: String,
      pub version: Version,              // bundle's own version
      pub runtime: RuntimeLanguage,
      pub file_path: PathBuf,
      pub dependencies: Vec<BundleDependency>,
  }
  ```
- **D-25:** Create `BundleDependency` struct:
  ```rust
  pub struct BundleDependency {
      pub name: String,              // bundle name
      pub min_version: Option<Version>,  // None = any version
  }
  ```

### Bundle-Level Dependencies (Replaces Contract-Level)

- **D-26:** Replace `[[dependency]]` table with bundle-level dependencies
- **D-27:** manifest.toml syntax:
  ```toml
  [bundle]
  name = "my-plugin"
  version = "1.0.0"
  dependencies = ["image-decoder@1.0", "audio-encoder"]  # @version = min_version
  ```
- **D-28:** Dependency parsing:
  - `"image-decoder"` → `{ name: "image-decoder", min_version: None }`
  - `"image-decoder@1.0"` → `{ name: "image-decoder", min_version: Some(Version::new(1, 0, 0)) }`
- **D-29:** Resolution: lookup name in `bundle_name_index`, get BundleIds, check version if specified

### Multi-Version Handling

- **D-30:** If multiple versions of same bundle loaded, versionless dependency resolves to ALL versions
- **D-31:** `get_bundles_by_name("image-decoder")` returns all loaded version BundleIds
- **D-32:** Plugin can access contracts from any resolved version

### New RuntimeStore APIs

- **D-33:** `list_bundles() -> Vec<BundleId>` — iterate all loaded bundles
- **D-34:** `get_bundle_descriptor(BundleId) -> Option<&BundleDescriptor>` — get bundle metadata
- **D-35:** `get_bundle_plugin_slots(BundleId) -> Vec<u32>` — O(1) slot lookup
- **D-36:** `get_bundles_by_name(String) -> Vec<BundleId>` — name → IDs (all versions)

### Migration Order

- **D-37:** Pass 1: Rename types and methods (ContractRegistry → RuntimeStore, all method names)
- **D-38:** Pass 2: Add BundleData, BundleDescriptor, bundle_name_index, new APIs

### Claude's Discretion

- Exact error types for dependency resolution failures
- Whether to add helper methods to BundleDescriptor
- Internal organization of RuntimeStoreData fields

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Core Files
- `crates/polyplug/src/registry/contract_registry.rs` — ContractRegistry to rename
- `crates/polyplug/src/runtime.rs` — Runtime that owns the registry
- `crates/polyplug/src/loader/manifest.rs` — ManifestData, RawManifestDependency (to replace)
- `crates/polyplug_abi/src/types/version.rs` — Version struct for bundle version

### ABI Types
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` — GuestContractInterface
- `crates/polyplug_abi/src/host/host_contract_interface.rs` — HostContractInterface
- `crates/polyplug_utils/src/bundle_id.rs` — BundleId

### AGENTS.md Rules
- `AGENTS.md` §3 — Explicit types required
- `AGENTS.md` §14 — No backward compatibility code
- `AGENTS.md` §16 — No type aliases

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `ContractRegistry` struct with RwLock pattern — keep locking strategy
- `Version` struct in polyplug_abi — reuse for bundle version
- `BundleId` in polyplug_utils — reuse as-is

### Established Patterns
- Single `RwLock<RuntimeStoreData>` for all mutable state
- `#[repr(C)]` for FFI types, not needed for RuntimeStore (internal)
- O(1) lookup patterns: contract_index, now bundle_data

### Integration Points
- `Runtime` owns `ContractRegistry` → will own `RuntimeStore`
- `ffi.rs` calls registry methods → update all call sites
- `reload.rs` calls `find_slots_by_bundle()` → update to `get_bundle_plugin_slots()`
- `loader/mod.rs` parses manifest → update to new dependency format

</code_context>

<specifics>
## Specific Ideas

- Bundle contains plugins, each plugin implements a Guest Contract
- Dependencies are bundle-level, not contract-level
- Plugin dev writes `"bundle-name@1.0"` in manifest — simple, readable
- Multi-version: versionless dep sees ALL versions of that bundle
- All "Guest Contract" naming for consistency with Guest/Host terminology

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---
*Phase: 17-refactor-contractregistry-to-unified-runtimestore*
*Context gathered: 2026-04-10*