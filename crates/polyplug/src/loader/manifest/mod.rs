//! Manifest — manifest.toml parsing for plugin bundles.
//!
//! Reads the companion `manifest.toml` for a plugin bundle before loading.
//! The `runtime` field determines which `BundleLoader` handles the bundle.
//! If absent, defaults to `"native"`.

use serde::Deserialize;

fn default_runtime() -> String {
    "native".to_owned()
}

/// Data parsed from a bundle's `manifest.toml`.
///
/// Only `runtime` is read in this epic. Additional fields are added in Epic 12.
#[derive(Debug, Deserialize)]
pub struct ManifestData {
    /// The runtime required to load this bundle.
    /// Matched against `BundleLoader::runtime_name()` during dispatch.
    /// Defaults to `"native"` when the field is absent from the TOML file.
    #[serde(default = "default_runtime")]
    pub runtime: String,
}
