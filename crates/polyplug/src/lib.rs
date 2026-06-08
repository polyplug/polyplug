//! polyplug — core plugin runtime for the polyplug platform.

pub mod compatibility;
pub mod error;
pub mod ffi;
pub mod host_bridge;
pub mod loader;
pub mod reload;
pub mod runtime;
pub mod runtime_builder;
pub mod runtime_store;

pub use polyplug_abi::runtime::ReloadPhase;
pub use polyplug_abi::runtime::{Compatibility, RuntimeConfig, UnloadMode};
pub use reload::ReloadEvent;

// Re-export Runtime for loader crates
pub use runtime::Runtime;
