//! Scanner — filesystem discovery of plugin bundles.
//!
//! Scans one or more directories for plugin bundles by looking for:
//!   - Subdirectories containing a `manifest.toml` file
//!
//! Results are returned as `(PathBuf, ManifestData)` pairs, sorted by `bundle_name`.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use crate::loader::manifest::ManifestData;

/// Scan a single directory for plugin bundles.
///
/// Returns a sorted list of `(bundle_path, ManifestData)` pairs.
/// Bundles without a companion `manifest.toml` are silently skipped (with a warning).
/// I/O errors are logged with `eprintln!` and the offending entry is skipped.
///
/// The results are sorted lexicographically by `bundle_name`.
pub fn scan_dir(dir: &Path) -> Vec<(PathBuf, ManifestData)> {
    let read_dir_iter: std::fs::ReadDir = match std::fs::read_dir(dir) {
        Ok(iter) => iter,
        Err(e) => {
            eprintln!(
                "[polyplug] scan_dir: failed to read directory {}: {e}",
                dir.display()
            );
            return Vec::new();
        }
    };

    let mut results: Vec<(PathBuf, ManifestData)> = Vec::new();

    for entry_result in read_dir_iter {
        let entry: std::fs::DirEntry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[polyplug] scan_dir: skipping entry: {e}");
                continue;
            }
        };

        let entry_path: PathBuf = entry.path();

        let metadata: std::fs::Metadata = match std::fs::metadata(&entry_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "[polyplug] scan_dir: failed to stat {}: {e}",
                    entry_path.display()
                );
                continue;
            }
        };

        if metadata.is_dir() {
            let mut manifest: ManifestData = match crate::loader::parse_manifest(&entry_path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "[polyplug] scan_dir: skipping {}: {e}",
                        entry_path.display()
                    );
                    continue;
                }
            };
            manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name);
            results.push((entry_path, manifest));
        }
    }

    results.sort_by(|a: &(PathBuf, ManifestData), b: &(PathBuf, ManifestData)| {
        a.1.bundle_name.cmp(&b.1.bundle_name)
    });

    results
}

/// Scan multiple directories for plugin bundles.
///
/// Combines results from all directories. If the same bundle path is found in
/// multiple directories, only the first occurrence is kept (dedup by path).
///
/// Results are NOT globally sorted — order follows directory order, with
/// per-directory sorting preserved.
pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<(PathBuf, ManifestData)> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut all: Vec<(PathBuf, ManifestData)> = Vec::new();

    for dir in dirs {
        for entry in scan_dir(dir) {
            if seen.insert(entry.0.clone()) {
                all.push(entry);
            }
        }
    }

    all
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn scan_dir_empty_returns_empty() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert!(result.is_empty(), "empty dir must return empty vec");
    }

    #[test]
    fn scan_dir_skips_bundle_without_manifest() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        // Create a directory WITHOUT manifest.toml inside
        let bundle_dir: PathBuf = tmp.path().join("plugin_without_manifest");
        std::fs::create_dir(&bundle_dir).expect("create dir");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert!(
            result.is_empty(),
            "directory without manifest must be skipped"
        );
    }

    #[test]
    fn scan_dir_finds_bundle_with_manifest() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        // Create directory bundle: myplugin/manifest.toml + myplugin/myplugin.so
        let bundle_dir: PathBuf = tmp.path().join("myplugin");
        std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
        std::fs::write(bundle_dir.join("myplugin.so"), b"").expect("write stub so");
        let manifest_content: &str =
            "bundle_name = \"myplugin\"\nruntime = \"native\"\nfile = \"myplugin.so\"\n";
        std::fs::write(bundle_dir.join("manifest.toml"), manifest_content).expect("write manifest");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert_eq!(result.len(), 1, "expected exactly one bundle");
        assert_eq!(result[0].1.bundle_name, "myplugin");
    }

    #[test]
    fn scan_dir_finds_dir_bundle_with_manifest() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");

        // Create a subdirectory bundle
        let bundle_dir: PathBuf = tmp.path().join("mybundle");
        std::fs::create_dir(&bundle_dir).expect("create bundle dir");
        std::fs::write(bundle_dir.join("mybundle.so"), b"").expect("write stub so");

        let manifest_content: &str =
            "bundle_name = \"mybundle\"\nruntime = \"native\"\nfile = \"mybundle.so\"\n";
        std::fs::write(bundle_dir.join("manifest.toml"), manifest_content).expect("write manifest");

        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert_eq!(result.len(), 1, "expected exactly one bundle");
        assert_eq!(result[0].1.bundle_name, "mybundle");
    }

    #[test]
    fn scan_dirs_deduplicates_by_path() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        // Create a directory bundle
        let bundle_dir: PathBuf = tmp.path().join("plugin");
        std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
        std::fs::write(bundle_dir.join("plugin.so"), b"").expect("write stub so");
        let manifest_content: &str =
            "bundle_name = \"plugin\"\nruntime = \"native\"\nfile = \"plugin.so\"\n";
        std::fs::write(bundle_dir.join("manifest.toml"), manifest_content).expect("write manifest");
        // Scan the same directory twice
        let dirs: Vec<PathBuf> = vec![tmp.path().to_path_buf(), tmp.path().to_path_buf()];
        let result: Vec<(PathBuf, ManifestData)> = scan_dirs(&dirs);
        assert_eq!(result.len(), 1, "same path scanned twice must dedup to one");
    }

    #[test]
    fn scan_dir_ignores_flat_so_files() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        // Create a flat .so file (not in a bundle directory) — should be ignored
        std::fs::write(tmp.path().join("libplugin.so"), b"").expect("write flat so");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert!(result.is_empty(), "flat .so files must be ignored");
    }
}
