//! Manifest — re-exports of manifest types from polyplug core.
//!
//! The Rust host SDK re-exports manifest types from the polyplug crate
//! rather than defining duplicates. This ensures single source of truth.

// Re-export manifest types from polyplug core
pub use polyplug::loader::{ManifestData, ManifestDependency, RawManifestDependency, parse_manifest};