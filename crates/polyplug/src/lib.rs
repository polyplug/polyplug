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
pub use polyplug_abi::runtime::{Compatibility, RuntimeConfig};
pub use reload::ReloadEvent;
pub use runtime::{clear_init_bundle_id, get_init_bundle_id, set_init_bundle_id};

// Re-export Runtime for loader crates
pub use runtime::Runtime;
