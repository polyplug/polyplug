//! Manifest — runtime-side `manifest.toml` loading and resolution.
//!
//! The manifest schema types ([`ManifestData`], [`RawManifestDependency`],
//! [`ManifestDependency`]) and their pure validation live in `polyplug_common`.
//! This module holds the runtime-only wrappers that pure crate cannot: reading
//! the file from disk, resolving dependencies with a live logger, and turning
//! bundle-dependency specs into runtime [`BundleDependency`] values.

use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use polyplug_abi::types::{LogLevel, Version};
use polyplug_common::{ManifestData, ManifestDependency, ManifestError, RawManifestDependency};

use crate::error::LoaderError;
use crate::logger::LoggerHandle;
use crate::runtime_store::BundleDependency;

/// Parse a `manifest.toml` file from a bundle directory.
///
/// Reads the file and delegates parsing to [`ManifestData::parse_from_str`],
/// then records the bundle directory as the manifest's `path` (used for
/// diagnostics and artifact resolution).
pub fn parse_manifest(bundle_dir: &Path) -> Result<ManifestData, LoaderError> {
    let manifest_path: PathBuf = bundle_dir.join("manifest.toml");
    let content: String =
        read_to_string(&manifest_path).map_err(|e| LoaderError::ManifestParse {
            path: manifest_path.display().to_string(),
            reason: format!("failed to read: {e}"),
        })?;
    let mut manifest: ManifestData =
        ManifestData::parse_from_str(&content).map_err(|e| LoaderError::ManifestParse {
            path: manifest_path.display().to_string(),
            reason: manifest_parse_reason(&e),
        })?;
    manifest.path = bundle_dir.to_path_buf();
    Ok(manifest)
}

/// Extract the human-readable reason from a common `ManifestError::Parse`,
/// falling back to the full Display for the (unreachable here) other variants.
fn manifest_parse_reason(err: &ManifestError) -> String {
    match err {
        ManifestError::Parse { reason, .. } => reason.clone(),
        other => other.to_string(),
    }
}

/// Resolve all raw dependencies into typed [`ManifestDependency`] values,
/// logging a Warn-level diagnostic for each ByBundle dependency skipped because
/// it has no `bundle_id`.
///
/// The resolution rule itself is pure ([`ManifestData::resolved_dependencies`]);
/// only the diagnostic is a runtime concern, so it lives here.
pub(crate) fn resolved_dependencies_with_logger(
    manifest: &ManifestData,
    logger: LoggerHandle,
) -> Vec<ManifestDependency> {
    manifest
        .dependencies
        .iter()
        .filter_map(|dep: &RawManifestDependency| {
            let resolved: Option<ManifestDependency> = dep.resolve();
            if resolved.is_none() && dep.bundle.is_some() {
                logger.log(LogLevel::Warn, "manifest", || {
                    format!("ByBundle dep '{}' has no bundle_id; skipping", dep.contract)
                });
            }
            resolved
        })
        .collect::<Vec<ManifestDependency>>()
}

/// Parse `bundle_dependencies` string specs into [`BundleDependency`] structs.
///
/// Format: `"name"` or `"name@1.0"` where `@version` specifies a minimum version.
pub(crate) fn parsed_bundle_dependencies(manifest: &ManifestData) -> Vec<BundleDependency> {
    manifest
        .bundle_dependencies
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::fs::write;

    use polyplug_common::{ManifestData, ManifestDependency};
    use polyplug_utils::{GuestContractId, bundle_id as compute_bundle_id};
    use tempfile::TempDir;

    use super::{parsed_bundle_dependencies, resolved_dependencies_with_logger};
    use crate::logger::LoggerHandle;

    #[test]
    fn parse_manifest_reads_and_sets_path() {
        let dir: TempDir = TempDir::new().expect("tempdir");
        let id: u64 = compute_bundle_id("disk_bundle");
        write(
            dir.path().join("manifest.toml"),
            format!(
                "loader = \"native\"\nname = \"disk_bundle\"\nid = {id}\nversion = \"1.0.0\"\nfile = \"disk_bundle.so\"\n"
            ),
        )
        .expect("write manifest");
        let manifest: ManifestData =
            super::parse_manifest(dir.path()).expect("parse_manifest should read the file");
        assert_eq!(manifest.name, "disk_bundle");
        assert_eq!(manifest.path, dir.path());
        manifest
            .validate()
            .expect("on-disk manifest should validate");
    }

    #[test]
    fn resolved_dependencies_with_logger_skips_bundle_dep_with_no_bundle_id() {
        let cid: GuestContractId = GuestContractId::new("y", 1);
        let mut m: ManifestData =
            ManifestData::parse_from_str("loader = \"native\"\nname = \"p\"\n")
                .expect("parse minimal manifest");
        m.dependencies = vec![
            polyplug_common::RawManifestDependency {
                kind: "bundle".to_owned(),
                contract: "x".to_owned(),
                min_version: "1.0".to_owned(),
                bundle: Some("x-bundle".to_owned()),
                contract_id: GuestContractId::new("x", 1),
                bundle_id: None, // missing — skipped, with a Warn diagnostic
            },
            polyplug_common::RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "y".to_owned(),
                min_version: "1.0".to_owned(),
                bundle: None,
                contract_id: cid,
                bundle_id: None,
            },
        ];
        let deps: Vec<ManifestDependency> =
            resolved_dependencies_with_logger(&m, LoggerHandle::default_stderr());
        assert_eq!(
            deps.len(),
            1,
            "bundle dep without bundle_id must be skipped"
        );
        match &deps[0] {
            ManifestDependency::ByContract { contract, .. } => assert_eq!(contract, "y"),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn parsed_bundle_dependencies_parses_name_and_version() {
        let mut m: ManifestData =
            ManifestData::parse_from_str("loader = \"native\"\nname = \"p\"\n")
                .expect("parse minimal manifest");
        m.bundle_dependencies = vec!["image-decoder@1.0".to_owned(), "audio-encoder".to_owned()];
        let deps = parsed_bundle_dependencies(&m);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "image-decoder");
        assert!(deps[0].min_version.is_some());
        assert_eq!(deps[1].name, "audio-encoder");
        assert!(deps[1].min_version.is_none());
    }
}
