//! Manifest — manifest.toml parsing for plugin bundles.
//!
//! Reads the companion `manifest.toml` for a plugin bundle before loading.
//! The `runtime` field determines which `BundleLoader` handles the bundle.
//! If absent, defaults to `"native"`.

use std::collections::HashMap;
use std::path::PathBuf;

fn default_runtime() -> String {
    "native".to_owned()
}

/// Raw dependency declaration from a `[[dependency]]` table in `manifest.toml`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RawManifestDependency {
    pub kind: String,
    pub contract: String,
    pub min_version: String,
    #[serde(default)]
    pub bundle: Option<String>,
    #[serde(default)]
    pub contract_id: u64,
    #[serde(default)]
    pub bundle_id: Option<u64>,
}

impl RawManifestDependency {
    /// Resolve this raw dependency into a typed `ManifestDependency`.
    ///
    /// Returns `None` if the dependency is a ByBundle dep without a `bundle_id` (and emits a warning).
    pub fn resolve(&self) -> Option<ManifestDependency> {
        match &self.bundle {
            None => Some(ManifestDependency::ByContract {
                contract: self.contract.clone(),
                contract_id: self.contract_id,
                min_version: self.min_version.clone(),
            }),
            Some(bundle) => match self.bundle_id {
                None => {
                    eprintln!(
                        "[polyplug] warning: ByBundle dep '{}' has no bundle_id; skipping",
                        self.contract
                    );
                    None
                }
                Some(bid) => Some(ManifestDependency::ByBundle {
                    bundle: bundle.clone(),
                    bundle_id: bid,
                    contract: self.contract.clone(),
                    contract_id: self.contract_id,
                    min_version: self.min_version.clone(),
                }),
            },
        }
    }
}

/// Resolved dependency — either a contract-only or bundle+contract form.
#[derive(Debug, Clone)]
pub enum ManifestDependency {
    ByContract {
        contract: String,
        contract_id: u64,
        min_version: String,
    },
    ByBundle {
        bundle: String,
        bundle_id: u64,
        contract: String,
        contract_id: u64,
        min_version: String,
    },
}

/// Data parsed from a bundle's `manifest.toml`.
///
/// Only `runtime` is read in this epic. Additional fields are added in Epic 12.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ManifestData {
    /// The runtime required to load this bundle.
    /// Matched against `BundleLoader::runtime_name()` during dispatch.
    /// Defaults to `"native"` when the field is absent from the TOML file.
    #[serde(default = "default_runtime")]
    pub runtime: String,
    /// Bundle name — used by the loader to compute bundle_id via abi::bundle_id()
    #[serde(default)]
    pub bundle_name: String,
    /// Raw dependency declarations from [[dependency]] table in manifest.toml
    #[serde(default, rename = "dependency")]
    pub dependencies: Vec<RawManifestDependency>,
    /// Computed from bundle_name by the loader after parsing; NOT in the TOML
    #[serde(skip)]
    pub bundle_id: u64,
    /// Human-readable name of the plugin bundle
    #[serde(default)]
    pub name: String,
    /// Version string for this bundle
    #[serde(default)]
    pub version: String,
    /// Path to the shared library file (relative to bundle root)
    #[serde(default)]
    pub file: String,
    /// List of contract names this bundle provides implementations for
    #[serde(default)]
    pub provides: Vec<String>,
    /// Map from contract name to number of exported functions
    #[serde(default)]
    pub function_count: HashMap<String, u32>,
    /// Whether this bundle needs re-initialization when a dependency is hot-reloaded.
    /// Defaults to false. Most bundles do not need it.
    #[serde(default)]
    pub needs_reinit_on_dep_reload: bool,
    #[serde(skip)]
    pub path: PathBuf,
}

impl ManifestData {
    /// Resolve all raw dependencies into typed `ManifestDependency` values.
    ///
    /// Deps with missing `bundle_id` are silently skipped (with a warning printed).
    pub fn resolved_dependencies(&self) -> Vec<ManifestDependency> {
        self.dependencies
            .iter()
            .filter_map(|dep: &RawManifestDependency| dep.resolve())
            .collect::<Vec<ManifestDependency>>()
    }
}

#[cfg(test)]
mod tests {
    use super::{ManifestDependency, RawManifestDependency};

    #[test]
    fn raw_dep_resolve_by_contract() {
        let dep = RawManifestDependency {
            kind: "contract".to_owned(),
            contract: "math".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: None,
            contract_id: 42,
            bundle_id: None,
        };
        let resolved: Option<ManifestDependency> = dep.resolve();
        match resolved.expect("should resolve") {
            ManifestDependency::ByContract {
                contract,
                contract_id,
                min_version,
            } => {
                assert_eq!(contract, "math");
                assert_eq!(contract_id, 42);
                assert_eq!(min_version, "1.0");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn raw_dep_resolve_by_bundle() {
        let dep = RawManifestDependency {
            kind: "bundle".to_owned(),
            contract: "math".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: Some("math-bundle".to_owned()),
            contract_id: 42,
            bundle_id: Some(99),
        };
        let resolved: Option<ManifestDependency> = dep.resolve();
        match resolved.expect("should resolve") {
            ManifestDependency::ByBundle {
                bundle,
                bundle_id,
                contract,
                contract_id,
                min_version,
            } => {
                assert_eq!(bundle, "math-bundle");
                assert_eq!(bundle_id, 99);
                assert_eq!(contract, "math");
                assert_eq!(contract_id, 42);
                assert_eq!(min_version, "1.0");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn raw_dep_resolve_by_bundle_missing_bundle_id_returns_none() {
        let dep = RawManifestDependency {
            kind: "bundle".to_owned(),
            contract: "math".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: Some("math-bundle".to_owned()),
            contract_id: 42,
            bundle_id: None,
        };
        let resolved: Option<ManifestDependency> = dep.resolve();
        assert!(
            resolved.is_none(),
            "expected None when bundle_id is missing"
        );
    }
}
