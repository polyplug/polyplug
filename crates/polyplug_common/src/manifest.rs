//! Manifest — `manifest.toml` schema types and their pure behaviour.
//!
//! A plugin bundle carries a companion `manifest.toml`. This module owns the
//! parsed shape ([`ManifestData`]), its dependency declarations, and the pure
//! operations on them: TOML deserialization ([`ManifestData::parse_from_str`]),
//! self-validation ([`ManifestData::validate`]), and dependency resolution
//! ([`ManifestData::resolved_dependencies`]).
//!
//! No I/O, no logging, no runtime orchestration lives here — the `polyplug`
//! runtime wraps these with a filesystem reader (`parse_manifest`) and a
//! logger-aware resolver.

use core::fmt::{Formatter, Result as FmtResult};
use core::str::FromStr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserializer;
use serde::de::{self, MapAccess, Visitor};
use thiserror::Error;

use polyplug_abi::types::Version;
use polyplug_utils::{BundleId, GuestContractId, bundle_id as compute_bundle_id};

/// Errors from parsing or validating a bundle `manifest.toml`.
///
/// The runtime maps these 1:1 into its own `LoaderError` (via a `From` impl) so
/// existing runtime call sites and tests keep their flat error shape.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest parse error for `{path}`: {reason}")]
    Parse { path: String, reason: String },

    #[error("bundle \"{bundle}\" manifest.toml has an empty or missing `file` field")]
    MissingFile { bundle: String },

    #[error(
        "bundle \"{bundle}\" tampered with bundle_id: expected={expected:#x}, found={found:#x}"
    )]
    Tampered {
        bundle: String,
        expected: u64,
        found: u64,
    },
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
    struct FileFieldVisitor;

    impl<'de> Visitor<'de> for FileFieldVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut Formatter) -> FmtResult {
            formatter.write_str("a string or a table with platform keys")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
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

            Err(de::Error::custom(format!(
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
    /// Resolve this raw dependency into a typed [`ManifestDependency`].
    ///
    /// Returns `None` for a ByBundle dependency (`bundle` set) that has no
    /// `bundle_id` — such a dependency cannot be resolved. This is a pure
    /// operation: the runtime resolver logs a diagnostic on that `None`, but the
    /// resolution rule itself is intrinsic to the type.
    pub fn resolve(&self) -> Option<ManifestDependency> {
        match &self.bundle {
            None => Some(ManifestDependency::ByContract {
                contract: self.contract.clone(),
                contract_id: self.contract_id,
                min_version: self.min_version.clone(),
            }),
            Some(bundle) => self
                .bundle_id
                .map(|bid: BundleId| ManifestDependency::ByBundle {
                    bundle: bundle.clone(),
                    bundle_id: bid,
                    contract: self.contract.clone(),
                    contract_id: self.contract_id,
                    min_version: self.min_version.clone(),
                }),
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
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ManifestData {
    /// The external loader required to acquire a disk bundle (e.g. `"native"`,
    /// `"lua"`, `"js-quickjs"`). Internal registration leaves this empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub loader: String,
    /// Bundle name — human-readable identifier for this bundle.
    #[serde(default)]
    pub name: String,
    /// Raw dependency declarations from [[dependency]] table in manifest.toml
    #[serde(default, rename = "dependency")]
    pub dependencies: Vec<RawManifestDependency>,
    /// Bundle ID — computed from name via `polyplug_utils::bundle_id`, or provided in TOML.
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
    /// Resolve all raw dependencies into typed [`ManifestDependency`] values.
    ///
    /// ByBundle dependencies with a missing `bundle_id` are silently skipped —
    /// this is the pure form. The runtime's logger-aware resolver additionally
    /// emits a Warn-level diagnostic for each skipped dependency.
    pub fn resolved_dependencies(&self) -> Vec<ManifestDependency> {
        self.dependencies
            .iter()
            .filter_map(|dep: &RawManifestDependency| dep.resolve())
            .collect::<Vec<ManifestDependency>>()
    }

    /// Validate canonical bundle identity, provider metadata, and dependencies.
    ///
    /// This validation applies to every prepared-bundle transaction. External
    /// acquisition validates its loader and artifact separately.
    pub fn validate_metadata(&self) -> Result<(), ManifestError> {
        if self.name.is_empty() {
            return Err(ManifestError::Parse {
                path: self.path.display().to_string(),
                reason: "name field is required but was empty".to_owned(),
            });
        }
        if self.id == 0 {
            return Err(ManifestError::Parse {
                path: self.path.display().to_string(),
                reason: "id field is required but was 0 or missing".to_owned(),
            });
        }

        let expected_id: u64 = compute_bundle_id(&self.name);
        if self.id != expected_id {
            return Err(ManifestError::Tampered {
                bundle: self.name.clone(),
                expected: expected_id,
                found: self.id,
            });
        }

        for spec in &self.provides {
            validate_name_version_spec(spec, "provides", &self.path)?;
        }
        for spec in &self.bundle_dependencies {
            validate_name_version_spec(spec, "bundle_dependencies", &self.path)?;
        }

        for dep in &self.dependencies {
            if dep.contract_id.id() == 0 {
                continue;
            }
            let bare_contract: &str = match dep.contract.split_once('@') {
                Some((name, _)) => name,
                None => dep.contract.as_str(),
            };
            let dep_major: u32 = Version::from_str(&dep.min_version)
                .map(|v: Version| v.major)
                .unwrap_or(0);
            let expected: GuestContractId = GuestContractId::new(bare_contract, dep_major);
            if dep.contract_id != expected {
                return Err(ManifestError::Parse {
                    path: self.path.display().to_string(),
                    reason: format!(
                        "dependency \"{}\" declares contract_id {} but the canonical id for \
                         contract \"{}\" at major {} is {}",
                        dep.contract,
                        dep.contract_id.id(),
                        bare_contract,
                        dep_major,
                        expected.id()
                    ),
                });
            }
        }
        Ok(())
    }

    /// Validate all fields required to acquire an external bundle artifact.
    pub fn validate_acquisition(&self) -> Result<(), ManifestError> {
        if self.loader.is_empty() {
            return Err(ManifestError::Parse {
                path: self.path.display().to_string(),
                reason: "loader field is required but was empty".to_owned(),
            });
        }
        self.validate_file()
    }

    /// Validate both canonical metadata and external acquisition fields.
    pub fn validate(&self) -> Result<(), ManifestError> {
        self.validate_metadata()?;
        self.validate_acquisition()
    }

    /// Validate that the file field is present and non-empty.
    pub fn validate_file(&self) -> Result<(), ManifestError> {
        if self.file.trim().is_empty() {
            return Err(ManifestError::MissingFile {
                bundle: self.name.clone(),
            });
        }
        Ok(())
    }

    /// Parse a manifest from a TOML string.
    ///
    /// The returned manifest's `path` is empty — the runtime's filesystem reader
    /// sets it to the bundle directory after reading.
    pub fn parse_from_str(s: &str) -> Result<Self, ManifestError> {
        let mut manifest: ManifestData =
            toml::from_str(s).map_err(|e: toml::de::Error| ManifestError::Parse {
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
fn validate_name_version_spec(spec: &str, field: &str, path: &Path) -> Result<(), ManifestError> {
    let (name, version): (&str, Option<&str>) = match spec.split_once('@') {
        Some((name, version_str)) => (name, Some(version_str)),
        None => (spec, None),
    };
    if name.trim().is_empty() {
        return Err(ManifestError::Parse {
            path: path.display().to_string(),
            reason: format!("{field} entry \"{spec}\" has an empty contract/bundle name"),
        });
    }
    if let Some(version_str) = version {
        // Canonical form is `name@major` (bare major, as hashed into contract ids);
        // a full `major.minor.patch` version string is also accepted.
        let is_bare_major: bool = version_str.parse::<u32>().is_ok();
        if !is_bare_major && Version::from_str(version_str).is_err() {
            return Err(ManifestError::Parse {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::collections::HashMap;
    use std::path::PathBuf;

    use polyplug_utils::{BundleId, GuestContractId, bundle_id as compute_bundle_id};

    use super::{ManifestData, ManifestDependency, ManifestError, RawManifestDependency};

    fn make_manifest(file: &str, name: &str) -> ManifestData {
        ManifestData {
            loader: "native".to_owned(),
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
        match m.validate_file() {
            Err(ManifestError::MissingFile { bundle }) => {
                assert_eq!(bundle, "myplugin");
            }
            Err(other) => panic!("unexpected error variant: {:?}", other),
            Ok(()) => panic!("expected MissingFile error, got Ok"),
        }
    }

    #[test]
    fn validate_file_err_when_file_is_whitespace_only() {
        let m: ManifestData = make_manifest("   \t\n  ", "myplugin");
        match m.validate_file() {
            Err(ManifestError::MissingFile { bundle }) => {
                assert_eq!(bundle, "myplugin");
            }
            Err(other) => panic!("unexpected error variant: {:?}", other),
            Ok(()) => panic!("expected MissingFile error, got Ok"),
        }
    }

    // ── validate id == FNV1a-64(name) enforcement ─────────────────────────

    #[test]
    fn validate_ok_when_id_matches_bundle_id() {
        let mut m: ManifestData = make_manifest("plugin.so", "my_plugin");
        m.id = compute_bundle_id("my_plugin");
        assert!(
            m.validate().is_ok(),
            "validate must accept id == bundle_id(name)"
        );
    }

    #[test]
    fn validate_err_when_id_does_not_match_bundle_id() {
        let mut m: ManifestData = make_manifest("plugin.so", "my_plugin");
        m.id = compute_bundle_id("my_plugin").wrapping_add(1);
        match m.validate() {
            Err(ManifestError::Tampered {
                bundle,
                expected,
                found,
            }) => {
                assert_eq!(bundle, "my_plugin");
                assert_eq!(expected, compute_bundle_id("my_plugin"));
                assert_eq!(found, m.id);
            }
            Err(other) => panic!("unexpected error variant: {:?}", other),
            Ok(()) => panic!("expected Tampered error, got Ok"),
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
        match dep.resolve().expect("should resolve") {
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
        match dep.resolve().expect("should resolve") {
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
        assert!(
            dep.resolve().is_none(),
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
        // Test that a flat `file` string deserializes correctly.
        let toml = r#"
name = "test"
bundle_name = "test"
loader = "native"
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
loader = "native"
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
        } else if cfg!(target_os = "macos")
            && (cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64"))
        {
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
loader = "native"
[file]
freebsd.riscv64 = "libtest.so"
"#;
        let err: ManifestError = ManifestData::parse_from_str(toml)
            .expect_err("platform table missing the current platform must fail to parse");
        let message: String = err.to_string();
        assert!(
            message.contains("no file entry for platform"),
            "unexpected error message: {message}"
        );
    }
}
