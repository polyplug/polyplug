//! Manifest — manifest.toml parsing for plugin bundles.
//!
//! Reads the companion `manifest.toml` for a plugin bundle before loading.
//! The `runtime` field determines which `BundleLoader` handles the bundle.
//! If absent, defaults to `"native"`.

// TODO: Move toml parse to host rust SDK

use std::collections::HashMap;
use std::path::PathBuf;

use polyplug_utils::{BundleId, PluginContractId};
use serde::Deserializer;

fn default_runtime() -> String {
    "native".to_owned()
}

const fn current_os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    }
}

const fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    }
}

fn deserialize_file_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::MapAccess;
    use serde::de::Visitor;

    struct FileFieldVisitor;

    impl<'de> Visitor<'de> for FileFieldVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
            formatter.write_str("a string or a table with platform keys")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v)
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let target_os: &str = current_os();
            let target_arch: &str = current_arch();

            // Try to parse as nested map: {"linux": {"x86_64": "file.so"}}
            let mut os_arch_map: HashMap<String, HashMap<String, String>> = HashMap::new();

            while let Some(key) = map.next_key::<String>()? {
                // Try to get the value as a nested table first
                let value: Result<HashMap<String, String>, _> = map.next_value();
                if let Ok(nested) = value {
                    os_arch_map.insert(key, nested);
                }
            }

            // Try nested map
            if let Some(arch_map) = os_arch_map.get(target_os) {
                if let Some(path) = arch_map.get(target_arch) {
                    return Ok(path.clone());
                }
            }

            // Not found
            let available: Vec<String> = os_arch_map
                .iter()
                .flat_map(|(os, arch_map)| {
                    arch_map.keys().map(move |arch| format!("{}.{}", os, arch))
                })
                .collect();

            Err(serde::de::Error::custom(format!(
                "no file entry for platform {}.{}, available: {:?}",
                target_os, target_arch, available
            )))
        }
    }

    deserializer.deserialize_any(FileFieldVisitor)
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
    pub contract_id: PluginContractId,
    #[serde(default)]
    pub bundle_id: Option<BundleId>,
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
        contract_id: PluginContractId,
        min_version: String,
    },
    ByBundle {
        bundle: String,
        bundle_id: BundleId,
        contract: String,
        contract_id: PluginContractId,
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
    /// Bundle name — human-readable identifier for this bundle.
    #[serde(default)]
    pub name: String,
    /// Raw dependency declarations from [[dependency]] table in manifest.toml
    #[serde(default, rename = "dependency")]
    pub dependencies: Vec<RawManifestDependency>,
    /// Bundle ID — computed from name via abi::bundle_id(), or provided in TOML.
    #[serde(default)]
    pub id: u64,
    /// Version string for this bundle
    #[serde(default)]
    pub version: String,
    /// Path to the plugin file (relative to bundle root).
    /// For native bundles this is resolved from the platform table at parse time.
    #[serde(default, deserialize_with = "deserialize_file_field")]
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
    #![allow(clippy::expect_used)]
    use polyplug_utils::{BundleId, PluginContractId};

    use super::{ManifestData, ManifestDependency, RawManifestDependency};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_manifest(file: &str, name: &str) -> ManifestData {
        ManifestData {
            runtime: "native".to_owned(),
            name: name.to_owned(),
            dependencies: Vec::new(),
            id: 0,
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
        assert!(
            m.validate_file().is_ok(),
            "non-empty file must pass validation"
        );
    }

    #[test]
    fn validate_file_err_when_file_is_empty_string() {
        let m: ManifestData = make_manifest("", "myplugin");
        let result: Result<(), crate::error::LoaderError> = m.validate_file();
        match result {
            Err(crate::error::LoaderError::ManifestMissingFile { bundle }) => {
                assert_eq!(bundle, "myplugin");
            }
            Err(other) => panic!("unexpected error variant: {:?}", other),
            Ok(()) => panic!("expected ManifestMissingFile error, got Ok"),
        }
    }

    #[test]
    fn validate_file_err_when_file_is_whitespace_only() {
        let m: ManifestData = make_manifest("   \t\n  ", "myplugin");
        let result: Result<(), crate::error::LoaderError> = m.validate_file();
        match result {
            Err(crate::error::LoaderError::ManifestMissingFile { bundle }) => {
                assert_eq!(bundle, "myplugin");
            }
            Err(other) => panic!("unexpected error variant: {:?}", other),
            Ok(()) => panic!("expected ManifestMissingFile error, got Ok"),
        }
    }

    #[test]
    fn validate_file_err_carries_bundle_name() {
        let m: ManifestData = make_manifest("", "special-bundle");
        match m.validate_file() {
            Err(crate::error::LoaderError::ManifestMissingFile { bundle }) => {
                assert_eq!(
                    bundle, "special-bundle",
                    "error must carry the correct bundle name"
                );
            }
            Err(other) => panic!("unexpected error variant: {:?}", other),
            Ok(()) => panic!("expected ManifestMissingFile error, got Ok"),
        }
    }

    // ── RawManifestDependency::resolve edge cases ─────────────────────────

    #[test]
    fn raw_dep_resolve_by_contract() {
        let b_contract_id: PluginContractId = PluginContractId::new("test", 1);
        let dep = RawManifestDependency {
            kind: "contract".to_owned(),
            contract: "math".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: None,
            contract_id: b_contract_id,
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
                assert_eq!(contract_id, b_contract_id);
                assert_eq!(min_version, "1.0");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn raw_dep_resolve_by_bundle() {
        let b_contract_id: PluginContractId = PluginContractId::new("test", 1);
        let b_bundle_id: BundleId = BundleId::new("test");

        let dep: RawManifestDependency = RawManifestDependency {
            kind: "bundle".to_owned(),
            contract: "math".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: Some("math-bundle".to_owned()),
            contract_id: b_contract_id,
            bundle_id: Some(b_bundle_id),
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
                assert_eq!(bundle_id, b_bundle_id);
                assert_eq!(contract, "math");
                assert_eq!(contract_id, b_contract_id);
                assert_eq!(min_version, "1.0");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn raw_dep_resolve_by_bundle_missing_bundle_id_returns_none() {
        let b_contract_id: PluginContractId = PluginContractId::new("test", 1);

        let dep: RawManifestDependency = RawManifestDependency {
            kind: "bundle".to_owned(),
            contract: "math".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: Some("math-bundle".to_owned()),
            contract_id: b_contract_id,
            bundle_id: None,
        };
        let resolved: Option<ManifestDependency> = dep.resolve();
        assert!(
            resolved.is_none(),
            "expected None when bundle_id is missing"
        );
    }

    // ── resolved_dependencies helper ──────────────────────────────────────

    #[test]
    fn resolved_dependencies_skips_bundle_dep_with_no_bundle_id() {
        let b_contract_id_1: PluginContractId = PluginContractId::new("test1", 1);
        let b_contract_id_2: PluginContractId = PluginContractId::new("test2", 1);

        let mut m: ManifestData = make_manifest("p.so", "p");
        m.dependencies = vec![
            RawManifestDependency {
                kind: "bundle".to_owned(),
                contract: "x".to_owned(),
                min_version: "1.0".to_owned(),
                bundle: Some("x-bundle".to_owned()),
                contract_id: b_contract_id_1,
                bundle_id: None, // missing — must be skipped
            },
            RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "y".to_owned(),
                min_version: "1.0".to_owned(),
                bundle: None,
                contract_id: b_contract_id_2,
                bundle_id: None,
            },
        ];
        let deps: Vec<ManifestDependency> = m.resolved_dependencies();
        assert_eq!(
            deps.len(),
            1,
            "bundle dep without bundle_id must be skipped"
        );
        match &deps[0] {
            ManifestDependency::ByContract { contract, .. } => {
                assert_eq!(contract, "y");
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn manifest_file_field_nested_table() {
        // Test that [file] table with dotted keys deserializes correctly
        let toml = r#"
name = "test"
bundle_name = "test"
runtime = "native"
file = "fallback.so"
provides = ["data.Test@1.0"]
function_count = { "data.Test@1" = 1 }
"#;
        let m: ManifestData =
            ManifestData::parse_from_str(toml).expect("flat file field should parse");
        assert_eq!(m.file, "fallback.so");
    }

    #[test]
    fn manifest_file_field_platform_table() {
        // Test that [file] table with dotted keys deserializes correctly
        // Note: TOML linux.x86_64 = "..." creates nested structure {"linux": {"x86_64": "..."}}
        let toml = r#"
name = "test"
bundle_name = "test"
runtime = "native"
[file]
linux.x86_64 = "libtest.so"
macos.aarch64 = "libtest.dylib"
provides = ["data.Test@1.0"]
function_count = { "data.Test@1" = 1 }
"#;
        let m: ManifestData =
            ManifestData::parse_from_str(toml).expect("platform file table should parse");
        // On linux x86_64, should resolve to libtest.so
        if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            assert_eq!(m.file, "libtest.so");
        }
    }
}
