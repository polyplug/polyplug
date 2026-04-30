//! Loader — bundle loading via BundleLoader trait.
//!
//! The runtime dispatches each bundle to the loader whose `runtime_name()`
//! matches the `runtime` field in the bundle's `manifest.toml`.

mod bundle_loader;
mod loaded_bundle;
pub mod manifest;
pub mod scanner;

pub use bundle_loader::BundleLoader;
pub(crate) use loaded_bundle::LoadedBundle;
pub use manifest::{ManifestData, ManifestDependency, RawManifestDependency, parse_manifest};
