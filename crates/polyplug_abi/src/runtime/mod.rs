//! Runtime configuration and lifecycle types.
//!
//! This module houses all runtime-related types moved from the polyplug crate:
//! - `Compatibility` - version compatibility enforcement modes
//! - `RuntimeConfig` - configuration for runtime creation
//! - `ReloadPhase` - FFI-safe reload phase for hot-reload callbacks
//! - `ReloadPhaseType` - type of reload phase for FFI
//! - `UnloadMode` - how a bundle's loader resources are reclaimed on unload

mod compatibility;
mod reload_phase;
mod runtime_config;
mod unload_mode;

pub use compatibility::Compatibility;
pub use reload_phase::{ReloadPhase, ReloadPhaseType};
pub use runtime_config::RuntimeConfig;
pub use unload_mode::UnloadMode;
