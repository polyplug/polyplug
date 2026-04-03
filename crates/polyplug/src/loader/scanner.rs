//! Scanner — discovers plugin bundles in directories.
//!
//! Scans directories for `manifest.toml` files and parses them.

use std::path::PathBuf;

use super::{parse_manifest, ManifestData};

/// Scan directories for plugin bundles.
///
/// Returns a list of (bundle_path, manifest) pairs.
pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<(PathBuf, ManifestData)> {
    let mut discovered: Vec<(PathBuf, ManifestData)> = Vec::new();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }

        let entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(dir) {
            Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
            Err(_) => continue,
        };

        for entry in entries {
            let path: PathBuf = entry.path();
            if path.is_dir() {
                let manifest_path: PathBuf = path.join("manifest.toml");
                if manifest_path.exists() {
                    if let Ok(manifest) = parse_manifest(&path) {
                        discovered.push((path, manifest));
                    }
                }
            }
        }
    }

    discovered
}
