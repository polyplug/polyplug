//! Canonical digest computation for bundle signing.
//!
//! See `lib.rs` module-level documentation for the full algorithm specification.

use std::fs::{self, DirEntry, FileType, ReadDir};
use std::io::Error as IoError;
use std::path::{Component, Path, PathBuf, StripPrefixError};

use sha2::{Digest, Sha256};

use crate::SigError;

/// The name of the detached signature file — excluded from the digest.
pub(crate) const SIG_FILE_NAME: &str = "bundle.sig";

/// Domain-separation tag prepended to the canonical buffer so a bundle digest can
/// never collide with any other SHA-256 pre-image produced by a different protocol.
const DOMAIN_SEP_TAG: &[u8; 20] = b"polyplug-bundle-sig\0";

/// Canonical-digest algorithm version, prepended after the domain-separation tag.
const DIGEST_ALGO_VERSION: u8 = 0x01;

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
    let mut entries: Vec<(String, PathBuf)> = collect_files(bundle_dir, bundle_dir, &bundle_name)?;

    // A signable bundle must contain at least one file.
    if entries.is_empty() {
        return Err(SigError::EmptyBundle {
            bundle: bundle_name,
        });
    }

    // Sort lexicographically by relative path bytes for determinism.
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    // Build the canonical buffer:
    //   domain-separation tag || algo version || file count (u64 LE)
    //   followed by, for each file in sorted order:
    //   relative_path_utf8 + 0x00 + sha256(file_bytes)
    let mut canonical: Vec<u8> = Vec::new();
    canonical.extend_from_slice(DOMAIN_SEP_TAG);
    canonical.push(DIGEST_ALGO_VERSION);
    let file_count: u64 = entries.len() as u64;
    canonical.extend_from_slice(&file_count.to_le_bytes());

    for (rel_path, abs_path) in &entries {
        let file_bytes: Vec<u8> = fs::read(abs_path).map_err(|e: IoError| SigError::Io {
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
) -> Result<Vec<(String, PathBuf)>, SigError> {
    let mut result: Vec<(String, PathBuf)> = Vec::new();

    let read_dir: ReadDir = fs::read_dir(dir).map_err(|e: IoError| SigError::Io {
        path: dir.display().to_string(),
        source: e,
    })?;

    for entry_result in read_dir {
        let entry: DirEntry = entry_result.map_err(|e: IoError| SigError::Io {
            path: dir.display().to_string(),
            source: e,
        })?;

        let abs_path: PathBuf = entry.path();

        let file_type: FileType = entry.file_type().map_err(|e: IoError| SigError::Io {
            path: abs_path.display().to_string(),
            source: e,
        })?;

        // A symlink is rejected outright: it is excluded from the digest but the
        // loader would still `dlopen` its target, a signature bypass.
        if file_type.is_symlink() {
            return Err(SigError::SymlinkNotAllowed {
                bundle: bundle_name.to_owned(),
                path: abs_path.display().to_string(),
            });
        }

        if file_type.is_dir() {
            let mut sub: Vec<(String, PathBuf)> =
                collect_files(&abs_path, bundle_root, bundle_name)?;
            result.append(&mut sub);
        } else if file_type.is_file() {
            let rel: PathBuf = abs_path
                .strip_prefix(bundle_root)
                .map_err(|_: StripPrefixError| SigError::PathOutsideBundle {
                    bundle: bundle_name.to_owned(),
                    path: abs_path.display().to_string(),
                })?
                .to_path_buf();

            // Build a platform-independent relative path using `/` separator.
            let rel_str: String = rel
                .components()
                .map(|c: Component<'_>| {
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
        } else {
            // A signable bundle is a plain tree of regular files and directories;
            // fifos, sockets, and device nodes are rejected.
            return Err(SigError::IrregularFile {
                bundle: bundle_name.to_owned(),
                path: abs_path.display().to_string(),
            });
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::fs;
    use std::process::{Command, ExitStatus};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn empty_bundle_is_rejected() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        assert!(matches!(
            canonical_digest(tmp.path()),
            Err(SigError::EmptyBundle { .. })
        ));
    }

    #[test]
    fn bundle_with_only_sig_file_is_rejected_as_empty() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        fs::write(tmp.path().join(SIG_FILE_NAME), b"ignored").expect("write sig");
        assert!(matches!(
            canonical_digest(tmp.path()),
            Err(SigError::EmptyBundle { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_file_in_bundle_is_rejected() {
        use std::os::unix::fs::symlink;

        let tmp: TempDir = TempDir::new().expect("tmp dir");
        fs::write(tmp.path().join("real.so"), b"\x7fELF").expect("write real");

        // Point a symlink at a target outside the bundle.
        let outside: TempDir = TempDir::new().expect("outside dir");
        let target: PathBuf = outside.path().join("evil.so");
        fs::write(&target, b"evil bytes").expect("write evil");
        symlink(&target, tmp.path().join("artifact.so")).expect("create symlink");

        assert!(matches!(
            canonical_digest(tmp.path()),
            Err(SigError::SymlinkNotAllowed { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn fifo_in_bundle_is_rejected_as_irregular() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        let fifo_path: PathBuf = tmp.path().join("pipe");

        // Create a fifo via the `mkfifo` utility to avoid a `libc` dependency. Skip
        // the assertion only if the utility is unavailable on this host.
        let status: Option<ExitStatus> = Command::new("mkfifo").arg(&fifo_path).status().ok();
        match status {
            Some(s) if s.success() => {}
            _ => return,
        }

        assert!(matches!(
            canonical_digest(tmp.path()),
            Err(SigError::IrregularFile { .. })
        ));
    }

    #[test]
    fn nested_subdirectory_files_are_covered_by_the_digest() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        fs::write(tmp.path().join("manifest.toml"), b"name = \"x\"").expect("write manifest");
        let nested: PathBuf = tmp.path().join("lib").join("inner");
        fs::create_dir_all(&nested).expect("create nested dirs");
        fs::write(nested.join("deep.so"), b"deep bytes").expect("write deep");

        let before: [u8; 32] = canonical_digest(tmp.path()).expect("digest before");

        // Mutate the nested file → the digest must change.
        fs::write(nested.join("deep.so"), b"DEEP TAMPERED").expect("rewrite deep");
        let after: [u8; 32] = canonical_digest(tmp.path()).expect("digest after");

        assert_ne!(
            before, after,
            "a nested-subdirectory file must be covered by the canonical digest"
        );
    }
}
