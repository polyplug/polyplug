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

pub use reload::ReloadEvent;
pub use reload::ReloadPhase;
pub use runtime_config::RuntimeConfig;