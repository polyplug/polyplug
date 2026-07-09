//! polyplug_common — shared schema TYPES for polyplug bundles.
//!
//! This crate holds the `manifest.toml` schema (`ManifestData` and friends) plus
//! its *intrinsic, pure* behaviour: TOML deserialization, self-validation, and
//! dependency resolution. It performs no I/O, no logging, and no runtime
//! orchestration — those stay in the `polyplug` runtime.
//!
//! It exists so the `polyplugc` CLI can accept exactly what the runtime accepts
//! without depending on the runtime crate: both depend on `polyplug_common`.

pub mod manifest;

pub use manifest::{ManifestData, ManifestDependency, ManifestError, RawManifestDependency};
