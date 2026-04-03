//! Runtime configuration and lifecycle types.
//!
//! This module houses all runtime-related types moved from the polyplug crate:
//! - `Compatibility` - version compatibility enforcement modes
//! - `RuntimeConfig` - configuration for runtime creation
//! - `ReloadPhaseData` - FFI-safe reload phase data for hot-reload callbacks
//! - `ReloadPhaseType` - type of reload phase for FFI

mod compatibility;
mod runtime_config;
mod reload_phase_data;

pub use compatibility::Compatibility;
pub use runtime_config::RuntimeConfig;
pub use reload_phase_data::{ReloadPhaseData, ReloadPhaseType};