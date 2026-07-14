//! Loader — bundle loading via BundleLoader trait.
//!
//! The runtime dispatches each bundle to the loader whose `loader_name()`
//! matches the `loader` field in the bundle's `manifest.toml`.

mod bundle_loader;
mod bundle_source;
pub mod manifest;
pub mod scanner;

pub use bundle_loader::BundleLoader;
pub use bundle_source::{BundleOrigin, BundleSource};
// Manifest schema types (`ManifestData`, `ManifestDependency`,
// `RawManifestDependency`) live in `polyplug_common`; import them from there.
// Only the runtime-side filesystem reader is re-exported here.
pub use manifest::parse_manifest;
pub use scanner::{ScanDiagnostic, ScanResult, scan_dirs};
