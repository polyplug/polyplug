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
            let manifest: ManifestData = match crate::loader::parse_manifest(&entry_path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "[polyplug] scan_dir: skipping {}: {e}",
                        entry_path.display()
                    );
                    continue;
                }
            };
            if manifest.id == 0 {
                eprintln!(
                    "[polyplug] warning: skipping bundle with missing id: {}",
                    entry_path.display()
                );
                continue;
            }
            results.push((entry_path, manifest));
        }
    }

    results.sort_by(|a: &(PathBuf, ManifestData), b: &(PathBuf, ManifestData)| {
        a.1.name.cmp(&b.1.name)
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
    use crate::loader::manifest::ManifestData;

    fn write_test_bundle(dir: &Path, name: &str, id: u64) {
        let bundle_dir: PathBuf = dir.join(name);
        std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
        let so_name: String = format!("{name}.so");
        std::fs::write(bundle_dir.join(&so_name), b"").expect("write stub so");
        let manifest: ManifestData = ManifestData {
            id,
            name: name.to_owned(),
            runtime: "native".to_owned(),
            file: so_name,
            ..ManifestData::for_test("", "native", "")
        };
        std::fs::write(bundle_dir.join("manifest.toml"), manifest.to_toml())
            .expect("write manifest");
    }

    #[test]
    fn scan_dir_empty_returns_empty() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert!(result.is_empty(), "empty dir must return empty vec");
    }

    #[test]
    fn scan_dir_skips_bundle_without_manifest() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
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
        write_test_bundle(tmp.path(), "myplugin", 1);
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert_eq!(result.len(), 1, "expected exactly one bundle");
        assert_eq!(result[0].1.name, "myplugin");
    }

    #[test]
    fn scan_dir_finds_dir_bundle_with_manifest() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path(), "mybundle", 2);
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert_eq!(result.len(), 1, "expected exactly one bundle");
        assert_eq!(result[0].1.name, "mybundle");
    }

    #[test]
    fn scan_dirs_deduplicates_by_path() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path(), "plugin", 3);
        let dirs: Vec<PathBuf> = vec![tmp.path().to_path_buf(), tmp.path().to_path_buf()];
        let result: Vec<(PathBuf, ManifestData)> = scan_dirs(&dirs);
        assert_eq!(result.len(), 1, "same path scanned twice must dedup to one");
    }

    #[test]
    fn scan_dir_ignores_flat_so_files() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        std::fs::write(tmp.path().join("libplugin.so"), b"").expect("write flat so");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert!(result.is_empty(), "flat .so files must be ignored");
    }

    #[test]
    fn scan_dir_result_path_is_subdir_not_root() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path(), "alpha", 4);
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert_eq!(result.len(), 1);
        assert_ne!(
            result[0].0,
            tmp.path().to_path_buf(),
            "result path must be the bundle directory, not the scan root"
        );
        assert_eq!(result[0].0, tmp.path().join("alpha"));
    }

    #[test]
    fn scan_dir_results_sorted_by_bundle_name() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        for (idx, name) in ["zebra", "apple", "mango"].iter().enumerate() {
            write_test_bundle(tmp.path(), name, 5 + idx as u64);
        }
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert_eq!(result.len(), 3);
        let names: Vec<&str> = result.iter().map(|(_, m)| m.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["apple", "mango", "zebra"],
            "results must be sorted by bundle_name"
        );
    }

    #[test]
    #[cfg(unix)]
    fn scan_dir_returns_empty_on_permission_denied() {
        use std::os::unix::fs::PermissionsExt;
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        let restricted_dir: PathBuf = tmp.path().join("no_access");
        std::fs::create_dir_all(&restricted_dir).expect("create dir");
        std::fs::set_permissions(&restricted_dir, std::fs::Permissions::from_mode(0o000))
            .expect("set permissions");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(&restricted_dir);
        std::fs::set_permissions(&restricted_dir, std::fs::Permissions::from_mode(0o700))
            .expect("restore permissions");
        assert!(
            result.is_empty(),
            "unreadable directory must return empty vec, not panic"
        );
    }

    #[test]
    #[cfg(unix)]
    fn scan_dir_does_not_follow_symlinks() {
        let real_bundle_tmp: tempfile::TempDir = tempfile::TempDir::new().expect("real bundle tmp");
        write_test_bundle(real_bundle_tmp.path(), "real_plugin", 12345);

        let scan_tmp: tempfile::TempDir = tempfile::TempDir::new().expect("scan root tmp");
        let symlink_path: PathBuf = scan_tmp.path().join("sym_plugin");
        std::os::unix::fs::symlink(real_bundle_tmp.path().join("real_plugin"), &symlink_path)
            .expect("create symlink");

        let result: Vec<(PathBuf, ManifestData)> = scan_dir(scan_tmp.path());

        for (path, _manifest) in &result {
            assert!(
                path.starts_with(scan_tmp.path()),
                "scanner result path must be within the scan root, not the symlink target"
            );
        }
    }

    #[test]
    fn scan_dir_returns_empty_for_nonexistent_dir() {
        let nonexistent: PathBuf = PathBuf::from("/tmp/polyplug_test_does_not_exist_xyz987");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(&nonexistent);
        assert!(
            result.is_empty(),
            "non-existent directory must return empty vec, not panic"
        );
    }
}
