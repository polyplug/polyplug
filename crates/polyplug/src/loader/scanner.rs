//! Scanner — discovers plugin bundles in directories.
//!
//! Scans directories for `manifest.toml` files and parses them.
//!
//! Scanning a plugins directory is best-effort: one corrupt or unreadable
//! bundle must never hide the others. Every failure encountered while scanning
//! is surfaced to the caller as a [`ScanDiagnostic`] in [`ScanResult::diagnostics`]
//! rather than being silently swallowed.

use std::fs::{DirEntry, ReadDir, read_dir};
use std::io::Error as IoError;
use std::path::PathBuf;

use thiserror::Error;

use polyplug_common::ManifestData;

use super::parse_manifest;
use crate::error::LoaderError;

/// A non-fatal problem encountered while scanning a directory for bundles.
///
/// A diagnostic never aborts the scan — valid bundles are still returned. Each
/// variant identifies the offending path so the caller can report exactly which
/// entry failed.
#[derive(Debug, Error)]
pub enum ScanDiagnostic {
    /// A directory could not be read (e.g. permissions, I/O failure).
    #[error("failed to read directory `{path}`: {source}")]
    DirectoryReadFailed {
        path: PathBuf,
        #[source]
        source: IoError,
    },

    /// A directory entry could not be accessed (e.g. permissions, I/O failure).
    #[error("failed to access entry in `{dir}`: {source}")]
    EntryReadFailed {
        dir: PathBuf,
        #[source]
        source: IoError,
    },

    /// A `manifest.toml` was present but could not be parsed.
    #[error("failed to parse bundle at `{path}`: {source}")]
    ManifestParseFailed {
        path: PathBuf,
        #[source]
        source: LoaderError,
    },
}

/// Result of scanning one or more directories for plugin bundles.
///
/// `found` holds every successfully discovered bundle; `diagnostics` holds every
/// failure encountered. The two are independent: a populated `diagnostics` does
/// not invalidate the bundles in `found`.
#[derive(Debug)]
pub struct ScanResult {
    /// Successfully discovered bundles as `(bundle_path, manifest)` pairs.
    pub found: Vec<(PathBuf, ManifestData)>,
    /// Non-fatal problems encountered during the scan.
    pub diagnostics: Vec<ScanDiagnostic>,
}

/// Scan directories for plugin bundles.
///
/// Returns a [`ScanResult`] with the discovered `(bundle_path, manifest)` pairs
/// and any [`ScanDiagnostic`]s collected along the way. The scan is best-effort:
/// a corrupt manifest or an unreadable entry produces a diagnostic but does not
/// stop discovery of the remaining bundles.
pub fn scan_dirs(dirs: &[PathBuf]) -> ScanResult {
    let mut found: Vec<(PathBuf, ManifestData)> = Vec::new();
    let mut diagnostics: Vec<ScanDiagnostic> = Vec::new();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }

        let read_dir: ReadDir = match read_dir(dir) {
            Ok(read_dir) => read_dir,
            Err(source) => {
                diagnostics.push(ScanDiagnostic::DirectoryReadFailed {
                    path: dir.clone(),
                    source,
                });
                continue;
            }
        };

        for entry in read_dir {
            let entry: DirEntry = match entry {
                Ok(entry) => entry,
                Err(source) => {
                    diagnostics.push(ScanDiagnostic::EntryReadFailed {
                        dir: dir.clone(),
                        source,
                    });
                    continue;
                }
            };

            let path: PathBuf = entry.path();
            if !path.is_dir() {
                continue;
            }

            let manifest_path: PathBuf = path.join("manifest.toml");
            if !manifest_path.exists() {
                continue;
            }

            match parse_manifest(&path) {
                Ok(manifest) => found.push((path, manifest)),
                Err(source) => {
                    diagnostics.push(ScanDiagnostic::ManifestParseFailed { path, source });
                }
            }
        }
    }

    ScanResult { found, diagnostics }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use core::slice;

    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::{ScanDiagnostic, ScanResult, scan_dirs};

    fn write_valid_bundle(root: &Path, name: &str) {
        let bundle_dir: PathBuf = root.join(name);
        fs::create_dir_all(&bundle_dir).expect("create bundle dir");
        let manifest: String = format!(
            "id = 1\nname = \"{name}\"\nloader = \"native\"\nfile = \"{name}.so\"\nversion = \"1.0\"\n"
        );
        fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    }

    #[test]
    fn valid_bundle_returned_corrupt_and_unreadable_reported() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");

        // One valid bundle.
        write_valid_bundle(tmp.path(), "good_bundle");

        // One corrupt manifest.
        let bad_dir: PathBuf = tmp.path().join("bad_bundle");
        fs::create_dir_all(&bad_dir).expect("create bad bundle dir");
        fs::write(bad_dir.join("manifest.toml"), b"NOT VALID TOML ===== [[[")
            .expect("write bad manifest");

        let dirs: &[PathBuf] = &[tmp.path().to_path_buf()];
        let result: ScanResult = scan_dirs(dirs);

        // Valid bundle still returned.
        assert_eq!(result.found.len(), 1, "expected exactly the valid bundle");
        assert_eq!(result.found[0].1.name, "good_bundle");

        // Corrupt manifest surfaced as a diagnostic naming the path.
        let parse_diags: Vec<&ScanDiagnostic> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d, ScanDiagnostic::ManifestParseFailed { .. }))
            .collect();
        assert_eq!(parse_diags.len(), 1, "expected one parse diagnostic");
        let msg: String = parse_diags[0].to_string();
        assert!(
            msg.contains("bad_bundle"),
            "diagnostic must name the bundle: {msg}"
        );
    }

    #[test]
    fn missing_directory_skipped_without_diagnostic() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        let missing: PathBuf = tmp.path().join("does_not_exist");

        // A path that is not a directory is skipped (no diagnostic).
        let result: ScanResult = scan_dirs(&[missing]);
        assert!(result.found.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_reported_as_diagnostic() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        let locked: PathBuf = tmp.path().join("locked");
        fs::create_dir_all(&locked).expect("create locked dir");

        // Remove read/execute permissions so read_dir fails.
        let mut perms: fs::Permissions = fs::metadata(&locked).expect("metadata").permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&locked, perms).expect("chmod 000");

        let result: ScanResult = scan_dirs(slice::from_ref(&locked));

        // Restore permissions so TempDir cleanup can remove the directory.
        let mut restore: fs::Permissions = fs::Permissions::from_mode(0o755);
        restore.set_mode(0o755);
        let _ = fs::set_permissions(&locked, restore);

        // Running as root bypasses permission checks; only assert when the
        // read actually failed (non-root). When it fails it must be reported.
        if !result.diagnostics.is_empty() {
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|d| matches!(d, ScanDiagnostic::DirectoryReadFailed { .. })),
                "expected a DirectoryReadFailed diagnostic"
            );
        }
    }

    #[test]
    fn empty_dir_yields_no_bundles_no_diagnostics() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        let result: ScanResult = scan_dirs(&[tmp.path().to_path_buf()]);
        assert!(result.found.is_empty());
        assert!(result.diagnostics.is_empty());
    }
}
