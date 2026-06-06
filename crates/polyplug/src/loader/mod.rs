//! Loader — bundle loading via BundleLoader trait.
//!
//! The runtime dispatches each bundle to the loader whose `runtime_name()`
//! matches the `runtime` field in the bundle's `manifest.toml`.

mod bundle_loader;
pub mod manifest;
pub mod scanner;

pub use bundle_loader::BundleLoader;
pub use manifest::{ManifestData, ManifestDependency, RawManifestDependency, parse_manifest};
pub use scanner::{ScanDiagnostic, ScanResult, scan_dirs};
