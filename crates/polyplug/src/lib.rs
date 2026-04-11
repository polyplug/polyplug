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

// Import from polyplug_abi (moved in Phase 01-03)
pub use polyplug_abi::runtime::{RuntimeConfig, Compatibility};

// Keep ReloadEvent export (internal Rust type)
pub use reload::ReloadEvent;

// Re-export ReloadPhase from polyplug_abi (single FFI-first type)
pub use polyplug_abi::runtime::ReloadPhase;

// Re-export TLS functions for loaders
pub use runtime::{set_init_bundle_id, clear_init_bundle_id, get_init_bundle_id};

// Re-export Runtime for loader crates
pub use runtime::Runtime;

// Re-export host_* functions for null-host testing (used by integration tests)
pub use runtime::{
    host_load_bundle,
    host_reload_bundle,
    host_get_last_error,
    host_get_error_len,
    host_resolve_guest_contract,
};