//! Canonical digest computation for bundle signing.
//!
//! See `lib.rs` module-level documentation for the full algorithm specification.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::SigError;

/// The name of the detached signature file — excluded from the digest.
pub(crate) const SIG_FILE_NAME: &str = "bundle.sig";

/// Compute the canonical 32-byte SHA-256 digest over all bundle files except `bundle.sig`.
///
/// The digest is deterministic: it covers file names and contents in sorted order,
/// so any compliant signer in any language can reproduce the exact same bytes.
pub fn canonical_digest(bundle_dir: &Path) -> Result<[u8; 32], SigError> {
    if !bundle_dir.is_dir() {
        return Err(SigError::NotADirectory {
            path: bundle_dir.display().to_string(),
        });
    }

    let bundle_name: String = bundle_dir.display().to_string();

    // Collect all file paths relative to the bundle root, excluding bundle.sig.
    let mut entries: Vec<(String, std::path::PathBuf)> =
        collect_files(bundle_dir, bundle_dir, &bundle_name)?;

    // Sort lexicographically by relative path bytes for determinism.
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    // Build the canonical buffer:
    // For each file in sorted order: relative_path_utf8 + 0x00 + sha256(file_bytes)
    let mut canonical: Vec<u8> = Vec::new();
    for (rel_path, abs_path) in &entries {
        let file_bytes: Vec<u8> =
            std::fs::read(abs_path).map_err(|e: std::io::Error| SigError::Io {
                path: abs_path.display().to_string(),
                source: e,
            })?;

        let file_hash: [u8; 32] = Sha256::digest(&file_bytes).into();

        canonical.extend_from_slice(rel_path.as_bytes());
        canonical.push(0x00);
        canonical.extend_from_slice(&file_hash);
    }

    // The final digest is SHA-256 of the canonical buffer.
    let digest: [u8; 32] = Sha256::digest(&canonical).into();
    Ok(digest)
}

/// Recursively collect all files under `dir`, returning (relative_path, absolute_path) pairs.
/// `bundle.sig` is excluded. Relative paths use `/` as separator on all platforms.
fn collect_files(
    dir: &Path,
    bundle_root: &Path,
    bundle_name: &str,
) -> Result<Vec<(String, std::path::PathBuf)>, SigError> {
    let mut result: Vec<(String, std::path::PathBuf)> = Vec::new();

    let read_dir: std::fs::ReadDir =
        std::fs::read_dir(dir).map_err(|e: std::io::Error| SigError::Io {
            path: dir.display().to_string(),
            source: e,
        })?;

    for entry_result in read_dir {
        let entry: std::fs::DirEntry = entry_result.map_err(|e: std::io::Error| SigError::Io {
            path: dir.display().to_string(),
            source: e,
        })?;

        let abs_path: std::path::PathBuf = entry.path();

        let file_type: std::fs::FileType =
            entry
                .file_type()
                .map_err(|e: std::io::Error| SigError::Io {
                    path: abs_path.display().to_string(),
                    source: e,
                })?;

        if file_type.is_dir() {
            let mut sub: Vec<(String, std::path::PathBuf)> =
                collect_files(&abs_path, bundle_root, bundle_name)?;
            result.append(&mut sub);
        } else if file_type.is_file() {
            let rel: std::path::PathBuf = abs_path
                .strip_prefix(bundle_root)
                .unwrap_or(&abs_path)
                .to_path_buf();

            // Build a platform-independent relative path using `/` separator.
            let rel_str: String = rel
                .components()
                .map(|c: std::path::Component<'_>| {
                    c.as_os_str().to_str().ok_or_else(|| SigError::NonUtf8Path {
                        bundle: bundle_name.to_owned(),
                        path: abs_path.display().to_string(),
                    })
                })
                .collect::<Result<Vec<&str>, SigError>>()?
                .join("/");

            // Exclude bundle.sig from the digest.
            if rel_str == SIG_FILE_NAME {
                continue;
            }

            result.push((rel_str, abs_path));
        }
        // Symlinks are skipped — they are not a supported bundle artifact type.
    }

    Ok(result)
}
