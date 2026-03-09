//! Scanner — filesystem discovery of plugin bundles.
//!
//! Scans one or more directories for plugin bundles by looking for:
//!   - Shared library files (`.so`, `.dll`, `.dylib`) with a companion `manifest.toml`
//!   - Subdirectories containing a `manifest.toml` file
//!
//! Results are returned as `(PathBuf, ManifestData)` pairs, sorted by `bundle_name`.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use crate::loader::manifest::ManifestData;
use crate::loader::parse_manifest;

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

        if metadata.is_file() {
            let ext: Option<String> = entry_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e: &str| e.to_ascii_lowercase());

            match ext.as_deref() {
                Some("so") | Some("dll") | Some("dylib") => {}
                _ => continue,
            }

            let manifest_path: PathBuf = entry_path.with_extension("manifest.toml");

            if !manifest_path.exists() {
                eprintln!(
                    "[polyplug] skipping {}: no companion manifest.toml",
                    entry_path.display()
                );
                continue;
            }

            let mut manifest: ManifestData = match parse_manifest(&entry_path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "[polyplug] scan_dir: failed to parse manifest for {}: {e}",
                        entry_path.display()
                    );
                    continue;
                }
            };

            manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name);
            results.push((entry_path, manifest));
        } else if metadata.is_dir() {
            let manifest_path: PathBuf = entry_path.join("manifest.toml");

            if !manifest_path.exists() {
                continue;
            }

            let toml_str: String = match std::fs::read_to_string(&manifest_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "[polyplug] scan_dir: failed to read {}: {e}",
                        manifest_path.display()
                    );
                    continue;
                }
            };

            let mut manifest: ManifestData = match toml::from_str(&toml_str) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "[polyplug] scan_dir: failed to parse {}: {e}",
                        manifest_path.display()
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
    use std::io::Write;

    #[test]
    fn scan_dir_empty_returns_empty() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert!(result.is_empty(), "empty dir must return empty vec");
    }

    #[test]
    fn scan_dir_skips_bundle_without_manifest() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        // Create a .so file with no companion manifest
        std::fs::File::create(tmp.path().join("plugin.so")).expect("create .so");
        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert!(result.is_empty(), "bundle without manifest must be skipped");
    }

    #[test]
    fn scan_dir_finds_bundle_with_manifest() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");

        // Create a fake .so file
        std::fs::File::create(tmp.path().join("myplugin.so")).expect("create .so");

        // Create companion manifest.toml
        let manifest_content: &str = r#"
bundle_name = "myplugin"
runtime = "native"
"#;
        let mut f: std::fs::File = std::fs::File::create(tmp.path().join("myplugin.manifest.toml"))
            .expect("create manifest");
        f.write_all(manifest_content.as_bytes())
            .expect("write manifest");

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

        let manifest_content: &str = r#"
bundle_name = "mybundle"
runtime = "native"
"#;
        let mut f: std::fs::File =
            std::fs::File::create(bundle_dir.join("manifest.toml")).expect("create manifest");
        f.write_all(manifest_content.as_bytes())
            .expect("write manifest");

        let result: Vec<(PathBuf, ManifestData)> = scan_dir(tmp.path());
        assert_eq!(result.len(), 1, "expected exactly one bundle");
        assert_eq!(result[0].1.bundle_name, "mybundle");
    }

    #[test]
    fn scan_dirs_deduplicates_by_path() {
        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");

        // Create a fake .so file
        std::fs::File::create(tmp.path().join("plugin.so")).expect("create .so");

        // Create companion manifest.toml
        let manifest_content: &str = r#"
bundle_name = "plugin"
runtime = "native"
"#;
        let mut f: std::fs::File = std::fs::File::create(tmp.path().join("plugin.manifest.toml"))
            .expect("create manifest");
        f.write_all(manifest_content.as_bytes())
            .expect("write manifest");

        // Scan the same directory twice
        let dirs: Vec<PathBuf> = vec![tmp.path().to_path_buf(), tmp.path().to_path_buf()];
        let result: Vec<(PathBuf, ManifestData)> = scan_dirs(&dirs);

        assert_eq!(result.len(), 1, "same path scanned twice must dedup to one");
    }
}
