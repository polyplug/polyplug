//! polyplug — core plugin runtime for the polyplug platform.

pub mod compatibility;
pub mod error;
pub mod ffi;
pub mod loader;
pub mod logger;
pub mod reload;
pub mod runtime;
pub mod runtime_builder;
pub mod runtime_store;

pub use reload::ReloadEvent;

/// Rust host API for loader-backed and in-process guest bundles.
pub use runtime::Runtime;
