//! polyplug — core plugin runtime for the polyplug platform.

pub mod compatibility;
pub mod error;
pub mod ffi;
pub mod host_bridge;
pub mod loader;
pub mod registry;
pub mod reload;
pub mod runtime;
pub mod runtime_builder;
mod runtime_config;

// Import from polyplug_abi (moved in Phase 01-03)
pub use polyplug_abi::runtime::{RuntimeConfig, Compatibility};

// Keep ReloadPhase and ReloadEvent exports (internal Rust types)
pub use reload::{ReloadPhase, ReloadEvent};

// Re-export Runtime for loader crates
pub use runtime::Runtime;