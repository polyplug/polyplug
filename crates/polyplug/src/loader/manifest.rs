//! Manifest — manifest.toml parsing for plugin bundles.
//!
//! Reads the companion `manifest.toml` for a plugin bundle before loading.
//! The `runtime` field determines which `BundleLoader` handles the bundle.
//! REQUIRED — must be explicitly set in the manifest, no default.

// TODO: Move toml parse to host rust SDK

use core::str::FromStr;
use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserializer;
use serde::de::MapAccess;
use serde::de::Visitor;

use polyplug_abi::types::Version;
use polyplug_utils::{BundleId, GuestContractId};

use crate::runtime_store::BundleDependency;

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
                // Only nested arch→path tables (e.g. `linux.x86_64 = "..."`) are platform
                // entries. TOML scopes any keys written after `[file]` under this table, so
                // a manifest may carry non-platform siblings here (arrays/scalars such as a
                // misplaced `provides`/`function_count`). Those legitimately fail to
                // deserialize as a nested table and are skipped — a deserialize failure here
                // means "not a platform entry", not "malformed platform table". A genuinely
                // malformed platform table surfaces later as a "no file entry for platform"
                // error when the active platform cannot be resolved.
                let value: Result<HashMap<String, String>, M::Error> = map.next_value();
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
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RawManifestDependency {
    pub kind: String,
    pub contract: String,
    pub min_version: String,
    #[serde(default)]
    pub bundle: Option<String>,
    #[serde(default)]
    pub contract_id: GuestContractId,
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
        contract_id: GuestContractId,
        min_version: String,
    },
    ByBundle {
        bundle: String,
        bundle_id: BundleId,
        contract: String,
        contract_id: GuestContractId,
        min_version: String,
    },
}

/// Data parsed from a bundle's `manifest.toml`.
///
/// Only `runtime` is read in this epic. Additional fields are added in Epic 12.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ManifestData {
    /// The runtime required to load this bundle.
    /// Matched against `BundleLoader::runtime_name()` during dispatch.
    /// REQUIRED — must be explicitly set in the manifest, no default.
    #[serde(skip_serializing_if = "String::is_empty")]
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
    /// Bundle-level dependencies as string specs: ["image-decoder@1.0", "audio-encoder"]
    #[serde(default)]
    pub bundle_dependencies: Vec<String>,
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

    /// Parse bundle_dependencies strings into BundleDependency structs.
    ///
    /// Format: "name" or "name@1.0" where @version specifies minimum version.
    pub fn parsed_bundle_dependencies(&self) -> Vec<BundleDependency> {
        self.bundle_dependencies
            .iter()
            .map(|spec: &String| match spec.split_once('@') {
                Some((name, version_str)) => BundleDependency {
                    name: name.to_string(),
                    min_version: version_str.parse::<Version>().ok(),
                },
                None => BundleDependency {
                    name: spec.clone(),
                    min_version: None,
                },
            })
            .collect::<Vec<BundleDependency>>()
    }

    /// Validate the manifest has all required fields and well-formed values.
    ///
    /// Checks performed:
    /// - `runtime`, `name` are non-empty; `file` is present.
    /// - `id` is non-zero (folded in from the former inline load-path check).
    /// - `id` equals `polyplug_utils::bundle_id(name)` (TRUST_MODEL §2 identity).
    /// - every `provides` / `bundle_dependencies` entry matches `name[@version]`
    ///   grammar, with a parseable version when one is given.
    pub fn validate(&self) -> Result<(), crate::error::LoaderError> {
        if self.runtime.is_empty() {
            return Err(crate::error::LoaderError::ManifestParse {
                path: self.path.display().to_string(),
                reason: "runtime field is required but was empty".to_owned(),
            });
        }
        if self.name.is_empty() {
            return Err(crate::error::LoaderError::ManifestParse {
                path: self.path.display().to_string(),
                reason: "name field is required but was empty".to_owned(),
            });
        }
        if self.file.is_empty() {
            return Err(crate::error::LoaderError::ManifestMissingFile {
                bundle: self.name.clone(),
            });
        }
        if self.id == 0 {
            return Err(crate::error::LoaderError::ManifestParse {
                path: self.path.display().to_string(),
                reason: "id field is required but was 0 or missing".to_owned(),
            });
        }

        let expected_id: u64 = polyplug_utils::bundle_id(&self.name);
        if self.id != expected_id {
            return Err(crate::error::LoaderError::BundleTampered {
                bundle: self.name.clone(),
                expected: expected_id,
                found: self.id,
            });
        }

        // Validate provides / bundle_dependencies version specs.
        for spec in &self.provides {
            validate_name_version_spec(spec, "provides", &self.path)?;
        }
        for spec in &self.bundle_dependencies {
            validate_name_version_spec(spec, "bundle_dependencies", &self.path)?;
        }
        Ok(())
    }

    /// Validate that the file field is present and non-empty.
    pub fn validate_file(&self) -> Result<(), crate::error::LoaderError> {
        if self.file.trim().is_empty() {
            return Err(crate::error::LoaderError::ManifestMissingFile {
                bundle: self.name.clone(),
            });
        }
        Ok(())
    }

    /// Parse a manifest from a TOML string.
    pub fn parse_from_str(s: &str) -> Result<Self, crate::error::LoaderError> {
        let mut manifest: ManifestData =
            toml::from_str(s).map_err(|e| crate::error::LoaderError::ManifestParse {
                path: String::new(),
                reason: e.to_string(),
            })?;
        manifest.path = PathBuf::new();
        Ok(manifest)
    }
}

/// Validate a `name[@version]` spec used in `provides` / `bundle_dependencies`.
///
/// The name part must be non-empty. When an `@version` suffix is present it must
/// parse as a [`Version`]; an empty or unparseable version is rejected so silent
/// coercion to "no version" can no longer mask a malformed manifest.
fn validate_name_version_spec(
    spec: &str,
    field: &str,
    path: &std::path::Path,
) -> Result<(), crate::error::LoaderError> {
    let (name, version): (&str, Option<&str>) = match spec.split_once('@') {
        Some((name, version_str)) => (name, Some(version_str)),
        None => (spec, None),
    };
    if name.trim().is_empty() {
        return Err(crate::error::LoaderError::ManifestParse {
            path: path.display().to_string(),
            reason: format!("{field} entry \"{spec}\" has an empty contract/bundle name"),
        });
    }
    if let Some(version_str) = version {
        // Canonical form is `name@major` (bare major, as hashed into contract ids);
        // a full `major.minor.patch` version string is also accepted.
        let is_bare_major: bool = version_str.parse::<u32>().is_ok();
        if !is_bare_major && Version::from_str(version_str).is_err() {
            return Err(crate::error::LoaderError::ManifestParse {
                path: path.display().to_string(),
                reason: format!(
                    "{field} entry \"{spec}\" has an unparseable version spec \"{version_str}\" \
                     (expected a bare major like \"name@1\" or a full version)"
                ),
            });
        }
    }
    Ok(())
}

/// Parse a manifest.toml file from a bundle directory.
pub fn parse_manifest(
    bundle_dir: &std::path::Path,
) -> Result<ManifestData, crate::error::LoaderError> {
    let manifest_path: std::path::PathBuf = bundle_dir.join("manifest.toml");
    let content: String = std::fs::read_to_string(&manifest_path).map_err(|e| {
        crate::error::LoaderError::ManifestParse {
            path: manifest_path.display().to_string(),
            reason: format!("failed to read: {e}"),
        }
    })?;
    let mut manifest: ManifestData =
        toml::from_str(&content).map_err(|e| crate::error::LoaderError::ManifestParse {
            path: manifest_path.display().to_string(),
            reason: e.to_string(),
        })?;
    manifest.path = bundle_dir.to_path_buf();
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use polyplug_utils::{BundleId, GuestContractId};

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
            bundle_dependencies: Vec::new(),
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

    // ── validate id == FNV1a-64(name) enforcement ─────────────────────────

    #[test]
    fn validate_ok_when_id_matches_bundle_id() {
        let mut m: ManifestData = make_manifest("plugin.so", "my_plugin");
        m.id = polyplug_utils::bundle_id("my_plugin");
        assert!(
            m.validate().is_ok(),
            "validate must accept id == bundle_id(name)"
        );
    }

    #[test]
    fn validate_err_when_id_does_not_match_bundle_id() {
        let mut m: ManifestData = make_manifest("plugin.so", "my_plugin");
        m.id = polyplug_utils::bundle_id("my_plugin").wrapping_add(1);
        match m.validate() {
            Err(crate::error::LoaderError::BundleTampered {
                bundle,
                expected,
                found,
            }) => {
                assert_eq!(bundle, "my_plugin");
                assert_eq!(expected, polyplug_utils::bundle_id("my_plugin"));
                assert_eq!(found, m.id);
            }
            Err(other) => panic!("unexpected error variant: {:?}", other),
            Ok(()) => panic!("expected BundleTampered error, got Ok"),
        }
    }

    // ── RawManifestDependency::resolve edge cases ─────────────────────────

    #[test]
    fn raw_dep_resolve_by_contract() {
        let b_contract_id: GuestContractId = GuestContractId::new("test", 1);
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
        let b_contract_id: GuestContractId = GuestContractId::new("test", 1);
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
        let b_contract_id: GuestContractId = GuestContractId::new("test", 1);

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
        let b_contract_id_1: GuestContractId = GuestContractId::new("test1", 1);
        let b_contract_id_2: GuestContractId = GuestContractId::new("test2", 1);

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
        // Every platform CI/dev machines run on is declared so the deserializer can
        // resolve the active platform on Linux, macOS, and Windows alike. Windows cdylib
        // naming has no `lib` prefix (test.dll); macOS uses libtest.dylib.
        // The `provides`/`function_count` lines deliberately follow the `[file]` header to
        // exercise the deserializer's documented tolerance of non-platform siblings.
        let toml: &str = r#"
name = "test"
bundle_name = "test"
runtime = "native"
[file]
linux.x86_64 = "libtest.so"
macos.x86_64 = "libtest.dylib"
macos.aarch64 = "libtest.dylib"
windows.x86_64 = "test.dll"
provides = ["data.Test@1.0"]
function_count = { "data.Test@1" = 1 }
"#;
        let m: ManifestData =
            ManifestData::parse_from_str(toml).expect("platform file table should parse");
        if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            assert_eq!(m.file, "libtest.so");
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
            assert_eq!(m.file, "libtest.dylib");
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            assert_eq!(m.file, "libtest.dylib");
        } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            assert_eq!(m.file, "test.dll");
        }
    }

    #[test]
    fn manifest_file_field_platform_missing() {
        // A `[file]` table that declares only a platform the current machine is not running
        // must produce a parse error on every OS. This locks down the deserializer error
        // path (`no file entry for platform ...`) regardless of the active target triple.
        let toml: &str = r#"
name = "test"
bundle_name = "test"
runtime = "native"
[file]
freebsd.riscv64 = "libtest.so"
"#;
        let err: crate::error::LoaderError = ManifestData::parse_from_str(toml)
            .expect_err("platform table missing the current platform must fail to parse");
        let message: String = err.to_string();
        assert!(
            message.contains("no file entry for platform"),
            "unexpected error message: {message}"
        );
    }
}
