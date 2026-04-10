# Phase 17: Refactor ContractRegistry to unified RuntimeStore - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-10
**Phase:** 17-refactor-contractregistry-to-unified-runtimestore
**Areas discussed:** API naming, Migration order, Bundle index structure, Dependency model, Multi-version handling

---

## API Naming

| Option | Description | Selected |
|--------|-------------|----------|
| Rename to clearer names | find_slots_by_bundle → get_bundle_plugin_slots, add "Guest" prefix to contract methods | ✓ |
| Keep existing names | Minimal diff but inconsistent with RuntimeStore naming | |

**User's choice:** Rename to clearer names with "Guest" prefix
**Notes:** Bundle contains plugins, plugin is implementation of Guest Contract. All contract-related names get "Guest" prefix.

---

## Migration Order

| Option | Description | Selected |
|--------|-------------|----------|
| Rename first, then index | Rename types first (easier diff review), then add indexing and BundleDescriptor | ✓ |
| Index first, then rename | Performance fix first, then rename | |
| All in one refactor | Single commit but larger diff | |

**User's choice:** Rename first, then add indexing

---

## Bundle Index Structure

| Option | Description | Selected |
|--------|-------------|----------|
| Create BundleData struct | BundleData { plugin_slots: Vec<u32>, descriptor: BundleDescriptor }, HashMap<BundleId, BundleData> | ✓ |
| Extend bundle_index to Vec<u32> | Just change HashMap<BundleId, u32> → HashMap<BundleId, Vec<u32>> | |

**User's choice:** Create BundleData struct with slots + metadata
**Notes:** User wants hosts to retrieve and iterate over bundles

---

## Dependency Model

| Option | Description | Selected |
|--------|-------------|----------|
| Replace with bundle deps | dependencies = ["bundle-name@1.0"] — bundle-level, not contract-level | ✓ |
| Keep both models | Bundle deps for access, contract deps for specificity | |
| Keep current only | [[dependency]] table with ByContract/ByBundle | |

**User's choice:** Replace contract-level dependencies with bundle-level dependencies
**Notes:** Simpler model: declare bundle dependency, then access any contract from that bundle

---

## Dependency Version Specification

| Option | Description | Selected |
|--------|-------------|----------|
| Name + optional min_version | "bundle-name@1.0" or just "bundle-name", min_version: Option<Version> | ✓ |
| Semver range string | "bundle-name@>=1.0.0,<2.0.0" — more complex parsing | |
| No versioning | Just names, no version constraints | |

**User's choice:** Name + optional min_version
**Notes:** Follows existing polyplug pattern (min_version for contracts). Simple and predictable.

---

## Bundle Name Resolution (for dependencies)

| Option | Description | Selected |
|--------|-------------|----------|
| Add name index (O(1)) | bundle_name_index: HashMap<String, Vec<BundleId>> | ✓ |
| Scan on demand (O(n)) | Scan bundle_data values to find by name | |

**User's choice:** Add name index for O(1) lookup
**Notes:** Required for dependency resolution at load time

---

## Multi-Version Handling

| Option | Description | Selected |
|--------|-------------|----------|
| Use all versions | Versionless dep sees ALL loaded versions of that bundle | ✓ |
| Use highest version | Automatically pick newest | |
| Reject ambiguous dep | Error if multiple versions exist without version spec | |
| Use first loaded | Whichever was loaded first | |

**User's choice:** Use all versions
**Notes:** Plugin can access contracts from any version. bundle_name_index maps name → Vec<BundleId>.

---

## Claude's Discretion

- Exact error types for dependency resolution failures
- Whether to add helper methods to BundleDescriptor
- Internal organization of RuntimeStoreData fields

---

## Deferred Ideas

None — discussion stayed within phase scope.