//! Runtime configuration and lifecycle types.
//!
//! This module houses all runtime-related types moved from the polyplug crate:
//! - `Compatibility` - version compatibility enforcement modes
//! - `RuntimeConfig` - configuration for runtime creation
//! - `ReloadPhase` - FFI-safe reload phase for hot-reload callbacks
//! - `ReloadPhaseType` - type of reload phase for FFI

mod compatibility;
mod reload_phase;
mod runtime_config;
mod signature_policy;

pub use compatibility::Compatibility;
pub use reload_phase::{ReloadPhase, ReloadPhaseType};
pub use runtime_config::RuntimeConfig;
pub use signature_policy::SignaturePolicy;
