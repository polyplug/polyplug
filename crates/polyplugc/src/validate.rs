//! Validate — assembled bundle directory validation for `polyplugc validate
//! --bundle-dir`.
//!
//! This mode does NOT re-implement manifest parsing. It drives the shared
//! schema type (`polyplug_common::ManifestData::parse_from_str` +
//! `ManifestData::validate`) so the CLI accepts exactly what the runtime would
//! accept: the same per-platform `[file]` resolution and the same
//! `id == fnv1a_64(name)` tamper check. On top of that it adds the checks that
//! only make sense for an already-assembled directory: the resolved entry file
//! must exist on disk and its extension must be consistent with the declared
//! `loader`.

use core::str::FromStr;
use std::ffi::OsStr;
use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;

use polyplug_abi::types::Version;
use polyplug_codegen::PolyplugcError;
use polyplug_common::{ManifestData, ManifestError};

/// Validate an assembled bundle directory.
///
/// On success the caller prints `OK: <dir>`; on failure a `PolyplugcError` with
/// a specific, field-named message is returned (exit non-zero).
pub fn validate_bundle_dir(dir: &Path) -> Result<(), PolyplugcError> {
    if !dir.is_dir() {
        return Err(PolyplugcError::ValidationFailed {
            message: format!(
                "bundle dir `{}` does not exist or is not a directory",
                dir.display()
            ),
        });
    }

    // Parse the manifest with the shared schema type. `parse_from_str` performs
    // per-platform `[file]` resolution (`linux.x86_64 = "..."`) into the `file`
    // field for the active platform, exactly as the runtime does at load time.
    let manifest_path: PathBuf = dir.join("manifest.toml");
    let content: String =
        read_to_string(&manifest_path).map_err(|e| PolyplugcError::ValidationFailed {
            message: format!(
                "manifest.toml: failed to read `{}`: {e}",
                manifest_path.display()
            ),
        })?;
    let mut manifest: ManifestData =
        ManifestData::parse_from_str(&content).map_err(|e: ManifestError| {
            PolyplugcError::ValidationFailed {
                message: format!("manifest.toml: {e}"),
            }
        })?;
    // Record the bundle dir so validation diagnostics carry a real path.
    manifest.path = dir.to_path_buf();

    // Reuse the shared structural + identity validation: `loader`, `name`,
    // `file` presence, `id != 0`, `id == fnv1a_64(name)` (tamper check) and
    // `provides` / `bundle_dependencies` version-spec grammar.
    manifest
        .validate()
        .map_err(|e: ManifestError| PolyplugcError::ValidationFailed {
            message: format!("manifest.toml: {e}"),
        })?;

    // The version string must parse as a `Version` (the loader stores it raw, so
    // validate it explicitly here for a clear, early error).
    if !manifest.version.is_empty() && Version::from_str(&manifest.version).is_err() {
        return Err(PolyplugcError::ValidationFailed {
            message: format!(
                "manifest.toml: field `version` = \"{}\" is not a valid version (expected major.minor[.patch])",
                manifest.version
            ),
        });
    }

    // The resolved entry artifact must exist inside the bundle directory.
    let artifact: PathBuf = dir.join(&manifest.file);
    if !artifact.is_file() {
        return Err(PolyplugcError::ValidationFailed {
            message: format!(
                "manifest.toml: field `file` resolves to \"{}\" for this platform, but `{}` does not exist in the bundle dir",
                manifest.file,
                artifact.display()
            ),
        });
    }

    // The artifact extension must be consistent with the declared runtime.
    check_extension_matches_runtime(&manifest)?;

    Ok(())
}

/// Verify the entry file's extension is one the declared `loader` would load.
fn check_extension_matches_runtime(manifest: &ManifestData) -> Result<(), PolyplugcError> {
    let extension: &str = Path::new(&manifest.file)
        .extension()
        .and_then(|e: &OsStr| e.to_str())
        .unwrap_or("");

    let allowed: &[&str] = match manifest.loader.as_str() {
        "native" => &["so", "dylib", "dll"],
        "lua" => &["lua"],
        "python" => &["py"],
        "js-quickjs" => &["js"],
        "dotnet" => &["dll"],
        other => {
            return Err(PolyplugcError::UnsupportedLanguage {
                lang: other.to_owned(),
            });
        }
    };

    if !allowed.contains(&extension) {
        return Err(PolyplugcError::ValidationFailed {
            message: format!(
                "manifest.toml: field `file` = \"{}\" has extension `.{extension}`, which is not valid for loader `{}` (expected one of: {})",
                manifest.file,
                manifest.loader,
                allowed
                    .iter()
                    .map(|e: &&str| format!(".{e}"))
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
        });
    }
    Ok(())
}
