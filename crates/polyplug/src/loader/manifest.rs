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

    /// Validate that the `file` field is non-empty after parsing.
    ///
    /// # Errors
    /// Returns `Err(ManifestMissingFile)` if the `file` field is absent or whitespace-only.
    pub fn validate_file(&self) -> Result<(), crate::error::LoaderError> {
        if self.file.trim().is_empty() {
            return Err(crate::error::LoaderError::ManifestMissingFile {
                bundle: self.bundle_name.clone(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ManifestData, ManifestDependency, RawManifestDependency};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_manifest(file: &str, bundle_name: &str) -> ManifestData {
        ManifestData {
            runtime: "native".to_owned(),
            bundle_name: bundle_name.to_owned(),
            dependencies: Vec::new(),
            bundle_id: 0,
            name: String::new(),
            version: String::new(),
            file: file.to_owned(),
            provides: Vec::new(),
            function_count: HashMap::new(),
            needs_reinit_on_dep_reload: false,
            path: PathBuf::new(),
        }
    }

    // ── validate_file edge cases ──────────────────────────────────────────

    #[test]
    fn validate_file_ok_when_file_is_set() {
        let m: ManifestData = make_manifest("myplugin.so", "myplugin");
        assert!(m.validate_file().is_ok(), "non-empty file must pass validation");
    }

    #[test]
    fn validate_file_err_when_file_is_empty_string() {
        let m: ManifestData = make_manifest("", "myplugin");
        let result: Result<(), crate::error::LoaderError> = m.validate_file();
        assert!(
            result.is_err(),
            "empty file field must fail validation"
        );
        match result.unwrap_err() {
            crate::error::LoaderError::ManifestMissingFile { bundle } => {
                assert_eq!(bundle, "myplugin");
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    #[test]
    fn validate_file_err_when_file_is_whitespace_only() {
        let m: ManifestData = make_manifest("   \t\n  ", "myplugin");
        let result: Result<(), crate::error::LoaderError> = m.validate_file();
        assert!(
            result.is_err(),
            "whitespace-only file field must fail validation"
        );
        match result.unwrap_err() {
            crate::error::LoaderError::ManifestMissingFile { bundle } => {
                assert_eq!(bundle, "myplugin");
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    #[test]
    fn validate_file_err_carries_bundle_name() {
        let m: ManifestData = make_manifest("", "special-bundle");
        match m.validate_file().unwrap_err() {
            crate::error::LoaderError::ManifestMissingFile { bundle } => {
                assert_eq!(bundle, "special-bundle", "error must carry the correct bundle name");
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    // ── RawManifestDependency::resolve edge cases ─────────────────────────

    #[test]
    fn raw_dep_resolve_by_contract() {
        let dep: RawManifestDependency = RawManifestDependency {
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
    fn raw_dep_resolve_by_contract_zero_contract_id() {
        // contract_id=0 is allowed — the resolve path does not validate the id.
        let dep: RawManifestDependency = RawManifestDependency {
            kind: "contract".to_owned(),
            contract: "audio".to_owned(),
            min_version: "0.1".to_owned(),
            bundle: None,
            contract_id: 0,
            bundle_id: None,
        };
        let resolved: Option<ManifestDependency> = dep.resolve();
        match resolved.expect("should resolve even with contract_id=0") {
            ManifestDependency::ByContract { contract_id, .. } => {
                assert_eq!(contract_id, 0);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn raw_dep_resolve_by_bundle() {
        let dep: RawManifestDependency = RawManifestDependency {
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
        let dep: RawManifestDependency = RawManifestDependency {
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

    #[test]
    fn raw_dep_resolve_by_bundle_zero_bundle_id_is_valid() {
        // bundle_id=Some(0) is a valid (if unusual) id — must NOT be treated as None.
        let dep: RawManifestDependency = RawManifestDependency {
            kind: "bundle".to_owned(),
            contract: "video".to_owned(),
            min_version: "2.0".to_owned(),
            bundle: Some("video-bundle".to_owned()),
            contract_id: 7,
            bundle_id: Some(0),
        };
        let resolved: Option<ManifestDependency> = dep.resolve();
        match resolved.expect("bundle_id=Some(0) must resolve") {
            ManifestDependency::ByBundle { bundle_id, .. } => {
                assert_eq!(bundle_id, 0);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    // ── resolved_dependencies helper ──────────────────────────────────────

    #[test]
    fn resolved_dependencies_skips_bundle_dep_with_no_bundle_id() {
        let mut m: ManifestData = make_manifest("p.so", "p");
        m.dependencies = vec![
            RawManifestDependency {
                kind: "bundle".to_owned(),
                contract: "x".to_owned(),
                min_version: "1.0".to_owned(),
                bundle: Some("x-bundle".to_owned()),
                contract_id: 1,
                bundle_id: None, // missing — must be skipped
            },
            RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "y".to_owned(),
                min_version: "1.0".to_owned(),
                bundle: None,
                contract_id: 2,
                bundle_id: None,
            },
        ];
        let deps: Vec<ManifestDependency> = m.resolved_dependencies();
        assert_eq!(deps.len(), 1, "bundle dep without bundle_id must be skipped");
        match &deps[0] {
            ManifestDependency::ByContract { contract, .. } => {
                assert_eq!(contract, "y");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }
}
