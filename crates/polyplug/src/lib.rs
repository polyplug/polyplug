//! polyplug — core plugin runtime for the polyplug platform.

pub mod compatibility;
pub mod error;
pub mod ffi;
pub mod host_bridge;
pub mod loader;
pub mod logger;
pub mod reload;
pub mod runtime;
pub mod runtime_builder;
pub mod runtime_store;

pub use reload::ReloadEvent;

/// Rust host APIs for loader-backed and statically linked guest bundles.
pub use runtime::{EmbeddedBundle, EmbeddedContract, Runtime};
